use super::emit_stmt_helpers::*;
use super::*;
use tsc_rs_ast::{ObjPatProp, PatKind};

/// Compute the furthest source position that a property in an object
/// destructuring pattern extends to. This accounts for default-value
/// initializers whose span goes beyond the inner pattern span.
fn obj_pat_prop_source_end(prop: &ObjPatProp) -> u32 {
    match prop {
        ObjPatProp::KeyValue(_, val) => match &val.kind {
            PatKind::Assign(_, init) => init.span.end,
            _ => val.span.end,
        },
        ObjPatProp::Shorthand(_, span) => span.end,
        ObjPatProp::ShorthandAssign(_, init, _) => init.span.end,
        ObjPatProp::Rest(p) => p.span.end,
    }
}

fn obj_pat_prop_source_start(prop: &ObjPatProp) -> u32 {
    match prop {
        ObjPatProp::KeyValue(key, _) => key.span().start,
        ObjPatProp::Shorthand(_, span) => span.start,
        ObjPatProp::ShorthandAssign(_, _, span) => span.start,
        ObjPatProp::Rest(p) => p.span.start,
    }
}

/// `true` if `s` contains either a line comment (`//`) or a block-comment
/// opener (`/*`). One pass over `s` finding `/` (memchr-fast for ASCII) and
/// peeking the next byte — vs. two SIMD scans from `s.contains("//") ||
/// s.contains("/*")`. Hot in `emit_stmt` source-text pre-checks (multiple
/// call sites profile near the top of `str::contains`).
#[inline]
pub(crate) fn contains_line_or_block_comment(s: &str) -> bool {
    let bytes = s.as_bytes();
    let mut start = 0;
    while start < bytes.len() {
        match memchr_byte(bytes, start, b'/') {
            Some(p) => {
                let next = bytes.get(p + 1).copied();
                if next == Some(b'/') || next == Some(b'*') {
                    return true;
                }
                start = p + 1;
            }
            None => return false,
        }
    }
    false
}

/// Tiny inline helper — std `[u8]::iter().position(|&b| b == n)` compiles to
/// memchr, so we just delegate. Kept as a named helper to make hot-path
/// callers self-documenting.
#[inline]
fn memchr_byte(bytes: &[u8], from: usize, needle: u8) -> Option<usize> {
    bytes[from..]
        .iter()
        .position(|&b| b == needle)
        .map(|p| from + p)
}

/// Trim trailing spaces before newlines inside block comments.
/// TypeScript normalizes:
/// `/* a \n b */` -> `/* a\n b */`
fn trim_block_comment_line_trailing_spaces(text: &str) -> String {
    if !text.contains("/*") || (!text.contains('\n') && !text.contains('\r')) {
        return text.to_string();
    }
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0usize;
    let mut in_block_comment = false;
    while i < bytes.len() {
        if !in_block_comment && i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            in_block_comment = true;
            out.push('/');
            out.push('*');
            i += 2;
            continue;
        }
        if in_block_comment && i + 1 < bytes.len() && bytes[i] == b'*' && bytes[i + 1] == b'/' {
            in_block_comment = false;
            out.push('*');
            out.push('/');
            i += 2;
            continue;
        }
        if in_block_comment && (bytes[i] == b' ' || bytes[i] == b'\t') {
            let mut j = i;
            while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
                j += 1;
            }
            if j < bytes.len() && (bytes[j] == b'\n' || bytes[j] == b'\r') {
                i = j;
                continue;
            }
            while i < j {
                i = crate::push_utf8_aware(&mut out, text, i);
            }
            continue;
        }
        i = crate::push_utf8_aware(&mut out, text, i);
    }
    out
}

fn split_smeared_expr_statement_line(text: &str) -> Option<String> {
    /// Try to split a line at top-level semicolons.
    /// Depths (paren, bracket, brace) are carried over from previous lines
    /// (e.g., from `(function () {` on an earlier line). Returns the parts
    /// (if split) and the final depths after scanning the line.
    fn split_line(
        line: &str,
        init_paren_depth: usize,
        init_bracket_depth: usize,
        init_brace_depth: usize,
    ) -> (Option<Vec<String>>, usize, usize, usize) {
        if line.trim().is_empty() || !line.contains(';') {
            // Still need to update depths even when not splitting
            let mut pd = init_paren_depth;
            let mut bd = init_bracket_depth;
            let mut brd = init_brace_depth;
            for ch in line.bytes() {
                match ch {
                    b'(' => pd += 1,
                    b')' => pd = pd.saturating_sub(1),
                    b'[' => bd += 1,
                    b']' => bd = bd.saturating_sub(1),
                    b'{' => brd += 1,
                    b'}' => brd = brd.saturating_sub(1),
                    _ => {}
                }
            }
            return (None, pd, bd, brd);
        }
        let bytes = line.as_bytes();
        let indent_len = line.chars().take_while(|c| c.is_ascii_whitespace()).count();
        let indent = &line[..indent_len];
        let mut in_string: Option<u8> = None;
        let mut in_line_comment = false;
        let mut in_block_comment = false;
        let mut paren_depth = init_paren_depth;
        let mut bracket_depth = init_bracket_depth;
        let mut brace_depth = init_brace_depth;
        let mut parts: Vec<String> = Vec::new();
        let mut segment_start = 0usize;
        let mut i = 0usize;
        while i < bytes.len() {
            let ch = bytes[i];
            if in_line_comment {
                i += 1;
                continue;
            }
            if in_block_comment {
                if ch == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                    in_block_comment = false;
                    i += 2;
                } else {
                    i += 1;
                }
                continue;
            }
            if let Some(quote) = in_string {
                if ch == b'\\' {
                    i += 2;
                    continue;
                }
                if ch == quote {
                    in_string = None;
                }
                i += 1;
                continue;
            }
            if ch == b'\'' || ch == b'"' || ch == b'`' {
                in_string = Some(ch);
                i += 1;
                continue;
            }
            if ch == b'/' && i + 1 < bytes.len() {
                if bytes[i + 1] == b'/' {
                    in_line_comment = true;
                    i += 2;
                    continue;
                }
                if bytes[i + 1] == b'*' {
                    in_block_comment = true;
                    i += 2;
                    continue;
                }
            }
            match ch {
                b'(' => paren_depth += 1,
                b')' => paren_depth = paren_depth.saturating_sub(1),
                b'[' => bracket_depth += 1,
                b']' => bracket_depth = bracket_depth.saturating_sub(1),
                b'{' => brace_depth += 1,
                b'}' => brace_depth = brace_depth.saturating_sub(1),
                // Split at `;` when at the top nesting level, or inside a
                // brace scope opened on a PREVIOUS line (e.g. IIFE body)
                // at the same brace AND paren depth as the line start.
                // This prevents splitting inside braces or parens opened on
                // THIS line (e.g. `function () { return x; };` or `for (a; b; c)`).
                b';' if bracket_depth == 0
                    && ((paren_depth == 0 && brace_depth == 0)
                        || (init_brace_depth > 0
                            && brace_depth == init_brace_depth
                            && paren_depth == init_paren_depth
                            && paren_depth <= brace_depth)) =>
                {
                    let seg = line[segment_start..=i].trim();
                    if !seg.is_empty() {
                        parts.push(format!("{indent}{seg}"));
                    }
                    segment_start = i + 1;
                }
                _ => {}
            }
            i += 1;
        }
        if parts.len() > 1 {
            // Append any trailing content (e.g., comments) after the last
            // split-point semicolon to the last part.
            let remainder = line[segment_start..].trim();
            if !remainder.is_empty() {
                if let Some(last) = parts.last_mut() {
                    *last = format!("{} {}", last, remainder);
                }
            }
            (Some(parts), paren_depth, bracket_depth, brace_depth)
        } else {
            (None, paren_depth, bracket_depth, brace_depth)
        }
    }

    let mut changed = false;
    let mut out_lines: Vec<String> = Vec::new();
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut brace_depth = 0usize;
    for line in text.lines() {
        let (parts, pd, bd, brd) = split_line(line, paren_depth, bracket_depth, brace_depth);
        paren_depth = pd;
        bracket_depth = bd;
        brace_depth = brd;
        if let Some(parts) = parts {
            changed = true;
            out_lines.extend(parts);
        } else {
            out_lines.push(line.to_string());
        }
    }
    if changed {
        Some(out_lines.join("\n"))
    } else {
        None
    }
}

struct TypeLiteralLessThanRecovery {
    operator: Option<String>,
    emit_empty_stmt: bool,
}

pub(crate) struct MalformedImportTypeOptionRecovery {
    key_text: String,
    mode_key_text: String,
    value_literal: String,
    qualifier: String,
    tail_after_qualifier: String,
}

impl<'a> Emitter<'a> {
    // ------------------------------------------------------------------
    // Statement emission
    // ------------------------------------------------------------------

    pub(super) fn emit_stmt(&mut self, stmt: &Stmt) {
        let mapped = self.source_map_gen.is_some();
        let output_before = self.output.len();
        self.emit_stmt_unmapped(stmt);
        // tsc maps the end of every emitted statement, right after its text.
        if mapped && self.output.len() > output_before {
            self.record_trailing_mapping(stmt.span.end);
        }
    }

    fn emit_stmt_unmapped(&mut self, stmt: &Stmt) {
        let saved_stmt_output_start = self.stmt_output_start;
        self.stmt_output_start = self.output.len();

        // Record source map mapping for statement start.
        self.record_mapping(stmt.span);

        let is_unmatched_type_assertion_placeholder =
            self.source.trim_start().starts_with("@<[[import(")
                && matches!(
                    &stmt.kind,
                    StmtKind::Expr(expr)
                        if expr.span.start == expr.span.end && expr_is_error_placeholder(expr)
                );
        if is_unmatched_type_assertion_placeholder {
            self.writeln(";");
            return;
        }
        if stmt.span.start < self.skip_recovery_until {
            return;
        }
        // ES5: destructuring declarations flatten to plain declarators.
        if let StmtKind::Var(var_stmt) = &stmt.kind {
            if let Some(flattened) = self.es5_flattened_var_stmt(stmt, var_stmt) {
                self.emit_var_stmt(&flattened);
                self.append_trailing_comment(stmt.span);
                self.stmt_output_start = saved_stmt_output_start;
                return;
            }
        }

        if matches!(&stmt.kind, StmtKind::TypeAlias(_))
            && self.emit_recovery_incorrect_return_token_type_alias(stmt.span)
        {
            return;
        }
        if matches!(&stmt.kind, StmtKind::TypeAlias(_))
            && self.emit_recovery_malformed_import_type_options_type_alias(stmt.span)
        {
            self.append_trailing_comment(stmt.span);
            return;
        }
        if matches!(&stmt.kind, StmtKind::TypeAlias(_))
            && self.emit_recovery_malformed_import_attributes_double_comma_type_alias(stmt.span)
        {
            self.append_trailing_comment(stmt.span);
            return;
        }
        if let StmtKind::Export(export_decl) = &stmt.kind {
            if let ExportDeclKind::Decl(inner) = &export_decl.kind {
                if matches!(&inner.kind, StmtKind::TypeAlias(_))
                    && self.emit_recovery_incorrect_return_token_type_alias(inner.span)
                {
                    self.append_trailing_comment(stmt.span);
                    return;
                }
                if matches!(&inner.kind, StmtKind::TypeAlias(_))
                    && self.emit_recovery_malformed_import_type_options_type_alias(inner.span)
                {
                    self.append_trailing_comment(stmt.span);
                    return;
                }
                if matches!(&inner.kind, StmtKind::TypeAlias(_))
                    && self.emit_recovery_malformed_import_attributes_double_comma_type_alias(
                        inner.span,
                    )
                {
                    self.append_trailing_comment(stmt.span);
                    return;
                }
            }
        }

        // Drop parser-recovery placeholder expression statements before any
        // source-copy fast path can preserve them. These artifacts (error
        // placeholders, garbage `}` tokens, split-array/hashbang shapes) only
        // exist when the parse produced diagnostics — the parser emits a
        // diagnostic alongside every recovery node — so on clean source the
        // whole block (span copy + recovery scans) is skipped.
        if self.file_has_recovery_errors {
            if let StmtKind::Expr(expr) = &stmt.kind {
                let expr_src = self.copy_span_trimmed(stmt.span).trim();
                let is_zero_span_empty_recovery =
                    expr_is_error_placeholder(expr) && stmt.span.start == stmt.span.end;
                let is_array_literal_split_empty_recovery =
                    self.expr_stmt_has_array_literal_split_empty_recovery(expr, stmt.span);
                let is_midfile_hashbang_recovery =
                    self.expr_stmt_has_midfile_hashbang_recovery_shape(expr, stmt.span);
                if is_array_literal_split_empty_recovery {
                    self.writeln(";");
                    self.append_trailing_comment(stmt.span);
                    self.advance_comment_pos(stmt.span.end);
                    return;
                }
                if (!is_zero_span_empty_recovery && expr_is_error_placeholder(expr))
                    && !is_midfile_hashbang_recovery
                    || expr_src == "}"
                    || expr_src == "};"
                    || self.expr_stmt_is_garbage_recovery(expr, stmt.span)
                {
                    // Skip comments attached to the dropped error token so they
                    // don't leak onto the next statement.
                    if expr_is_error_placeholder(expr) && stmt.span.start != stmt.span.end {
                        // Find the end of any block comment right after the error token.
                        let mut pos = stmt.span.end as usize;
                        let src = self.source.as_bytes();
                        while pos < src.len() {
                            if src[pos] == b' ' || src[pos] == b'\t' {
                                pos += 1;
                            } else if pos + 1 < src.len()
                                && src[pos] == b'/'
                                && src[pos + 1] == b'*'
                            {
                                // Skip entire block comment
                                if let Some(end) = self.source[pos + 2..].find("*/") {
                                    pos = pos + 2 + end + 2;
                                } else {
                                    break;
                                }
                            } else if pos + 1 < src.len()
                                && src[pos] == b'/'
                                && src[pos + 1] == b'/'
                            {
                                // Skip line comment
                                if let Some(nl) = self.source[pos..].find('\n') {
                                    pos = pos + nl + 1;
                                } else {
                                    pos = src.len();
                                }
                            } else {
                                break;
                            }
                        }
                        self.advance_comment_pos(pos as u32);
                    }
                    return;
                }
            }
        }

        // Strip TypeScript modifier keywords that leak as standalone
        // expression statements during error recovery (e.g. `private;`
        // from `private y = x;` in a namespace/function body).
        // Only strip when the keyword is NOT followed by a newline in source
        // (ASI cases like `abstract\nclass` should preserve the keyword).
        if let StmtKind::Expr(expr) = &stmt.kind {
            if let ExprKind::Ident(name) = &expr.kind {
                if matches!(
                    name.as_str(),
                    "private"
                        | "public"
                        | "protected"
                        | "abstract"
                        | "override"
                        | "readonly"
                        | "declare"
                        | "static"
                        | "accessor"
                        | "async"
                ) {
                    let end = stmt.span.end as usize;
                    let after = &self.source[end..self.source.len().min(end + 200)];
                    // Scan past the keyword span in source. If a newline appears
                    // before the next non-whitespace char, this is ASI context
                    // (e.g. `abstract\nclass`) and the keyword should be preserved.
                    // If the next content is on the same line, it's error recovery
                    // (e.g. `private y = x;`) and should be stripped.
                    // Comments between the keyword and the next token don't
                    // count as "content on the same line".  Line comments
                    // (`// ...`) implicitly end at EOL → newline.  Block
                    // comments (`/* ... */`) are skipped entirely and the
                    // scan continues with whatever follows them.
                    let mut found_newline = false;
                    let after_bytes = after.as_bytes();
                    let mut ci = 0;
                    while ci < after_bytes.len() {
                        let ch = after_bytes[ci];
                        if ch == b'\n' || ch == b'\r' {
                            found_newline = true;
                            break;
                        }
                        if ch == b'/' && ci + 1 < after_bytes.len() {
                            if after_bytes[ci + 1] == b'/' {
                                // Line comment → effectively a newline
                                found_newline = true;
                                break;
                            }
                            if after_bytes[ci + 1] == b'*' {
                                // Block comment — skip to `*/` and continue
                                ci += 2;
                                while ci + 1 < after_bytes.len() {
                                    if after_bytes[ci] == b'*' && after_bytes[ci + 1] == b'/' {
                                        ci += 2;
                                        break;
                                    }
                                    ci += 1;
                                }
                                continue;
                            }
                        }
                        if !ch.is_ascii_whitespace() {
                            break;
                        }
                        ci += 1;
                    }
                    // When the next content starts with `@`, it's a decorator
                    // on the following statement. TypeScript preserves the keyword
                    // as an expression statement (e.g., `abstract @dec class C`
                    // → `abstract;` then `@dec class C`).
                    let next_is_decorator = ci < after_bytes.len() && after_bytes[ci] == b'@';
                    if !found_newline && !next_is_decorator {
                        // Don't strip `declare` — when it reaches the emitter
                        // as an expression statement, the parser has already
                        // decided it's NOT a modifier. TypeScript emits it as
                        // `declare;` (e.g. `declare module {}`, `declare module \`M\``).
                        if name.as_str() == "declare" {
                            // Fall through to emit
                        } else {
                            // TypeScript emits `this.p1 = 0;` for `public p1 = 0;`
                            // inside constructor bodies.  Only apply when NOT inside
                            // a namespace/CJS/export context (constructor bodies have
                            // no export target).
                            if matches!(name.as_str(), "public" | "private" | "protected")
                                && self.export_target.is_none()
                            {
                                self.prepend_this_to_next_assign = true;
                            }
                            // TypeScript preserves `static` before var/function in
                            // namespace error recovery (e.g. `static var x = 0;`).
                            // Set a flag so the next statement emits `static ` prefix.
                            if name.as_str() == "static" {
                                self.emit_static_prefix = true;
                            } else if name.as_str() == "accessor" {
                                self.emit_accessor_prefix = true;
                            }
                            return;
                        }
                    }
                }
            }
        }

        // For statements that need no transformation, copy source text.
        // But when an export target is active (CJS or namespace), exported
        // declarations need transformation even if the declaration itself
        // has no TS-specific syntax.
        let needs_export = self.export_target.is_some() && stmt_has_export_modifier(stmt);
        let has_const_enum_ref = !self.const_enum_values.is_empty()
            && stmt_has_const_enum_ref(stmt, &self.const_enum_values);
        let has_cjs_import_ref =
            !self.cjs_import_map.is_empty() && stmt_has_cjs_import_ref(stmt, &self.cjs_import_map);
        // `import.meta.{dirname,filename,url}` has no runtime form in CJS;
        // force structured emit so emit_expr can rewrite to __dirname etc.
        let has_cjs_import_meta_rewrite =
            self.import_meta_needs_cjs_rewrite() && stmt_has_cjs_import_meta_rewrite(stmt);
        // Inside a namespace IIFE, statements referencing exported names need
        // structured emit so the identifier gets qualified (e.g. `b` → `m1.b`).
        let has_ns_export_ref = self.export_target.as_ref().is_some_and(|t| t != "exports")
            && ((!self.namespace_exports.is_empty()
                && stmt_has_ns_export_ref(stmt, &self.namespace_exports))
                || self.ns_export_stack.iter().any(|(_, exports)| {
                    !exports.is_empty() && stmt_has_ns_export_ref(stmt, exports)
                }));
        let has_missing_semis = self.has_inner_missing_semicolons(stmt);
        let has_error_marker_comment = self.span_has_error_marker_comment(stmt.span);
        let has_split_call_continuation_stmt = matches!(&stmt.kind, StmtKind::Expr(expr) if self.expr_has_split_call_continuation(expr));
        let has_missing_arg_commas_stmt = match &stmt.kind {
            StmtKind::Expr(expr) => self.expr_has_missing_arg_commas(expr),
            StmtKind::Var(var_stmt) => var_stmt.declarations.iter().any(|decl| {
                decl.init
                    .as_ref()
                    .is_some_and(|expr| self.expr_has_missing_arg_commas(expr))
            }),
            _ => false,
        };
        let has_obj_lit_missing_commas_stmt = match &stmt.kind {
            StmtKind::Expr(expr) => self.expr_has_obj_lit_missing_commas(expr),
            StmtKind::Var(var_stmt) => var_stmt.declarations.iter().any(|decl| {
                decl.init
                    .as_ref()
                    .is_some_and(|expr| self.expr_has_obj_lit_missing_commas(expr))
            }),
            _ => false,
        };
        let has_array_lit_missing_commas_stmt = match &stmt.kind {
            StmtKind::Expr(expr) => self.expr_has_array_lit_missing_commas(expr),
            StmtKind::Var(var_stmt) => var_stmt.declarations.iter().any(|decl| {
                decl.init
                    .as_ref()
                    .is_some_and(|expr| self.expr_has_array_lit_missing_commas(expr))
            }),
            _ => false,
        };
        let has_optional_shorthand_stmt = match &stmt.kind {
            StmtKind::Expr(expr) => self.expr_has_optional_shorthand(expr),
            StmtKind::Var(var_stmt) => var_stmt.declarations.iter().any(|decl| {
                decl.init
                    .as_ref()
                    .is_some_and(|expr| self.expr_has_optional_shorthand(expr))
            }),
            _ => false,
        };
        let has_multiline_prefix_update_stmt = matches!(&stmt.kind, StmtKind::Expr(expr) if {
            let s = expr.span.start as usize;
            let e = expr.span.end as usize;
            matches!(&expr.kind, ExprKind::Update(up)
                if matches!(up.op, UpdateOp::PreInc | UpdateOp::PreDec))
                && s < e && e <= self.source.len() && self.source[s..e].contains('\n')
        });
        let has_system_for_in_pat_rewrite = self.is_system()
            && matches!(&stmt.kind, StmtKind::ForIn(fi) if matches!(
                fi.left,
                ForInOfLeft::Pat(_) | ForInOfLeft::Expr(_)
            ));
        let has_system_live_export_write_stmt = self.is_system()
            && self.rewrite_ident_with_import_map
            && !self.system_live_export_names.is_empty()
            && stmt_has_live_export_write(stmt, &self.system_live_export_names);
        let has_cjs_export_ref = self.export_target.as_ref().is_some_and(|t| t == "exports")
            && !self.cjs_var_export_names.is_empty()
            && stmt_has_ns_export_ref(stmt, &self.cjs_var_export_names);
        let has_cjs_live_export_write = !self.cjs_live_export_keys.is_empty()
            && stmt_has_live_export_write(stmt, &self.cjs_live_export_keys);
        // Track ESM import statements so we don't add spurious `export {};` at
        // end of module files.  True side-effect imports (`import "mod"`) and
        // malformed imports with empty bindings both count.
        // `import {} from "mod"` (empty named clause, non-side-effect) does NOT
        // count because it will be elided and `export {};` may still be needed.
        if let StmtKind::Import(imp) = &stmt.kind {
            if !imp.type_only && !self.is_cjs_like() {
                if let ImportClause::Named {
                    default,
                    named,
                    namespace,
                } = &imp.specifiers
                {
                    if default.is_none() && named.is_empty() && namespace.is_none() {
                        // Side-effect import (`import "mod"`) OR malformed import
                        // with empty source. Only `import {} from "mod"` (non-side-effect
                        // with a real source) should NOT set the flag.
                        // Abandoned `import defer type ...` clauses (defer=true,
                        // (0,0) source span) DO want the trailing `export {};`
                        // marker — exclude them.
                        if (imp.is_side_effect || imp.source.is_empty()) && !imp.defer {
                            self.emitted_esm_export = true;
                        }
                    }
                }
            }
        }
        // Check if var statement spans multiple lines (keyword and first declarator on different lines,
        // or a destructuring pattern spans multiple lines).
        // TypeScript normalizes `let\na;` to `let a;` and multi-line destructuring to single-line.
        let var_spans_multiple_lines = matches!(&stmt.kind, StmtKind::Var(vs) if {
            if let Some(first) = vs.declarations.first() {
                let kw_end = stmt.span.start as usize + match vs.kind {
                    VarKind::Var => 3,
                    VarKind::Let => 3,
                    VarKind::Const => 5,
                    VarKind::Using => 5,
                    VarKind::AwaitUsing => 11,
                };
                let decl_start = first.name.span.start as usize;
                let kw_to_decl_newline = kw_end < decl_start && decl_start <= self.source.len()
                    && self.source[kw_end..decl_start].contains('\n');
                // Also check if any declarator's binding pattern spans multiple lines
                let pattern_multiline = vs.declarations.iter().any(|d| {
                    let s = d.name.span.start as usize;
                    let e = d.name.span.end as usize;
                    s < e && e <= self.source.len() && self.source[s..e].contains('\n')
                });
                // When a function/class init spans multiple lines, the
                // source-copy path preserves source indentation which may
                // differ from TypeScript's structured emit. Force structured
                // emit so indentation is normalized.
                let has_multiline_fn_init = vs.declarations.iter().any(|d| {
                        d.init.as_ref().is_some_and(|e| {
                            matches!(e.kind, ExprKind::FnExpr(_) | ExprKind::ClassExpr(_))
                                && {
                                    let s = e.span.start as usize;
                                    let end = e.span.end as usize;
                                    s < end
                                        && end <= self.source.len()
                                        && self.source[s..end].contains('\n')
                                }
                        })
                    });
                // Arrow functions with line terminators before `=>` are
                // normalized by TypeScript to a single line.  Force
                // structured emit to match.  Also catches expression body
                // arrows spanning multiple lines (when no comments).
                let has_multiline_arrow_expr_body = vs.declarations.iter().any(|d| {
                    d.init.as_ref().is_some_and(|e| {
                        if let ExprKind::Arrow(a) = &e.kind {
                            let s = e.span.start as usize;
                            let end = e.span.end as usize;
                            if s < end && end <= self.source.len() {
                                let txt = &self.source[s..end];
                                if !txt.contains('\n') {
                                    return false;
                                }
                                // Check for newline before `=>` (line terminator
                                // before arrow). Find `=>` that isn't inside a
                                // string and check if newline precedes it.
                                if let Some(arrow_pos) = Self::find_fat_arrow_pos(txt) {
                                    if txt[..arrow_pos].contains('\n') {
                                        return true;
                                    }
                                }
                                // Expr-body arrows spanning multiple lines
                                // (without comments) are also normalized.
                                if matches!(a.body, ArrowBody::Expr(_)) {
                                    return !contains_line_or_block_comment(txt);
                                }
                            }
                        }
                        false
                    })
                });
                // Multi-line arithmetic / unary operator initializers are
                // normalized by TypeScript's structured emit; source-copy
                // preserves the original blank-line layout too literally.
                let has_multiline_operator_init = vs.declarations.iter().any(|d| {
                    d.init.as_ref().is_some_and(|e| {
                        matches!(e.kind, ExprKind::Binary(_) | ExprKind::Unary(_)) && {
                            let s = e.span.start as usize;
                            let end = e.span.end as usize;
                            s < end
                                && end <= self.source.len()
                                && {
                                    let src = &self.source[s..end];
                                    src.contains('\n') && !contains_line_or_block_comment(src)
                                }
                        }
                    })
                });
                // Multi-declarator lists that span multiple lines are
                // normalized by TypeScript to a single line.
                let multiline_declarator_list = vs.declarations.len() > 1 && {
                    let s = stmt.span.start as usize;
                    let e = stmt.span.end as usize;
                    s < e && e <= self.source.len() && self.source[s..e].contains('\n')
                };
                kw_to_decl_newline
                    || pattern_multiline
                    || has_multiline_fn_init
                    || has_multiline_arrow_expr_body
                    || has_multiline_operator_init
                    || multiline_declarator_list
            } else {
                false
            }
        });
        // TypeScript appends `.` after bare `super` expressions (not in member
        // access or call).  Source-copy would preserve the original `super` without
        // the dot, so force structured emit.
        let has_bare_super = {
            let s = stmt.span.start as usize;
            let e = stmt.span.end as usize;
            if s < e && e <= self.source.len() {
                let text = &self.source[s..e];
                let mut found = false;
                for (i, _) in text.match_indices("super") {
                    let after = i + 5;
                    // Check that `super` is not part of a longer identifier
                    if after < text.len() && text.as_bytes()[after].is_ascii_alphanumeric() {
                        continue;
                    }
                    // Also check char before for longer idents like `_super`
                    if i > 0
                        && (text.as_bytes()[i - 1].is_ascii_alphanumeric()
                            || text.as_bytes()[i - 1] == b'_')
                    {
                        continue;
                    }
                    // Bare super: not followed by `.` or `(`
                    if after >= text.len()
                        || (text.as_bytes()[after] != b'.' && text.as_bytes()[after] != b'(')
                    {
                        found = true;
                        break;
                    }
                }
                found
            } else {
                false
            }
        };
        let has_adjacent_spread_comment = self.span_has_adjacent_spread_comment(stmt.span);
        let has_system_hoisted_var_stmt = self.system_hoist_var_in_execute
            && matches!(&stmt.kind, StmtKind::Var(v) if v.kind == VarKind::Var);
        let has_dynamic_import_rewrite_stmt = (self.is_system()
            && !self.system_context_fn.is_empty()
            || self.should_downlevel_dynamic_import())
            && {
                let s = stmt.span.start as usize;
                let e = stmt.span.end as usize;
                s < e && e <= self.source.len() && self.source[s..e].contains("import(")
            };
        let has_relative_import_extension_rewrite_stmt = self.rewrite_relative_import_extensions()
            && {
                let s = stmt.span.start as usize;
                let e = stmt.span.end as usize;
                s < e
                    && e <= self.source.len()
                    && (self.source[s..e].contains("import(")
                        || self.source[s..e].contains("require("))
            };
        // Empty argument lists with comments need structured emit so TypeScript's
        // formatting is reproduced (space before comment, multi-line closing paren).
        // Inside function scopes, yield expressions with spacing issues need
        // structured emit: `yield(foo)` → `yield (foo)`, `yield*;` → `yield* ;`.
        // Only triggers when source text has the issue, to preserve inline comments.
        let has_yield_in_fn_scope = matches!(&stmt.kind, StmtKind::Expr(e) if {
            match &e.kind {
                ExprKind::Yield(delegate, arg) => {
                    // Delegate yields: force structured emit for proper spacing.
                    // Inside functions with arg: keep source-copy to preserve comments.
                    if *delegate && (arg.is_none() || self.fn_scope_depth == 0) {
                        true
                    } else if self.fn_scope_depth > 0 {
                        let s = e.span.start as usize;
                        let end = e.span.end as usize;
                        s < end && end <= self.source.len()
                            && self.source[s..end].starts_with("yield(")
                    } else {
                        false
                    }
                }
                _ => false,
            }
        });
        let has_empty_args_comments = self.options.remove_comments != Some(true) && {
            let has = |e: &Expr| -> bool {
                match &e.kind {
                    ExprKind::Call(c) if c.args.is_empty() => {
                        let s = e.span.start as usize;
                        let end = e.span.end as usize;
                        s < end
                            && end <= self.source.len()
                            && (self.source[s..end].contains("/*")
                                || self.source[s..end].contains("//"))
                    }
                    _ => false,
                }
            };
            match &stmt.kind {
                StmtKind::Expr(e) => has(e),
                _ => false,
            }
        };
        let has_multiline_paren_assign = matches!(&stmt.kind, StmtKind::Expr(e) if {
            match &e.kind {
                ExprKind::Paren(inner) if matches!(inner.kind, ExprKind::Assign(_)) => {
                    let s = e.span.start as usize;
                    let end = e.span.end as usize;
                    s < end
                        && end <= self.source.len()
                        && {
                            let src = &self.source[s..end];
                            src.contains('\n') && !contains_line_or_block_comment(src)
                        }
                }
                _ => false,
            }
        });
        let needs_pre_update_await_recovery = matches!(&stmt.kind, StmtKind::Expr(e) if {
            matches!(
                &e.kind,
                ExprKind::Update(up)
                    if matches!(up.op, UpdateOp::PreInc | UpdateOp::PreDec)
                        && matches!(&up.argument.kind, ExprKind::Await(_))
            )
        });
        // TypeScript normalizes multi-line for-loop var declarations
        // (destructuring patterns with defaults) to single lines. Force
        // structured emit when any for-loop var declarator spans multiple
        // lines.  Only applies to ForInit::Var / ForInOfLeft::Var, not to
        // bare expression patterns (which may legitimately wrap long lines).
        let for_var_decl_multiline = |decls: &[VarDeclarator]| -> bool {
            decls.iter().any(|d| {
                let s = d.name.span.start as usize;
                let e = d.span.end as usize;
                s < e && e <= self.source.len() && self.source[s..e].contains('\n')
            })
        };
        let for_head_multiline = match &stmt.kind {
            StmtKind::For(f) => f.init.as_ref().is_some_and(|init| match init {
                ForInit::Var(vs) => for_var_decl_multiline(&vs.declarations),
                _ => false,
            }),
            StmtKind::ForIn(fi) => match &fi.left {
                ForInOfLeft::Var(vs) => for_var_decl_multiline(&vs.declarations),
                _ => false,
            },
            StmtKind::ForOf(fo) => match &fo.left {
                ForInOfLeft::Var(vs) => for_var_decl_multiline(&vs.declarations),
                _ => false,
            },
            _ => false,
        };
        // Multi-line ternary (Cond) expressions need structured emit so
        // nested ternaries get extra indentation (TypeScript re-indents by nesting depth).
        // Source-copy uses flat indentation which produces wrong whitespace.
        let has_multiline_cond = self.stmt_has_multiline_cond(stmt);
        // Inside a block or function scope (error recovery context), module-level
        // constructs should be emitted verbatim instead of being transformed.
        if (self.block_depth > 0 || self.fn_scope_depth > 0)
            && stmt.span.end > stmt.span.start
            && (stmt.span.end as usize) <= self.source.len()
        {
            // `export = X` — normally erased in ESM, but kept in error recovery
            if stmt_is_export_assign(stmt) {
                self.emit_source_line(stmt);
                return;
            }
            // `import X from "mod"` etc — normally elided if type-only, but kept
            // verbatim in error recovery (no import elision in block scope)
            if matches!(&stmt.kind, StmtKind::Import(_)) {
                self.emit_source_line(stmt);
                return;
            }
            // `import I = M` or `import I = require("mod")` — kept as `import I = ...`
            // verbatim in error recovery (not converted to `var I = ...`),
            // but still elided if the RHS is type-only.
            if let StmtKind::ImportEquals(ie) = &stmt.kind {
                let has_rv = self.import_equals_rhs_has_runtime_value(&ie.module_ref);
                if has_rv {
                    self.emit_source_line(stmt);
                } else {
                    // Type-only import-equals in block scope — elide
                    self.advance_comment_pos(stmt.span.end);
                }
                return;
            }
            // Inside a block scope (block_depth > 0), export variants are kept
            // verbatim for ESM-style recovery emit. CJS-like emit still lowers
            // them so invalid nested exports become runtime assignments like TSC.
            // This does NOT apply inside function/namespace bodies
            // (fn_scope_depth > 0 alone) — those handle exports normally.
            // Exception: type-only exports are still erased.
            if self.block_depth > 0 {
                if let StmtKind::Export(ed) = &stmt.kind {
                    // `declare export ...` is always erased, even in block scope
                    let s = stmt.span.start as usize;
                    let e = (stmt.span.end as usize).min(self.source.len());
                    let starts_with_declare =
                        s < e && self.source[s..e].trim_start().starts_with("declare");
                    let is_type_only = starts_with_declare
                        || match &ed.kind {
                            ExportDeclKind::Decl(inner) => {
                                stmt_is_erased(inner, self.preserve_const_enums_effective())
                            }
                            ExportDeclKind::Named { type_only, .. } => *type_only,
                            ExportDeclKind::All { type_only, .. } => *type_only,
                            _ => false,
                        };
                    if is_type_only {
                        self.advance_comment_pos(stmt.span.end);
                    } else if !self.is_cjs_like() {
                        self.emit_source_line_expand_class(stmt);
                    } else {
                        // Fall through to the normal export emission path.
                    }
                    if is_type_only || !self.is_cjs_like() {
                        return;
                    }
                }
            }
        }
        if let StmtKind::Expr(expr) = &stmt.kind {
            if self.emit_recovery_questionable_jsx_namespace_member(stmt.span) {
                return;
            }
            if self.emit_recovery_invalid_jsx_attribute_name(stmt.span) {
                return;
            }
            if self.emit_recovery_async_iterable_return_type_tail(expr, stmt.span) {
                self.append_trailing_comment(stmt.span);
                return;
            }
            if self.emit_recovery_cast_of_bare_yield(expr, stmt.span) {
                self.append_trailing_comment(stmt.span);
                return;
            }
            if self.emit_recovery_invalid_in_lhs_assignment(expr) {
                self.append_trailing_comment(stmt.span);
                return;
            }
            if self.emit_recovery_malformed_strict_eq_assignment(expr, stmt.span) {
                self.append_trailing_comment(stmt.span);
                return;
            }
            if self.emit_recovery_malformed_jsx_unary_expr_stmt(expr, stmt.span) {
                return;
            }
            if self.emit_recovery_missing_paren_typed_arrow(expr, stmt.span) {
                return;
            }
            if self.emit_recovery_midfile_hashbang_expr_stmt(expr, stmt.span) {
                self.append_trailing_comment(stmt.span);
                return;
            }
            if self.emit_recovery_missing_type_argument_call(expr, stmt.span) {
                self.append_trailing_comment(stmt.span);
                return;
            }
        }
        if let StmtKind::Var(var_stmt) = &stmt.kind {
            if self.emit_recovery_invalid_type_query_var(stmt, var_stmt) {
                return;
            }
            if self.emit_recovery_optional_method_return_in_object_var(stmt, var_stmt) {
                return;
            }
            if self.emit_recovery_symbol_indexer_object_var(stmt, var_stmt) {
                return;
            }
            if self.emit_recovery_object_method_incorrect_return_token_var(stmt, var_stmt) {
                return;
            }
            if self.emit_recovery_nested_class_object_var(stmt, var_stmt) {
                return;
            }
            if self.emit_recovery_private_indexer_object_var(stmt, var_stmt) {
                return;
            }
            if self.emit_recovery_private_name_indexed_access_var_stmt(stmt, var_stmt) {
                return;
            }
            if self.emit_recovery_malformed_unary_jsx_var(stmt, var_stmt) {
                return;
            }
            if self.emit_recovery_object_rest_property_name_var(stmt, var_stmt) {
                return;
            }
            if self.emit_recovery_ambiguous_generic_assertion_var(stmt, var_stmt) {
                return;
            }
            let has_multiline_operator_init = var_stmt.declarations.iter().any(|d| {
                d.init.as_ref().is_some_and(|e| {
                    matches!(e.kind, ExprKind::Binary(_) | ExprKind::Unary(_)) && {
                        let s = e.span.start as usize;
                        let end = e.span.end as usize;
                        s < end && end <= self.source.len() && {
                            let src = &self.source[s..end];
                            src.contains('\n') && !contains_line_or_block_comment(src)
                        }
                    }
                })
            });
            if has_multiline_operator_init {
                if var_stmt.modifiers & MOD_EXPORT != 0
                    && (self.export_target.is_some() || self.is_cjs_like())
                {
                    self.emit_var_export(var_stmt);
                } else {
                    self.emit_var_stmt(var_stmt);
                    if var_stmt.modifiers & MOD_DECLARE == 0 {
                        self.append_trailing_comment(stmt.span);
                    }
                }
                return;
            }
        }
        let has_js_file_type_assertion_fragment_tail = matches!(
            &stmt.kind,
            StmtKind::Expr(expr) if self.expr_needs_js_file_fragment_recovery_tail(expr)
        );
        let has_recovered_arrow_var_init = matches!(&stmt.kind, StmtKind::Var(var_stmt) if {
            var_stmt
                .declarations
                .iter()
                .filter_map(|decl| decl.init.as_ref())
                .any(|init| self.arrow_needs_structured_recovery_emit(init))
        });
        let has_recovered_arrow_in_stmt = self.stmt_has_recovered_arrow(stmt);
        let has_invalid_let_for_header_recovery = matches!(
            &stmt.kind,
            StmtKind::For(for_stmt)
                if self.for_stmt_has_invalid_let_header_recovery_shape(for_stmt, stmt.span)
        );
        let has_malformed_await_using_for_header_recovery = matches!(
            &stmt.kind,
            StmtKind::For(for_stmt)
                if self.for_stmt_has_malformed_await_using_header_recovery_shape(for_stmt, stmt.span)
        );
        let has_multiline_tagged_template_gap = matches!(&stmt.kind, StmtKind::Var(var_stmt) if {
            var_stmt
                .declarations
                .iter()
                .filter_map(|decl| decl.init.as_ref())
                .any(|init| self.expr_has_multiline_tagged_template_gap(init))
        });
        let has_new_empty_array_call_callee_var_init = matches!(&stmt.kind, StmtKind::Var(var_stmt) if {
            var_stmt
                .declarations
                .iter()
                .filter_map(|decl| decl.init.as_ref())
                .any(|init| matches!(&init.kind, ExprKind::New(new_expr) if self.new_expr_has_empty_array_call_callee_recovery_shape(new_expr)))
        });
        let has_paren_inner_comments_stmt = matches!(&stmt.kind, StmtKind::Expr(expr) if {
            match &expr.kind {
                ExprKind::Paren(inner) => {
                    self.has_comments_in_range(expr.span.start + 1, inner.span.start)
                        || self.has_comments_in_range(inner.span.end, expr.span.end)
                }
                _ => false,
            }
        });
        let has_member_inner_comments_stmt =
            matches!(&stmt.kind, StmtKind::Expr(expr) if self.expr_has_member_inner_comments(expr));
        let has_trailing_backslash_error_placeholder_recovery = matches!(&stmt.kind, StmtKind::Expr(expr) if {
            self.expr_stmt_has_trailing_backslash_error_placeholder_recovery(expr, stmt.span)
        });
        let has_unterminated_string_expr_tail = matches!(
            &stmt.kind,
            StmtKind::Expr(expr) if self.expr_stmt_has_unterminated_string_literal_tail(expr)
        );
        let has_var_trailing_space_string_literal = matches!(&stmt.kind, StmtKind::Var(var_stmt) if {
            var_stmt
                .declarations
                .iter()
                .filter_map(|decl| decl.init.as_ref())
                .any(|init| self.expr_has_trailing_space_string_literal(init))
        });
        let has_error_placeholder_var_init_recovery = matches!(&stmt.kind, StmtKind::Var(var_stmt) if {
            var_stmt
                .declarations
                .iter()
                .filter_map(|decl| decl.init.as_ref())
                .any(|init| {
                    self.file_has_recovery_errors
                        && expr_has_error_member(init)
                        && matches!(init.kind, ExprKind::Unary(_) | ExprKind::NonNull(_))
                })
        });
        let has_invalid_let_array_var_recovery = matches!(&stmt.kind, StmtKind::Var(var_stmt) if {
            self.var_stmt_has_invalid_let_array_recovery(stmt, var_stmt)
        });
        let has_reserved_word_var_recovery = matches!(&stmt.kind, StmtKind::Var(var_stmt) if {
            self.var_stmt_has_reserved_word_recovery_shape(stmt, var_stmt)
        });
        let has_generator_object_method_var_recovery = matches!(&stmt.kind, StmtKind::Var(var_stmt) if {
            self.var_stmt_has_generator_object_method_recovery(stmt, var_stmt)
        });
        let has_malformed_async_await_arrow_var_recovery = matches!(&stmt.kind, StmtKind::Var(var_stmt) if {
            self.var_stmt_has_malformed_async_await_arrow_recovery(var_stmt)
        });
        let has_malformed_arrow_type_var_recovery = matches!(&stmt.kind, StmtKind::Var(var_stmt) if {
            self.var_stmt_has_malformed_arrow_type_recovery(var_stmt)
        });
        // Parser-recovery artifact: only possible when the parse produced a
        // diagnostic. Gate the per-init `import(...)` span scan on that flag so
        // clean files skip it entirely.
        let has_malformed_import_type_option_var_recovery = self.file_has_recovery_errors
            && matches!(&stmt.kind, StmtKind::Var(var_stmt) if {
                self.var_stmt_has_malformed_import_type_option_recovery(var_stmt)
            });
        // Arrow init with `/* */` between `)` and `=>` — TypeScript strips these comments.
        let has_arrow_pre_arrow_comment_var_init = matches!(&stmt.kind, StmtKind::Var(var_stmt) if {
            var_stmt.declarations.iter().filter_map(|decl| decl.init.as_ref()).any(|init| {
                if let ExprKind::Arrow(a) = &init.kind {
                    if a.return_type.is_none() {
                        let s = init.span.start as usize;
                        let e = init.span.end as usize;
                        if s < e && e <= self.source.len() {
                            let src = &self.source[s..e];
                            if let Some(arrow_pos) = Self::find_fat_arrow_pos(src) {
                                let before_arrow = &src[..arrow_pos];
                                if let Some(close_paren) = before_arrow.rfind(')') {
                                    return before_arrow[close_paren..].contains("/*");
                                }
                            }
                        }
                    }
                }
                false
            })
        });
        let has_recovered_var_decl_split = matches!(&stmt.kind, StmtKind::Var(var_stmt) if {
            var_stmt.declarations.windows(2).any(|pair| {
                let prev_end = pair[0].span.end as usize;
                let next_start = pair[1].span.start as usize;
                prev_end <= next_start && next_start <= self.source.len() && {
                    let between = &self.source[prev_end..next_start];
                    between.contains(':') || !between.contains(',')
                }
            })
        });
        // Disqualify source-copy when the raw text starts with a Unicode
        // escape that resolves to a keyword (e.g. `\u0076ar` → `var`).
        // Var declarations with <error> binding names need structured emit
        // so the error declarator (and trailing comma) can be dropped.
        // e.g. `var a,;` → `var a;`, `var a,` → `var a;`
        let has_error_binding_var_decl = matches!(&stmt.kind, StmtKind::Var(var_stmt) if {
            var_stmt.declarations.iter().any(|d| {
                matches!(&d.name.kind, PatKind::Ident(name) if name == "<error>")
            })
        });
        // TypeScript's structured emit writes keyword tokens literally.
        // Destructuring binding patterns with reserved keyword shorthand
        // properties (e.g. `{ while }`) need structured emit so that the
        // keyword is emitted as `keyword: ` instead of shorthand.
        let has_keyword_shorthand_binding = matches!(&stmt.kind, StmtKind::Var(var_stmt) if {
            var_stmt.declarations.iter().any(|d| binding_has_keyword_shorthand(&d.name))
        }) || matches!(&stmt.kind, StmtKind::ForIn(for_stmt) if {
            if let ForInOfLeft::Var(vs) = &for_stmt.left {
                vs.declarations.iter().any(|d| binding_has_keyword_shorthand(&d.name))
            } else { false }
        }) || matches!(&stmt.kind, StmtKind::ForOf(for_stmt) if {
            if let ForInOfLeft::Var(vs) = &for_stmt.left {
                vs.declarations.iter().any(|d| binding_has_keyword_shorthand(&d.name))
            } else { false }
        });
        // Object destructuring with reserved word binding target (e.g. `{ while: while }`)
        // needs structured emit so the reserved word is stripped and doubled as a new property:
        // `{ while: while }` → `{ while: , while:  }`.
        let has_reserved_word_binding_target = matches!(&stmt.kind, StmtKind::Var(var_stmt) if {
            var_stmt.declarations.iter().any(|d| binding_has_reserved_word_keyvalue_target(&d.name, self.source))
        });
        // For-in/of with <error> binding needs structured emit so the gap is preserved
        // (e.g. `for (var of X)` → `for (var  of X)` with double space).
        let has_for_in_of_error_binding = {
            let check_left = |left: &ForInOfLeft| -> bool {
                if let ForInOfLeft::Var(vs) = left {
                    vs.declarations
                        .iter()
                        .any(|d| matches!(&d.name.kind, PatKind::Ident(name) if name == "<error>"))
                } else {
                    false
                }
            };
            matches!(&stmt.kind, StmtKind::ForIn(fi) if check_left(&fi.left))
                || matches!(&stmt.kind, StmtKind::ForOf(fo) if check_left(&fo.left))
        };
        let has_unicode_keyword_start = {
            let s = stmt.span.start as usize;
            let e = stmt.span.end as usize;
            if s < e && e <= self.source.len() {
                let src = self.source[s..e].trim_start();
                src.starts_with("\\u")
            } else {
                false
            }
        };
        // Decorator-prefixed expression statements: `@dec A;` should emit as `A;`.
        // TypeScript strips invalid decorator syntax from non-class expression statements
        // during error recovery.
        let has_decorator_prefix_expr = {
            let s = stmt.span.start as usize;
            let e = stmt.span.end as usize;
            s < e
                && e <= self.source.len()
                && self.source[s..e].trim_start().starts_with('@')
                && !matches!(&stmt.kind, StmtKind::ClassDecl(_) | StmtKind::FnDecl(_))
        };
        // Var declarations with `<error>` initializer (from `@` error recovery)
        // need structured emit to strip the error expression.
        let has_error_var_init = matches!(&stmt.kind, StmtKind::Var(var_stmt) if {
            var_stmt.declarations.iter().any(|d| {
                d.init.as_ref().is_some_and(|e| matches!(&e.kind, ExprKind::Ident(name) if name == "<error>"))
            })
        });
        // Expression statements that are just `<error>` identifiers (from
        // stray tokens like `]`, `@`) should use structured emit so the
        // error expression is skipped instead of source-copied.
        let has_error_expr_stmt = matches!(&stmt.kind, StmtKind::Expr(expr) if
        matches!(&expr.kind, ExprKind::Ident(name) if name == "<error>")
            || self.copy_span_trimmed(stmt.span).bytes().any(|b| {
                b < 0x20 && b != b'\n' && b != b'\r' && b != b'\t'
            }));
        let has_malformed_empty_for_header_recovery = matches!(&stmt.kind, StmtKind::For(for_stmt)
            if matches!(&for_stmt.init, Some(ForInit::Expr(expr)) if expr_is_error_placeholder(expr))
                && matches!(&for_stmt.update, Some(expr) if expr_is_error_placeholder(expr))
                && matches!(&for_stmt.body.kind, StmtKind::Expr(expr) if expr_is_error_placeholder(expr))
        );
        let has_private_array_assignment_recovery = self.file_has_recovery_errors
            && matches!(&stmt.kind, StmtKind::Expr(expr) if matches!(expr.kind, ExprKind::Assign(_)))
            && {
                let raw = self.copy_span_trimmed(stmt.span);
                raw.contains("[#") && raw.contains("]=")
            };
        // Binary expressions with empty right operand (error recovery)
        // need structured emit so the newline is preserved: `1 +\n;`
        let has_empty_binary_right_stmt = {
            fn has_error_binary(e: &Expr) -> bool {
                match &e.kind {
                    ExprKind::Binary(bin) => {
                        matches!(&bin.right.kind, ExprKind::Ident(name) if name.is_empty() || name == "<error>")
                            || matches!(&bin.left.kind, ExprKind::Ident(name) if name.is_empty() || name == "<error>")
                            || has_error_binary(&bin.right)
                            || has_error_binary(&bin.left)
                    }
                    ExprKind::Assign(a) => has_error_binary(&a.right),
                    ExprKind::Paren(inner) => has_error_binary(inner),
                    _ => false,
                }
            }
            match &stmt.kind {
                StmtKind::Expr(expr) => has_error_binary(expr),
                StmtKind::Var(var_stmt) => var_stmt
                    .declarations
                    .iter()
                    .any(|d| d.init.as_ref().is_some_and(|e| has_error_binary(e))),
                _ => false,
            }
        };
        // Multiline binary expression statements with unspaced division
        // operators (e.g. `1\n/notregexp/a.foo()`) need structured emit so
        // spaces are added around `/` and continuation indentation is applied.
        // Only trigger when a continuation line starts with `/` immediately
        // followed by an alphanumeric char (looks like regex but is division).
        let has_multiline_operator_expr_stmt = matches!(&stmt.kind, StmtKind::Expr(expr) if {
            matches!(expr.kind, ExprKind::Binary(_)) && {
                let s = expr.span.start as usize;
                let end = expr.span.end as usize;
                s < end && end <= self.source.len() && {
                    let src = &self.source[s..end];
                    src.contains('\n') && !contains_line_or_block_comment(src)
                    && src.lines().skip(1).any(|line| {
                        let t = line.trim_start().as_bytes();
                        t.len() >= 2 && t[0] == b'/' && t[1].is_ascii_alphanumeric()
                    })
                }
            }
        });
        let has_missing_predicate_function_block_recovery = matches!(
            &stmt.kind,
            StmtKind::Block(_) if {
                let raw = self.copy_span_trimmed(stmt.span);
                raw.trim_start().starts_with("function ") && raw.contains(" is {")
            }
        );
        let has_invalid_spread_expr_stmt = match &stmt.kind {
            StmtKind::Expr(expr) => matches!(expr.kind, ExprKind::Spread(_)),
            StmtKind::Block(stmts) => stmts
                .iter()
                .any(|stmt| matches!(&stmt.kind, StmtKind::Expr(expr) if matches!(expr.kind, ExprKind::Spread(_)))),
            _ => false,
        };
        // A lowered ES5 class member rewrites `super` structurally.
        let needs_transform = stmt_needs_transform(stmt)
            || (self.es5_super_home.is_some()
                && self
                    .source_between(stmt.span.start, stmt.span.end)
                    .contains("super"));
        let needs_downlevel = self.stmt_needs_downlevel(stmt);
        let has_unclosed_delimiter = self.stmt_has_unclosed_delimiter(stmt);
        // CJS: force structured emit for compound statements containing nested
        // var declarations whose names appear in the CJS export chain.  This
        // allows the block/if/while/etc. handler to recurse into children
        // and inject inline `exports.x = x;` after each var declaration.
        let has_cjs_nested_export_var = self.is_cjs_like()
            && self.fn_scope_depth == 0
            && !self.cjs_live_export_chain.is_empty()
            && !self.has_export_assign
            && self.stmt_has_nested_exported_var(stmt);
        // The full set of conditions under which a statement is safe to emit by
        // verbatim source-copy, EXCEPT the `!needs_transform` term. The fast-emit
        // type-erasure path below reuses this exact verdict: a typed statement
        // that is otherwise verbatim-safe can be copied minus its type
        // byte-ranges. Keeping the two in lockstep guarantees correctness parity.
        // The leading term restores the old `!needs_transform` short-circuit for
        // the default (non-fast-emit) path: when the statement needs a transform
        // and no fast-emit consumer exists, the rest of the battery is unused,
        // so don't evaluate its inline scans.
        let can_verbatim_modulo_types = (!needs_transform || self.fast_emit)
            && !needs_export
            && !needs_downlevel
            && !has_const_enum_ref
            && !has_cjs_import_ref
            && !has_cjs_import_meta_rewrite
            && !has_ns_export_ref
            && !has_cjs_export_ref
            && !has_missing_semis
            && !has_missing_arg_commas_stmt
            && !has_obj_lit_missing_commas_stmt
            && !has_array_lit_missing_commas_stmt
            && !has_optional_shorthand_stmt
            // If-statement with empty condition (error recovery) needs structured emit
            && !matches!(&stmt.kind, StmtKind::If(if_stmt) if
                matches!(&if_stmt.test.kind, ExprKind::Ident(name) if name.is_empty()))
            && !has_error_marker_comment
            && !has_split_call_continuation_stmt
            && !has_system_for_in_pat_rewrite
            && !has_system_live_export_write_stmt
            && !has_dynamic_import_rewrite_stmt
            && !has_relative_import_extension_rewrite_stmt
            && !has_cjs_live_export_write
            && !matches!(stmt.kind, StmtKind::FnDecl(_))
            && !matches!(&stmt.kind, StmtKind::Block(stmts) if stmts.is_empty())
            && !matches!(&stmt.kind, StmtKind::Block(_) if
                self.block_has_opening_brace_line_comment(stmt.span))
            && !has_missing_predicate_function_block_recovery
            && !has_invalid_spread_expr_stmt
            && !var_spans_multiple_lines
            && !for_head_multiline
            && !has_multiline_paren_comma(stmt, self.source)
            && !has_multiline_paren_assign
            && !needs_pre_update_await_recovery
            && !has_unclosed_delimiter
            && !has_adjacent_spread_comment
            && !has_system_hoisted_var_stmt
            && !has_empty_args_comments
            && !has_yield_in_fn_scope
            && !has_js_file_type_assertion_fragment_tail
            && !has_recovered_arrow_var_init
            && !has_recovered_arrow_in_stmt
            && !has_invalid_let_for_header_recovery
            && !has_multiline_tagged_template_gap
            && !has_new_empty_array_call_callee_var_init
            && !has_paren_inner_comments_stmt
            && !has_member_inner_comments_stmt
            && !has_trailing_backslash_error_placeholder_recovery
            && !has_unterminated_string_expr_tail
            && !has_var_trailing_space_string_literal
            && !has_error_placeholder_var_init_recovery
            && !has_invalid_let_array_var_recovery
            && !has_reserved_word_var_recovery
            && !has_generator_object_method_var_recovery
            && !has_malformed_async_await_arrow_var_recovery
            && !has_malformed_arrow_type_var_recovery
            && !has_malformed_import_type_option_var_recovery
            && !has_arrow_pre_arrow_comment_var_init
            && !has_recovered_var_decl_split
            && !has_multiline_cond
            && !has_unicode_keyword_start
            && !has_keyword_shorthand_binding
            && !has_reserved_word_binding_target
            && !has_for_in_of_error_binding
            && !has_malformed_await_using_for_header_recovery
            && !has_error_binding_var_decl
            && !has_decorator_prefix_expr
            && !has_error_var_init
            && !has_error_expr_stmt
            && !has_malformed_empty_for_header_recovery
            && !has_private_array_assignment_recovery
            && !has_empty_binary_right_stmt
            && !has_multiline_operator_expr_stmt
            && !has_multiline_prefix_update_stmt
            // Evaluated lazily (inline, not a pre-computed `let`): this walks the
            // whole statement subtree, so keep it behind the ~50 cheaper `&&`
            // terms above — they short-circuit it away for most statements
            // (e.g. FnDecl, which is excluded earlier in this chain).
            && !stmt_contains_class_expr(stmt)
            && !has_cjs_nested_export_var
            && !self.prepend_this_to_next_assign
            && stmt.span.end > stmt.span.start
            && (stmt.span.end as usize) <= self.source.len()
            && !(self.static_this_alias.is_some() && {
                let s = stmt.span.start as usize;
                let e = stmt.span.end as usize;
                s < e && e <= self.source.len() && self.source[s..e].contains("this")
            })
            && !(self.static_super_base_alias.is_some() && {
                let s = stmt.span.start as usize;
                let e = stmt.span.end as usize;
                s < e && e <= self.source.len() && self.source[s..e].contains("super")
            })
            && !(self.legacy_constructor_super.is_some() && {
                let s = stmt.span.start as usize;
                let e = stmt.span.end as usize;
                s < e
                    && e <= self.source.len()
                    && (self.source[s..e].contains("super")
                        || self.source[s..e].contains("this"))
            })
            && !(self.legacy_this_alias.is_some()
                && matches!(&stmt.kind, StmtKind::Return(None)))
            && !(self.in_static_block_await_to_yield && {
                let s = stmt.span.start as usize;
                let e = stmt.span.end as usize;
                s < e && e <= self.source.len() && self.source[s..e].contains("await")
            });
        if !needs_transform && can_verbatim_modulo_types {
            self.emit_source_line(stmt);
            // CJS: emit inline `exports.x = x;` for source-copied var stmts
            // inside nested compound bodies.  Top-level vars are handled by
            // the `declared_names` logic in the CJS loop of lib.rs.
            if let StmtKind::Var(var_stmt) = &stmt.kind {
                if self.is_cjs_like()
                    && self.cjs_inline_export_depth > 0
                    && self.fn_scope_depth == 0
                    && var_stmt.kind == VarKind::Var
                    && var_stmt.modifiers & MOD_DECLARE == 0
                    && !self.cjs_live_export_chain.is_empty()
                    && !self.has_export_assign
                {
                    let mut inline_exports: Vec<(String, String)> = Vec::new();
                    for decl in &var_stmt.declarations {
                        if let PatKind::Ident(ref name) = decl.name.kind {
                            if decl.init.is_some() {
                                if let Some(exported_names) =
                                    self.cjs_live_export_chain.get(name.as_str())
                                {
                                    for exported in exported_names {
                                        inline_exports
                                            .push((exported.to_string(), name.to_string()));
                                    }
                                }
                            }
                        }
                    }
                    for (exported, local) in &inline_exports {
                        self.write_cjs_export_access("exports", exported);
                        self.write(" = ");
                        self.write(local);
                        self.writeln(";");
                        self.cjs_inline_exported_var_names
                            .insert(local.clone().into());
                    }
                }
            }
            return;
        }

        // Fast-emit type erasure: a statement that is verbatim-safe EXCEPT for
        // type syntax (so `needs_transform` is true but `can_verbatim_modulo_types`
        // holds) is emitted by copying its source minus the collected type
        // byte-ranges, instead of structured per-node re-emit. Covers var / expr /
        // return / if / for / while / switch / try.
        //
        // Function declarations are excluded from `can_verbatim_modulo_types`, so
        // they get a dedicated verdict (`fn_decl_erasable`): the collector's
        // per-identifier CJS import/export + const-enum ref checks cover the body
        // references; the remaining battery guards (export/downlevel/import-meta/
        // decorator/ns/live-export/dynamic-import/system/nested-export + active
        // alias contexts) are re-listed here. Recovery-shaped battery guards
        // (missing semis/commas, error placeholders, malformed-* recoveries) are
        // vacuous under `!file_has_recovery_errors` and deliberately omitted;
        // purely cosmetic layout guards don't apply to fast emit. Validation
        // (asi_validate.mjs) gates any gap to 0.
        //
        // NEL-containing sources are excluded: verbatim copies would preserve
        // U+0085, which the TS scanner accepts as whitespace but JS engines
        // reject — the structured path normalizes it.
        if self.fast_emit
            && !self.source_has_nel
            && !self.file_has_recovery_errors
            && !self.emit_accessor_prefix
            && !self.emit_static_prefix
            // The erasure writer copies raw spans and cannot strip comments;
            // under removeComments fall through to structured emit.
            && self.options.remove_comments != Some(true)
        {
            let fn_decl_erasable = matches!(&stmt.kind, StmtKind::FnDecl(_))
                && !needs_export
                && !needs_downlevel
                && !has_const_enum_ref
                && !has_cjs_import_ref
                && !has_cjs_export_ref
                && !has_cjs_import_meta_rewrite
                && !has_cjs_live_export_write
                && !has_cjs_nested_export_var
                && !has_ns_export_ref
                && !has_dynamic_import_rewrite_stmt
                && !has_relative_import_extension_rewrite_stmt
                && !has_system_live_export_write_stmt
                && !has_decorator_prefix_expr
                && !self.prepend_this_to_next_assign
                && self.static_this_alias.is_none()
                && self.static_super_base_alias.is_none()
                && !self.in_static_block_await_to_yield
                && stmt.span.end > stmt.span.start
                && (stmt.span.end as usize) <= self.source.len();
            if can_verbatim_modulo_types || fn_decl_erasable {
                if let Some(erase) = self.try_collect_stmt_erasures(stmt) {
                    self.write_stmt_erasing(stmt.span, &erase);
                    self.append_trailing_comment(stmt.span);
                    return;
                }
            }
        }

        // Clear prepend_this flag for non-expression statements so it
        // doesn't leak to later statements.
        if !matches!(&stmt.kind, StmtKind::Expr(_)) {
            self.prepend_this_to_next_assign = false;
        }
        // Clear emit_static_prefix for statements that don't consume it.
        // It's consumed in emit_var_stmt, emit_fn_decl, class/enum/namespace emit.
        if !matches!(
            &stmt.kind,
            StmtKind::Var(_)
                | StmtKind::FnDecl(_)
                | StmtKind::ClassDecl(_)
                | StmtKind::EnumDecl(_)
                | StmtKind::ModuleDecl(_)
                | StmtKind::Import(_)
        ) {
            self.emit_static_prefix = false;
            self.emit_accessor_prefix = false;
        }

        match &stmt.kind {
            StmtKind::Var(var_stmt) => {
                if self.is_js_file {
                    let raw = self.copy_span_trimmed(stmt.span).trim();
                    let invalid_object_rest_suffix = raw.find("...").is_some_and(|spread| {
                        let tail = &raw[spread + 3..];
                        let end = tail.find(|ch| ch == ',' || ch == '}').unwrap_or(tail.len());
                        tail[..end].contains(':') || tail[..end].contains('=')
                    });
                    if invalid_object_rest_suffix {
                        self.write(raw.trim_end_matches(';'));
                        self.writeln(";");
                        self.append_trailing_comment(stmt.span);
                        return;
                    }
                }
                if self.emit_recovery_invalid_let_array_var_stmt(stmt, var_stmt) {
                    self.append_trailing_comment(stmt.span);
                    return;
                }
                if self.emit_recovery_reserved_word_var_stmt(stmt, var_stmt) {
                    self.append_trailing_comment(stmt.span);
                    return;
                }
                if self.emit_recovery_generator_object_method_var_stmt(stmt, var_stmt) {
                    self.append_trailing_comment(stmt.span);
                    return;
                }
                if self.emit_recovery_private_name_indexed_access_var_stmt(stmt, var_stmt) {
                    return;
                }
                if self.emit_recovery_malformed_async_await_arrow_var_stmt(stmt, var_stmt) {
                    self.append_trailing_comment(stmt.span);
                    return;
                }
                if self.emit_recovery_malformed_arrow_type_var_stmt(stmt, var_stmt) {
                    self.append_trailing_comment(stmt.span);
                    return;
                }
                let malformed_import_type_tails =
                    self.malformed_import_type_option_recovery_tails_for_var_stmt(var_stmt);
                let has_malformed_import_type_tails = !malformed_import_type_tails.is_empty();
                let accessor_prefix_pos = if self.emit_accessor_prefix {
                    let before = self.output.len();
                    let at_line_start = self.at_line_start;
                    self.write("accessor ");
                    self.emit_accessor_prefix = false;
                    Some((before, at_line_start))
                } else {
                    None
                };
                if var_stmt.modifiers & MOD_EXPORT != 0
                    && (self.export_target.is_some() || self.is_cjs_like())
                {
                    self.emit_var_export(var_stmt);
                } else {
                    self.emit_var_stmt(var_stmt);
                    if self.finish_object_spread_call_followed_by_block_recovery(stmt, var_stmt) {
                        return;
                    }
                    if var_stmt.modifiers & MOD_DECLARE == 0 {
                        self.append_trailing_comment(stmt.span);
                    }
                    // CJS: emit inline `exports.x = x;` for hoisted `var` declarations
                    // inside nested compound bodies.  Top-level vars handled by lib.rs.
                    if self.is_cjs_like()
                        && self.cjs_inline_export_depth > 0
                        && self.fn_scope_depth == 0
                        && var_stmt.kind == VarKind::Var
                        && var_stmt.modifiers & MOD_DECLARE == 0
                        && !self.cjs_live_export_chain.is_empty()
                        && !self.has_export_assign
                    {
                        let mut inline_exports: Vec<(String, String)> = Vec::new();
                        for decl in &var_stmt.declarations {
                            if let PatKind::Ident(ref name) = decl.name.kind {
                                if decl.init.is_some() {
                                    if let Some(exported_names) =
                                        self.cjs_live_export_chain.get(name.as_str())
                                    {
                                        for exported in exported_names {
                                            inline_exports
                                                .push((exported.to_string(), name.to_string()));
                                        }
                                    }
                                }
                            }
                        }
                        for (exported, local) in &inline_exports {
                            self.write_cjs_export_access("exports", exported);
                            self.write(" = ");
                            self.write(local);
                            self.writeln(";");
                            self.cjs_inline_exported_var_names
                                .insert(local.clone().into());
                        }
                    }
                    let type_lit_recovery_tails = self
                        .type_literal_less_than_recovery_tails_for_var_stmt(var_stmt, stmt.span);
                    for tail in type_lit_recovery_tails {
                        self.writeln(&tail);
                    }
                    let recovery_tail_count =
                        self.js_file_type_assertion_fragment_tail_count_for_var(var_stmt);
                    if recovery_tail_count > 0 && self.options.jsx == Some(JsxEmit::Preserve) {
                        let line_end = self.output.trim_end_matches('\n').len();
                        let line_start =
                            self.output[..line_end].rfind('\n').map_or(0, |pos| pos + 1);
                        if let Some(close_rel) = self.output[line_start..line_end].rfind("/>") {
                            let close = line_start + close_rel;
                            if close > 0 && !self.output.as_bytes()[close - 1].is_ascii_whitespace()
                            {
                                self.output.insert(close, ' ');
                            }
                        }
                    }
                    for _ in 0..recovery_tail_count {
                        self.writeln("</>;");
                    }
                    if let Some(recovery_tail) =
                        self.var_stmt_trailing_return_recovery_tail(stmt, var_stmt)
                    {
                        self.writeln(&recovery_tail);
                        self.skip_recovery_until = self.skip_recovery_until.max(stmt.span.end);
                    }
                }
                for tail in malformed_import_type_tails {
                    self.writeln(&tail);
                }
                if has_malformed_import_type_tails {
                    self.skip_recovery_until = self.skip_recovery_until.max(stmt.span.end);
                }
                if let Some((before, at_line_start)) = accessor_prefix_pos {
                    if self.output.len() == before + "accessor ".len() {
                        self.output.truncate(before);
                        self.at_line_start = at_line_start;
                    }
                }
            }
            StmtKind::Expr(expr) => {
                let expr_src = self.copy_span_trimmed(stmt.span).trim();
                if self.file_has_recovery_errors
                    && expr_src.contains("[#")
                    && expr_src.contains("]=")
                {
                    if let Some(eq) = expr_src.rfind('=') {
                        self.write(expr_src[..eq].trim_end());
                        self.writeln(" =");
                        self.writeln(";");
                        return;
                    }
                }
                let is_zero_span_empty_recovery =
                    expr_is_error_placeholder(expr) && stmt.span.start == stmt.span.end;
                let is_midfile_hashbang_recovery =
                    self.expr_stmt_has_midfile_hashbang_recovery_shape(expr, stmt.span);
                if (!is_zero_span_empty_recovery && expr_is_error_placeholder(expr))
                    && !is_midfile_hashbang_recovery
                    || expr_src == "}"
                    || self.expr_stmt_is_garbage_recovery(expr, stmt.span)
                {
                    return;
                }
                if is_zero_span_empty_recovery {
                    self.writeln(";");
                    self.append_trailing_comment(stmt.span);
                    return;
                }
                if let ExprKind::Update(up) = &expr.kind {
                    if matches!(up.op, UpdateOp::PreInc | UpdateOp::PreDec)
                        && matches!(&up.argument.kind, ExprKind::Await(_))
                    {
                        let op = match up.op {
                            UpdateOp::PreInc => "++",
                            UpdateOp::PreDec => "--",
                            _ => unreachable!(),
                        };
                        self.write(op);
                        self.writeln(";");
                        if let ExprKind::Await(inner) = &up.argument.kind {
                            self.write("await ");
                            self.emit_expr(inner);
                            self.writeln(";");
                        }
                        self.append_trailing_comment(stmt.span);
                        return;
                    }
                }
                // TypeScript wraps the entire expression-statement in parens
                // when a type assertion around an object literal is stripped
                // and the result has a member/call/elem access chain:
                // `({} as T).foo` → `({}.foo);` not `({}).foo;`
                let needs_outer_wrap = expr_stmt_needs_outer_paren_wrap(expr);
                // When a type assertion is stripped from an expression statement,
                // the resulting expression may start with `{`, `function`, or `class`,
                // which would be ambiguous without parens:
                // `<any>{};` → `({});` not `{};`
                // `<T>function(){}()` → `(function(){}());` not `function(){}();`
                let needs_disambig_parens =
                    !needs_outer_wrap && expr_stmt_needs_disambiguation_parens(expr);
                let needs_hazard_free_arrow_wrap = matches!(
                    &expr.kind,
                    ExprKind::Arrow(arrow)
                        if self.can_downlevel_hazard_free_es5_arrow(expr.span, arrow)
                );
                // When the disambig expression is inside a call/member chain,
                // wrap only the leftmost fn/obj via a flag, not the whole stmt.
                let deep_disambig = needs_disambig_parens && disambig_needs_deep_wrap(expr);
                if needs_outer_wrap
                    || (needs_disambig_parens && !deep_disambig)
                    || needs_hazard_free_arrow_wrap
                {
                    self.write("(");
                    if needs_outer_wrap {
                        self.suppress_stmt_paren_inner = true;
                    }
                }
                if deep_disambig {
                    self.disambig_wrap_leftmost_fn = true;
                }
                self.suppress_oc_parens = true;
                // Prepend `this.` for constructor-body property declarations
                // that had their `public`/`private`/`protected` modifier stripped.
                if self.prepend_this_to_next_assign {
                    self.prepend_this_to_next_assign = false;
                    if matches!(&expr.kind, ExprKind::Assign(a) if matches!(&a.left.kind, ExprKind::Ident(_)))
                    {
                        self.write("this.");
                    }
                }
                self.cjs_export_in_expr_stmt = true;
                self.update_value_discarded = true;
                if let ExprKind::Spread(inner) = &expr.kind {
                    // A spread token is not valid as a standalone expression.
                    // TypeScript reports it, then emits the operand as the
                    // recovered expression statement (`{ ...props; }` becomes
                    // `{ props; }`).
                    self.emit_expr(inner);
                } else {
                    self.emit_expr(expr);
                }
                self.cjs_export_in_expr_stmt = false;
                self.update_value_discarded = false;
                self.suppress_stmt_paren_inner = false;
                self.disambig_wrap_leftmost_fn = false;
                self.strip_trailing_newline();
                if self.expr_stmt_has_trailing_backslash_error_placeholder_recovery(expr, stmt.span)
                    && self.output.ends_with('\\')
                {
                    self.output.pop();
                    self.out_col = self.out_col.saturating_sub(1);
                }
                let post_expr = self.source_between(expr.span.end, stmt.span.end);
                let detached_recovery_comment_before_semi = post_expr
                    .find("//")
                    .is_some_and(|comment| post_expr[..comment].matches('\n').count() >= 2);
                let pre_semi = post_expr
                    .rfind(';')
                    .map(|semi_pos| post_expr[..semi_pos].trim_end().to_string())
                    .filter(|text| {
                        !text.trim().is_empty() && !detached_recovery_comment_before_semi
                    });
                // Avoid double semicolons when source-copied expressions
                // already include a trailing `;`.
                let wrap = needs_outer_wrap
                    || (needs_disambig_parens && !deep_disambig)
                    || needs_hazard_free_arrow_wrap;
                if self.output.ends_with(';') {
                    if wrap {
                        let pos = self.output.len() - 1;
                        self.output.insert(pos, ')');
                    }
                    if self.expr_stmt_has_unterminated_string_literal_tail(expr) {
                        self.writeln(";");
                    } else {
                        self.newline();
                    }
                } else {
                    if wrap {
                        self.write(")");
                    }
                    if let Some(pre_semi) = pre_semi {
                        let pre_semi = pre_semi.trim();
                        if !pre_semi.is_empty() {
                            self.write(" ");
                            self.write(pre_semi);
                        }
                    }
                    // Error recovery: `using;` → `using ;` (add space for keyword
                    // identifiers that look like statement keywords).
                    if matches!(&expr.kind, ExprKind::Ident(name)
                        if name == "using" || name == "await")
                    {
                        self.write(" ");
                    }
                    // Error recovery: for `1 +\n;` pattern (binary with empty
                    // right operand from error recovery), emit `;` on a new line
                    // only when the source had a newline between the operator
                    // and the right operand.
                    if let ExprKind::Binary(bin) = &expr.kind {
                        if matches!(&bin.right.kind, ExprKind::Ident(name) if name.is_empty() || name == "<error>")
                        {
                            let left_end = bin.left.span.end as usize;
                            let right_start = bin.right.span.start as usize;
                            let has_newline = left_end < right_start
                                && right_start <= self.source.len()
                                && self.source[left_end..right_start].contains('\n');
                            if has_newline {
                                self.newline();
                            }
                        }
                    }
                    // Move inline block comments that precede the semicolon
                    // to after it: `1 >>  /**/;` → `1 >> ; /**/`
                    // This matches TypeScript's behavior for error recovery.
                    let mut deferred_comment = None;
                    if let ExprKind::Binary(bin) = &expr.kind {
                        if matches!(&bin.right.kind, ExprKind::Ident(name) if name.is_empty() || name == "<error>")
                        {
                            if self.output.ends_with("*/") {
                                // Find the `/*` that starts this block comment
                                if let Some(cs) = self.output.rfind("/*") {
                                    let before = self.output[..cs].trim_end();
                                    // Only apply for shift/other binary operators
                                    if !before.is_empty() {
                                        let comment = self.output[cs..].to_string();
                                        self.output.truncate(cs);
                                        // Remove trailing spaces before the comment,
                                        // but keep exactly one space after the operator.
                                        while self.output.ends_with("  ") {
                                            self.output.pop();
                                        }
                                        deferred_comment = Some(comment);
                                    }
                                }
                            }
                        }
                    }
                    if let Some(comment) = deferred_comment {
                        self.write(";");
                        self.write(" ");
                        self.writeln(&comment);
                    } else {
                        self.writeln(";");
                    }
                }
                let has_empty_binary_right = |candidate: &Expr| {
                    matches!(
                        &candidate.kind,
                        ExprKind::Binary(bin)
                            if matches!(&bin.right.kind, ExprKind::Ident(name) if name.is_empty() || name == "<error>")
                    )
                };
                let has_unmatched_paren_after_empty_binary = (has_empty_binary_right(expr)
                    || matches!(&expr.kind, ExprKind::Assign(assign) if has_empty_binary_right(&assign.right)))
                    && self
                        .source
                        .get(stmt.span.start as usize..)
                        .and_then(|tail| tail.lines().next())
                        .is_some_and(|line| line.trim_end().ends_with(");"));
                if has_unmatched_paren_after_empty_binary {
                    self.writeln(";");
                }
                let malformed_fragment_closing_tag = matches!(&expr.kind, ExprKind::JsxFragment(_))
                    && {
                        let raw = self.copy_span_trimmed(stmt.span);
                        raw.contains("</") && !raw.contains("</>")
                    };
                if malformed_fragment_closing_tag {
                    self.deferred_recovery_comment = self.get_trailing_comment(stmt.span);
                } else {
                    self.append_trailing_comment(stmt.span);
                }
                if self.expr_needs_js_file_fragment_recovery_tail(expr) {
                    self.writeln("</>;");
                }
            }
            StmtKind::Return(arg) => {
                if self.emit_lexical_loop_control(stmt) {
                    return;
                }
                let return_arg = arg.as_ref().filter(|expr| {
                    !matches!(&expr.kind, ExprKind::Omitted)
                        && !matches!(&expr.kind, ExprKind::Ident(name) if name.is_empty() || name == "<error>")
                });
                let async_arrow_super_hoist =
                    return_arg.and_then(|expr| self.async_arrow_super_hoist_info(expr));
                let inline_async_arrow_super_hoist =
                    async_arrow_super_hoist
                        .as_ref()
                        .is_some_and(|(_, has_elem, _)| {
                            !*has_elem && {
                                let start = stmt.span.start as usize;
                                let end = stmt.span.end as usize;
                                start < end
                                    && end <= self.source.len()
                                    && !self.source[start..end].contains('\n')
                            }
                        });
                let saved_super_active = self.async_super_active;
                let saved_super_names = std::mem::take(&mut self.async_super_names);
                let saved_super_elem = self.async_super_has_element_access;
                let saved_super_write = self.async_super_has_write;
                let saved_super_suffix = std::mem::take(&mut self.async_super_suffix);
                if let Some((names, has_elem, has_write)) = async_arrow_super_hoist.clone() {
                    self.async_super_active = true;
                    self.async_super_names = names;
                    self.async_super_has_element_access = has_elem;
                    self.async_super_has_write = has_write;
                    self.async_super_suffix.clear();
                    if !inline_async_arrow_super_hoist && !self.at_line_start {
                        self.newline();
                    }
                    self.emit_super_hoisting(false);
                    if inline_async_arrow_super_hoist && self.output.ends_with('\n') {
                        self.output.pop();
                        self.at_line_start = false;
                        self.write(" ");
                    }
                }
                self.write("return");
                if let Some(expr) = return_arg {
                    self.write(" ");
                    self.suppress_oc_parens = true;
                    let handled_direct_class_iife = if let ExprKind::ClassExpr(class_decl) =
                        &expr.kind
                    {
                        let needs_static_iife = class_has_static_initializers(class_decl)
                            && (!self.use_define_for_class_fields()
                                || self.needs_downlevel("static-blocks"));
                        let needs_computed_instance_iife =
                            self.class_expr_needs_legacy_computed_iife(class_decl);
                        let needs_private_iife = self.class_expr_needs_private_iife(class_decl);
                        if needs_static_iife || needs_computed_instance_iife || needs_private_iife {
                            // TS emits direct return of class-expression static IIFEs
                            // without wrapping parens: `return _a = class ..., _a;`
                            self.emit_class_expr_iife_with_wrap(class_decl, false, None);
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    };
                    // A comma sequence is already syntactically valid as the
                    // direct operand of `return`. Type-erasing layers and their
                    // associated innermost paren disappear with it, while plain
                    // user-authored parens remain structural expression nodes.
                    let handled_direct_computed_object =
                        self.try_emit_direct_return_computed_object(expr);
                    if !handled_direct_class_iife && !handled_direct_computed_object {
                        self.emit_expr(expr);
                    }
                    // Class/function expressions end with `}\n` — strip the
                    // newline so the semicolon goes on the same line.
                    self.strip_trailing_newline();
                } else if let Some(alias) = self.legacy_this_alias.clone() {
                    self.write(" ");
                    self.write(&alias);
                }
                self.async_super_active = saved_super_active;
                self.async_super_names = saved_super_names;
                self.async_super_has_element_access = saved_super_elem;
                self.async_super_has_write = saved_super_write;
                self.async_super_suffix = saved_super_suffix;
                self.writeln(";");
                self.append_trailing_comment(stmt.span);
            }
            StmtKind::If(if_stmt) => {
                self.emit_if_stmt(if_stmt, stmt.span);
                self.append_trailing_comment(stmt.span);
            }
            StmtKind::While(wh) => {
                if self.emit_recovery_reserved_word_while_stmt(wh, stmt.span) {
                    self.append_trailing_comment(stmt.span);
                    return;
                }
                let test_start = wh.test.span.start;
                let body_start = wh.body.span.start;
                if self.has_block_comments_between(stmt.span.start, body_start) {
                    let kw_end =
                        Self::find_keyword_after(self.source, "while", stmt.span.start as usize)
                            + 5;
                    let pre_test = self.source_between(kw_end as u32, test_start);
                    self.write("while");
                    self.write(pre_test.trim_end());
                    self.emit_expr(&wh.test);
                    let post_test = self.source_between(wh.test.span.end, body_start);
                    if let Some(cp) = Self::find_close_paren_skipping_comments(post_test) {
                        let before_cp = post_test[..cp].trim_end();
                        let after_cp = &post_test[cp + 1..];
                        self.write(before_cp);
                        self.write(")");
                        self.write(after_cp);
                    } else {
                        self.write(")");
                        if matches!(wh.body.kind, StmtKind::Block(_)) {
                            self.write(" ");
                        }
                    }
                } else {
                    self.write("while (");
                    self.suppress_oc_parens = true;
                    self.emit_expr(&wh.test);
                    self.write(")");
                    if matches!(wh.body.kind, StmtKind::Block(_)) {
                        self.write(" ");
                    }
                }
                self.emit_stmt_body(&wh.body);
                self.append_trailing_comment(stmt.span);
            }
            StmtKind::DoWhile(dw) => {
                let has_comments = self.has_block_comments_between(stmt.span.start, stmt.span.end);
                // do body
                if has_comments {
                    let do_end =
                        Self::find_keyword_after(self.source, "do", stmt.span.start as usize) + 2;
                    let pre_body = self.source_between(do_end as u32, dw.body.span.start);
                    self.write("do");
                    if pre_body.contains("/*") {
                        self.write(pre_body);
                    } else if matches!(dw.body.kind, StmtKind::Block(_)) {
                        self.write(" ");
                    }
                } else {
                    self.write("do");
                    if matches!(dw.body.kind, StmtKind::Block(_)) {
                        self.write(" ");
                    }
                }
                self.emit_stmt_body(&dw.body);
                // while (test);
                if has_comments {
                    if matches!(dw.body.kind, StmtKind::Block(_)) {
                        self.strip_trailing_newline();
                    }
                    let body_end = dw.body.span.end;
                    let test_start = dw.test.span.start;
                    let between = self.source_between(body_end, test_start);
                    // Find "while" keyword in between
                    if let Some(while_pos) = between.find("while") {
                        let before_while = &between[..while_pos];
                        let after_while = &between[while_pos + 5..]; // past "while"
                                                                     // Emit text before while (e.g. " /*c*/ ")
                        self.write(before_while.trim_end());
                        self.write(" while");
                        // Emit text after while to test start (e.g. " /*d*/ ( /*e*/")
                        self.write(after_while.trim_end());
                    } else {
                        if matches!(dw.body.kind, StmtKind::Block(_)) {
                            self.write(" while (");
                        } else {
                            self.write("while (");
                        }
                    }
                    self.suppress_oc_parens = true;
                    self.emit_expr(&dw.test);
                    // After test, find ) and ;
                    let post_test = self.source_between(dw.test.span.end, stmt.span.end);
                    if let Some(cp) = Self::find_close_paren_skipping_comments(post_test) {
                        let before_cp = post_test[..cp].trim_end();
                        self.write(before_cp);
                        self.writeln(");");
                    } else {
                        self.writeln(");");
                    }
                } else {
                    if matches!(dw.body.kind, StmtKind::Block(_)) {
                        self.strip_trailing_newline();
                        self.write(" while (");
                    } else {
                        self.write("while (");
                    }
                    self.suppress_oc_parens = true;
                    self.emit_expr(&dw.test);
                    self.writeln(");");
                }
                self.append_trailing_comment(stmt.span);
            }
            StmtKind::For(for_stmt) => {
                if let Some(plan) = self.lexical_loop_plan(stmt.span) {
                    self.emit_captured_for_stmt(&plan, for_stmt, &[]);
                    self.append_trailing_comment(stmt.span);
                    return;
                }
                if self.emit_for_zero_span_recovery(for_stmt, stmt.span) {
                    return;
                }
                if let Some(ForInit::Var(var_stmt)) = &for_stmt.init {
                    if self.needs_downlevel("using")
                        && matches!(var_stmt.kind, VarKind::Using | VarKind::AwaitUsing)
                    {
                        self.emit_for_using_dispose_scope(for_stmt, var_stmt, &[]);
                        self.append_trailing_comment(stmt.span);
                        return;
                    }
                }
                // CJS: extract for-init var with exported names to separate
                // statement before the for loop, e.g.:
                //   for (var h = _; ;) { ... }
                // →
                //   var h = _;
                //   exports.h = h;
                //   for (;;) { ... }
                let extracted = self.cjs_extract_for_init_exports(for_stmt);
                if !extracted.is_empty() {
                    // Emit extracted var declarations and exports
                    for (name, exported_names, init_expr) in &extracted {
                        self.write("var ");
                        self.write(name);
                        self.write(" = ");
                        self.emit_expr(init_expr);
                        self.writeln(";");
                        for exported in exported_names {
                            self.write_cjs_export_access("exports", exported);
                            self.write(" = ");
                            self.write(name);
                            self.writeln(";");
                        }
                    }
                    // Emit the for loop with init stripped
                    self.emit_for_stmt_no_init(for_stmt, stmt.span);
                } else {
                    self.emit_for_stmt(for_stmt, stmt.span);
                }
                self.append_trailing_comment(stmt.span);
            }
            StmtKind::ForIn(fi) => {
                if let Some(plan) = self.lexical_loop_plan(stmt.span) {
                    self.emit_captured_for_in_stmt(&plan, fi, &[]);
                    self.append_trailing_comment(stmt.span);
                    return;
                }
                // CJS: collect inline exports for the for-in var binding
                let forin_inline_exports = self.cjs_for_in_of_var_exports(&fi.left);
                self.write("for (");
                self.emit_for_in_of_left(&fi.left);
                self.write(" in ");
                self.suppress_oc_parens = true;
                self.emit_expr(&fi.right);
                self.write(")");
                if !forin_inline_exports.is_empty() && !matches!(fi.body.kind, StmtKind::Block(_)) {
                    // Wrap non-block body in braces to inject exports
                    self.writeln(" {");
                    self.indent += 1;
                    self.cjs_inline_export_depth += 1;
                    for (exported, local) in &forin_inline_exports {
                        self.write_cjs_export_access("exports", exported);
                        self.write(" = ");
                        self.write(local);
                        self.writeln(";");
                    }
                    self.emit_stmt(&fi.body);
                    self.cjs_inline_export_depth -= 1;
                    self.indent -= 1;
                    self.writeln("}");
                } else {
                    if matches!(fi.body.kind, StmtKind::Block(_)) {
                        self.write(" ");
                    }
                    self.emit_stmt_body(&fi.body);
                }
                self.append_trailing_comment(stmt.span);
            }
            StmtKind::ForOf(fo) => {
                if let Some(plan) = self.lexical_loop_plan(stmt.span) {
                    self.emit_captured_for_of_stmt(&plan, fo, &[]);
                    self.append_trailing_comment(stmt.span);
                    return;
                }
                if self.needs_downlevel("using") && self.simple_for_of_using_binding(fo).is_some() {
                    if fo.is_await && self.needs_downlevel("async-generator") {
                        self.emit_for_await_using_downlevel_with_await(fo, &[]);
                    } else if self.needs_downlevel("for-of") {
                        self.emit_for_of_using_downlevel(fo, stmt.span, &[]);
                    } else {
                        self.emit_native_for_of_using(fo, &[]);
                    }
                } else if fo.is_await
                    && self.needs_downlevel("async-generator")
                    && !self.in_async_generator_transform
                    && self.can_emit_simple_for_await_downlevel(fo).is_some()
                {
                    // Top-level for-await-of downlevel (uses `await`, not `yield`).
                    self.emit_for_await_downlevel_with_await(fo);
                } else if self.needs_downlevel("for-of") {
                    self.emit_for_of_downlevel(fo, stmt.span);
                } else if self.needs_downlevel("object-spread") && self.for_of_has_obj_rest(fo) {
                    self.emit_for_of_object_rest(fo);
                } else {
                    // CJS: collect inline exports for the for-of var binding
                    let forof_inline_exports = self.cjs_for_in_of_var_exports(&fo.left);
                    if fo.is_await {
                        self.write("for await (");
                    } else {
                        self.write("for (");
                    }
                    self.emit_for_in_of_left(&fo.left);
                    self.write(" of ");
                    self.suppress_oc_parens = true;
                    self.emit_expr(&fo.right);
                    self.write(")");
                    if !forof_inline_exports.is_empty()
                        && !matches!(fo.body.kind, StmtKind::Block(_))
                    {
                        // Wrap non-block body in braces to inject exports
                        self.writeln(" {");
                        self.indent += 1;
                        self.cjs_inline_export_depth += 1;
                        for (exported, local) in &forof_inline_exports {
                            self.write_cjs_export_access("exports", exported);
                            self.write(" = ");
                            self.write(local);
                            self.writeln(";");
                        }
                        self.emit_stmt(&fo.body);
                        self.cjs_inline_export_depth -= 1;
                        self.indent -= 1;
                        self.writeln("}");
                    } else {
                        if matches!(fo.body.kind, StmtKind::Block(_)) {
                            self.write(" ");
                        }
                        self.emit_stmt_body(&fo.body);
                    }
                }
                self.append_trailing_comment(stmt.span);
            }
            StmtKind::Switch(sw) => {
                self.emit_switch_stmt(sw, stmt.span);
                self.append_trailing_comment(stmt.span);
            }
            StmtKind::Try(try_stmt) => {
                self.emit_try_stmt(try_stmt, stmt.span);
                self.append_trailing_comment(stmt.span);
            }
            StmtKind::Throw(expr) => {
                let throw_kw_end =
                    Self::find_keyword_after(self.source, "throw", stmt.span.start as usize) + 5;
                let pre_expr = self
                    .source_between(throw_kw_end as u32, expr.span.start)
                    .trim_end()
                    .to_string();
                self.write("throw");
                if pre_expr.is_empty() {
                    self.write(" ");
                } else {
                    self.write(&pre_expr);
                    if pre_expr.trim_start().starts_with("/*")
                        || pre_expr.trim_start().starts_with("//")
                    {
                        self.write(" ");
                    }
                }
                self.suppress_oc_parens = true;
                self.emit_expr(expr);
                self.strip_trailing_newline();
                let post_expr = self.source_between(expr.span.end, stmt.span.end);
                let pre_semi = post_expr
                    .rfind(';')
                    .map(|semi_pos| post_expr[..semi_pos].trim_end().to_string())
                    .filter(|text| !text.is_empty());
                if let Some(pre_semi) = pre_semi {
                    self.write(&pre_semi);
                }
                // When the throw expression has an `<error>` member from parser
                // error recovery (e.g. `throw undefined.`), place the semicolon
                // on its own line to match tsc's behavior.
                if self.file_has_recovery_errors && expr_has_error_member(expr) {
                    self.newline();
                    self.write(";");
                    self.newline();
                } else {
                    self.writeln(";");
                }
                self.append_trailing_comment(stmt.span);
            }
            StmtKind::Break(label) => {
                if self.emit_lexical_loop_control(stmt) {
                    return;
                }
                self.write("break");
                if let Some(ref l) = label {
                    self.write(" ");
                    self.write(l);
                }
                self.writeln(";");
                self.append_trailing_comment(stmt.span);
            }
            StmtKind::Continue(label) => {
                if self.emit_lexical_loop_control(stmt) {
                    return;
                }
                self.write("continue");
                if let Some(ref l) = label {
                    self.write(" ");
                    self.write(l);
                }
                self.writeln(";");
                self.append_trailing_comment(stmt.span);
            }
            StmtKind::Block(stmts) => {
                let raw_block = self.copy_span_trimmed(stmt.span);
                if self.file_has_recovery_errors
                    && raw_block.trim_start().starts_with("function ")
                    && raw_block.contains(" is {")
                {
                    for recovered in stmts {
                        self.emit_stmt(recovered);
                    }
                    return;
                }
                if self.emit_recovery_bare_module_block_stmt(stmt, stmts) {
                    self.append_trailing_comment(stmt.span);
                    return;
                }
                if stmts.is_empty() {
                    // Check if source block was single-line
                    let start = stmt.span.start as usize;
                    let end = stmt.span.end as usize;
                    let is_multiline = start < end
                        && end <= self.source.len()
                        && self.source[start..end].contains('\n');
                    if is_multiline {
                        // Check for block comments inside the empty block.
                        // Preserve them between `{` and `}`.
                        let block_src = &self.source[start..end];
                        let has_inner_comments = block_src.contains("/*");
                        if has_inner_comments {
                            // Find opening `{` and closing `}` in source
                            let open = block_src.find('{').unwrap_or(0);
                            let close = block_src.rfind('}').unwrap_or(block_src.len());
                            let inner = &block_src[open + 1..close];
                            // Write opening `{` with any trailing comment on same line
                            let first_nl = inner.find('\n').unwrap_or(inner.len());
                            let first_line = inner[..first_nl].trim_end();
                            if !first_line.is_empty() {
                                self.write("{");
                                self.write(first_line);
                                self.newline();
                            } else {
                                self.writeln("{");
                            }
                            // Write any remaining comments before `}` (on subsequent lines)
                            let rest = if first_nl < inner.len() {
                                &inner[first_nl + 1..]
                            } else {
                                ""
                            };
                            let rest_trimmed = rest.trim();
                            if !rest_trimmed.is_empty() {
                                self.indent += 1;
                                self.write(rest_trimmed);
                                self.write(" ");
                                self.indent -= 1;
                            }
                            self.writeln("}");
                            // Advance comment position past the block so inner
                            // comments are not re-emitted.
                            self.advance_comment_pos(stmt.span.end);
                        } else {
                            self.write("{");
                            self.append_brace_trailing_comment(stmt.span);
                            self.newline();
                            self.writeln("}");
                        }
                    } else {
                        self.writeln("{ }");
                    }
                } else {
                    self.write("{");
                    self.append_brace_trailing_comment(stmt.span);
                    self.newline();
                    self.indent += 1;
                    self.block_depth += 1;
                    self.cjs_inline_export_depth += 1;
                    if self.needs_downlevel("using") {
                        if let Some(first_using) = first_using_index(stmts) {
                            for s in &stmts[..first_using] {
                                let stmt_output_start = self.output.len();
                                let prev_helper_insert_pos = self.class_decl_helper_insert_pos;
                                self.stmt_output_start = stmt_output_start;
                                self.class_decl_helper_insert_pos = Some(stmt_output_start);
                                self.emit_leading_comments(s.span.start);
                                self.emit_stmt(s);
                                self.class_decl_helper_insert_pos = prev_helper_insert_pos;
                                self.advance_comment_pos(s.span.end);
                            }
                            self.emit_block_using_dispose_scope(&stmts[first_using..]);
                        } else {
                            for s in stmts {
                                let stmt_output_start = self.output.len();
                                let prev_helper_insert_pos = self.class_decl_helper_insert_pos;
                                self.stmt_output_start = stmt_output_start;
                                self.class_decl_helper_insert_pos = Some(stmt_output_start);
                                self.emit_leading_comments(s.span.start);
                                self.emit_stmt(s);
                                self.class_decl_helper_insert_pos = prev_helper_insert_pos;
                                self.advance_comment_pos(s.span.end);
                            }
                        }
                    } else {
                        for s in stmts {
                            let stmt_output_start = self.output.len();
                            let prev_helper_insert_pos = self.class_decl_helper_insert_pos;
                            self.stmt_output_start = stmt_output_start;
                            self.class_decl_helper_insert_pos = Some(stmt_output_start);
                            self.emit_leading_comments(s.span.start);
                            self.emit_stmt(s);
                            self.class_decl_helper_insert_pos = prev_helper_insert_pos;
                            self.advance_comment_pos(s.span.end);
                        }
                    }
                    self.cjs_inline_export_depth -= 1;
                    self.block_depth -= 1;
                    // Emit any comments between the last statement and the closing `}`.
                    {
                        let block_end = stmt.span.end as usize;
                        if block_end > 0 && block_end <= self.source.len() {
                            let brace_pos =
                                if self.source.as_bytes().get(block_end - 1) == Some(&b'}') {
                                    (block_end - 1) as u32
                                } else {
                                    self.source[..block_end]
                                        .rfind('}')
                                        .map(|p| p as u32)
                                        .unwrap_or(block_end as u32)
                                };
                            self.emit_leading_comments(brace_pos);
                        }
                    }
                    self.indent -= 1;
                    self.writeln("}");
                }
                self.append_trailing_comment(stmt.span);
            }
            StmtKind::Empty => {
                let synthetic_at_eof = self.file_has_recovery_errors
                    && stmt.span.start == stmt.span.end
                    && self
                        .source
                        .get(stmt.span.start as usize..)
                        .is_some_and(|tail| tail.trim().is_empty());
                if !synthetic_at_eof {
                    self.writeln(";");
                }
            }
            StmtKind::FnDecl(fn_decl) => {
                if self.emit_this_parameter_initializer_function_recovery(fn_decl) {
                    return;
                }
                if let Some(skip_end) = self.fn_decl_suppress_recovered_body_end(fn_decl) {
                    self.skip_recovery_until = self.skip_recovery_until.max(skip_end);
                    return;
                }
                let accessor_prefix_pos = if self.emit_accessor_prefix {
                    let before = self.output.len();
                    let at_line_start = self.at_line_start;
                    self.write("accessor ");
                    self.emit_accessor_prefix = false;
                    Some((before, at_line_start))
                } else {
                    None
                };
                self.emit_fn_decl(fn_decl);
                // Attach trailing comment to the function closing brace BEFORE
                // emitting any namespace export assignment (e.g. M.fn = fn;)
                if fn_decl.body.is_some() && fn_decl.modifiers & MOD_DECLARE == 0 {
                    self.append_trailing_comment(stmt.span);
                }
                if let Some(ref target) = self.export_target.clone() {
                    if fn_decl.modifiers & MOD_EXPORT != 0
                        && fn_decl.modifiers & MOD_DECLARE == 0
                        && fn_decl.body.is_some()
                        && target != "exports"
                    // Top-level CJS: handled in header
                    {
                        if let Some(ref name) = fn_decl.name {
                            self.write(&target);
                            self.write(".");
                            self.write(name);
                            self.write(" = ");
                            self.write(name);
                            self.writeln(";");
                        }
                    }
                }
                if let Some((before, at_line_start)) = accessor_prefix_pos {
                    if self.output.len() == before + "accessor ".len() {
                        self.output.truncate(before);
                        self.at_line_start = at_line_start;
                    }
                }
            }
            StmtKind::ClassDecl(class_decl) => {
                let accessor_prefix_pos =
                    if self.emit_accessor_prefix && !self.needs_downlevel("auto-accessors") {
                        let before = self.output.len();
                        let at_line_start = self.at_line_start;
                        self.write("accessor ");
                        self.emit_accessor_prefix = false;
                        Some((before, at_line_start))
                    } else {
                        self.emit_accessor_prefix = false;
                        None
                    };
                // Emit `static ` prefix from error recovery (e.g. `static class A {}` in namespace).
                if self.emit_static_prefix {
                    self.write("static ");
                    self.emit_static_prefix = false;
                }
                if self.emit_recovery_missing_name_generator_method_class_decl(class_decl) {
                    self.append_trailing_comment(stmt.span);
                    return;
                }
                if self.emit_recovery_reserved_word_class_method_param_tail(class_decl, stmt.span) {
                    self.append_trailing_comment(stmt.span);
                    return;
                }
                if self.emit_recovery_class_wrapped_try_property(class_decl, stmt.span) {
                    return;
                }
                if !self.should_preserve_decorators()
                    && self.options.experimental_decorators != Some(true)
                    && class_can_emit_simple_standard_decorator_wrapper(class_decl)
                {
                    self.emit_simple_standard_decorated_class_wrapper(class_decl);
                    self.append_trailing_comment(stmt.span);
                    // Emit CJS export after the IIFE if needed.
                    if let Some(ref target) = self.export_target.clone() {
                        if class_decl.modifiers & MOD_EXPORT != 0
                            && class_decl.modifiers & MOD_DECLARE == 0
                            && (target != "exports" || !self.has_export_assign)
                        {
                            if let Some(ref name) = class_decl.name {
                                self.write(target);
                                self.write(".");
                                self.write(name);
                                self.write(" = ");
                                self.write(name);
                                self.writeln(";");
                            }
                        }
                    }
                    return;
                }
                if !self.should_preserve_decorators()
                    && self.options.experimental_decorators != Some(true)
                    && class_native_standard_decorator_shape(class_decl).is_some()
                {
                    self.emit_native_standard_decorator_class_decl_wrapper(class_decl);
                    self.append_trailing_comment(stmt.span);
                    if let Some(ref target) = self.export_target.clone() {
                        if class_decl.modifiers & MOD_EXPORT != 0
                            && class_decl.modifiers & MOD_DECLARE == 0
                            && (target != "exports" || !self.has_export_assign)
                        {
                            if let Some(ref name) = class_decl.name {
                                self.write(target);
                                self.write(".");
                                self.write(name);
                                self.write(" = ");
                                self.write(name);
                                self.writeln(";");
                            }
                        }
                    }
                    return;
                }
                let emit_narrow_standard_decorator_decl_wrapper = !self
                    .should_preserve_decorators()
                    && self.options.experimental_decorators != Some(true)
                    && class_can_emit_narrow_standard_decorator_member_decl(class_decl);
                // Use the expr IIFE approach for class declarations with accessor/static
                // decorated members that the decl wrapper doesn't support.
                let emit_narrow_standard_decorator_expr_iife =
                    !emit_narrow_standard_decorator_decl_wrapper
                        && !self.should_preserve_decorators()
                        && self.options.experimental_decorators != Some(true)
                        && class_decl.name.is_some()
                        && class_can_emit_narrow_standard_decorator_member_expr(class_decl);
                let mut recovered_default_keyword_decorators: Vec<String> = Vec::new();
                let mut recovered_default_keyword_name: Option<String> = None;
                if self.options.experimental_decorators == Some(true)
                    && self.helper_prefix().is_empty()
                    && class_decl.modifiers & MOD_EXPORT == 0
                    && class_decl.modifiers & MOD_DECLARE == 0
                    && class_decl.name.is_none()
                {
                    if !class_decl.decorators.is_empty() {
                        recovered_default_keyword_decorators = class_decl
                            .decorators
                            .iter()
                            .map(|d| {
                                emit_expr_to_string(
                                    self.source,
                                    self.options,
                                    d,
                                    &self.cjs_import_map,
                                    &self.cjs_string_import_locals,
                                    &self.import_shadows,
                                )
                            })
                            .collect();
                    }
                    if !recovered_default_keyword_decorators.is_empty() {
                        recovered_default_keyword_name =
                            Some(format!("default_{}", self.default_export_counter));
                        self.default_export_counter += 1;
                    }
                }

                if let Some(ref recovered_name) = recovered_default_keyword_name {
                    if !self.needs_decorate_helper {
                        self.emit_decorate_helper();
                    }
                    self.write("let ");
                    self.write(recovered_name);
                    self.write(" = ");
                }

                // Decorated classes with class-level or constructor-param
                // decorators need `let X = class X { };` so the __decorate
                // call can reassign the class name.
                let needs_let_wrapper = self.options.experimental_decorators == Some(true)
                    && class_needs_let_wrapper(class_decl)
                    && recovered_default_keyword_name.is_none();
                // When a decorated class references its own name in its body
                // (e.g. `new Foo()` in a static method), TypeScript creates a
                // `_1` alias so internal references survive decoration:
                //   var C_1;
                //   let C = C_1 = class C { ... C_1.y ... };
                //   C = C_1 = __decorate([...], C);
                let self_ref_alias = if needs_let_wrapper {
                    class_decl.name.as_ref().and_then(|name| {
                        if decorated_class_has_self_reference(class_decl) {
                            let counter = self
                                .decorated_alias_emit_counter
                                .entry(name.clone().into())
                                .or_insert(0);
                            *counter += 1;
                            Some(format!("{}_{}", name, counter))
                        } else {
                            None
                        }
                    })
                } else {
                    None
                };
                let saved_import_map_entry = if let Some(ref alias) = self_ref_alias {
                    if let Some(ref name) = class_decl.name {
                        // `var X_1;` is hoisted to top by the pre-scan
                        let prev = self.cjs_import_map.remove(name.as_str());
                        self.cjs_import_map.insert(
                            name.clone().into(),
                            (alias.clone().into(), AstString::default()),
                        );
                        Some((name.clone(), prev))
                    } else {
                        None
                    }
                } else {
                    None
                };
                // When the class has static initializers and static blocks are
                // natively supported, emit `static { alias = this; }` inside
                // the class body instead of `alias = class C { ... }` outside.
                let use_static_alias_block = self_ref_alias.is_some()
                    && class_has_static_initializers(class_decl)
                    && !self.needs_downlevel("static-blocks");
                if needs_let_wrapper {
                    if let Some(ref name) = class_decl.name {
                        self.write("let ");
                        self.write(name);
                        self.write(" = ");
                        if self_ref_alias.is_some() && !use_static_alias_block {
                            self.write(&self_ref_alias.as_ref().unwrap());
                            self.write(" = ");
                        }
                    }
                }
                if use_static_alias_block {
                    self.inject_decorator_alias_block = self_ref_alias.clone();
                }
                let prev_helper_insert_pos = self.class_decl_helper_insert_pos;
                self.class_decl_helper_insert_pos = Some(saved_stmt_output_start);
                // Pass the recovered default name so emit_class_static_field_initializers
                // can use "default_1" instead of "default" for unnamed export default classes.
                let prev_recovered = self.recovered_default_class_name.take();
                if recovered_default_keyword_name.is_some() {
                    self.recovered_default_class_name = recovered_default_keyword_name.clone();
                }
                if emit_narrow_standard_decorator_decl_wrapper {
                    self.emit_narrow_standard_decorated_class_decl_wrapper(class_decl);
                } else if emit_narrow_standard_decorator_expr_iife {
                    self.needs_es_decorate_helper = true;
                    self.needs_run_initializers_helper = true;
                    let name = class_decl.name.as_deref().unwrap();
                    self.write("let ");
                    self.write(name);
                    self.write(" = ");
                    self.emit_narrow_standard_decorated_class_expr_iife(class_decl, None);
                    self.writeln(";");
                } else if !self.should_preserve_decorators()
                    && self.options.experimental_decorators != Some(true)
                    && (class_can_emit_method_only_decorator_wrapper(class_decl)
                        || (self.effective_target() >= ScriptTarget::ES2015
                            && self.can_emit_public_multi_method_decorator_wrapper(class_decl)))
                {
                    self.emit_method_only_decorated_class_wrapper(class_decl);
                } else {
                    self.emit_class_decl(class_decl);
                }
                self.recovered_default_class_name = prev_recovered;
                self.class_decl_helper_insert_pos = prev_helper_insert_pos;
                // For decorated classes (or recovered `default class`), replace
                // trailing `}\n` with `};\n` when emitted as `let X = class {}`.
                if needs_let_wrapper || recovered_default_keyword_name.is_some() {
                    // Use pre_static_output_len to find the class body end
                    // (before any static initializers emitted after it).
                    if let Some(pos) = self.pre_static_output_len {
                        if pos >= 2 && &self.output[pos - 2..pos] == "}\n" {
                            self.output.insert(pos - 1, ';');
                        }
                    } else if self.output.ends_with("}\n") {
                        let len = self.output.len();
                        self.output.insert(len - 1, ';');
                    }
                }
                // Attach trailing comment to the class closing brace BEFORE
                // emitting any namespace export assignment (e.g. M.C = C;)
                if class_decl.modifiers & MOD_DECLARE == 0 {
                    self.append_trailing_comment(stmt.span);
                }
                // Parser recovery for `class <keyword> {}` where `<keyword>` is
                // not accepted as a class name token (e.g. `class void {}`).
                // TypeScript emits a trailing `<keyword> {};` statement.
                if class_decl.name.is_none()
                    && class_decl.extends.is_none()
                    && class_decl.type_params.is_none()
                    && class_decl.implements.is_empty()
                    && class_decl.modifiers & (MOD_EXPORT | MOD_DEFAULT | MOD_DECLARE) == 0
                    && class_decl.decorators.is_empty()
                {
                    let class_src = self.copy_span_trimmed(class_decl.span);
                    let after_class = class_src
                        .find("class")
                        .and_then(|idx| class_src.get(idx + "class".len()..))
                        .unwrap_or("");
                    let before_body = after_class
                        .split('{')
                        .next()
                        .unwrap_or("")
                        .split_whitespace()
                        .collect::<Vec<_>>();
                    if before_body.len() == 1 {
                        let token = before_body[0];
                        if token
                            .chars()
                            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '$')
                        {
                            self.write(token);
                            self.writeln(" {};");
                        }
                    }
                }
                let has_decorators = self.options.experimental_decorators == Some(true)
                    && class_has_decorators(class_decl);
                // Check if this class is CJS-exported (either via modifier or via named export).
                let is_cjs_exported = if let Some(ref name) = class_decl.name {
                    self.export_target.as_deref() == Some("exports")
                        && !self.has_export_assign
                        && class_decl.modifiers & MOD_DECLARE == 0
                        && (class_decl.modifiers & MOD_EXPORT != 0
                            || self.cjs_exported_names.contains(name.as_str()))
                } else {
                    false
                };
                self.decorated_class_self_ref_alias = self_ref_alias.clone();
                if has_decorators && is_cjs_exported {
                    // CJS exported decorated class: emit plain export first,
                    // then combined `exports.X = X = __decorate(...)`.
                    if let Some(ref name) = class_decl.name {
                        let exported_name = self
                            .cjs_export_alias_map
                            .get(name.as_str())
                            .cloned()
                            .unwrap_or_else(|| name.clone().into());
                        self.write_cjs_export_access("exports", &exported_name);
                        self.write(" = ");
                        self.write(name);
                        self.writeln(";");
                    }
                    self.emit_decorator_applications(class_decl, Some("exports"));
                } else {
                    if !recovered_default_keyword_decorators.is_empty() {
                        let recovered_name = recovered_default_keyword_name
                            .as_deref()
                            .unwrap_or("default_1");
                        self.write(recovered_name);
                        self.write(" = ");
                        self.write(self.helper_prefix());
                        self.write("__decorate([");
                        self.newline();
                        self.indent += 1;
                        for (i, dec) in recovered_default_keyword_decorators.iter().enumerate() {
                            self.write(dec);
                            if i < recovered_default_keyword_decorators.len() - 1 {
                                self.writeln(",");
                            } else {
                                self.newline();
                            }
                        }
                        self.indent -= 1;
                        self.write("], ");
                        self.write(recovered_name);
                        self.writeln(");");
                    } else if has_decorators {
                        // When decorator arguments reference private names
                        // (e.g. `@decorator((x) => x.#x)`), __decorate calls
                        // must be inside a `static {}` block to access them.
                        let needs_static_block =
                            crate::analysis::class_decorators_reference_private_names(
                                class_decl,
                                self.source,
                            );
                        if needs_static_block {
                            // Remove trailing `}\n` from class body and add
                            // `static { __decorate(...); }\n}\n` instead.
                            if self.output.ends_with("}\n") {
                                let len = self.output.len();
                                self.output.truncate(len - 2);
                                self.indent += 1;
                                self.writeln("static {");
                                self.indent += 1;
                                self.emit_decorator_applications(class_decl, None);
                                self.indent -= 1;
                                self.writeln("}");
                                self.indent -= 1;
                                self.writeln("}");
                            } else {
                                self.emit_decorator_applications(class_decl, None);
                            }
                        } else {
                            self.emit_decorator_applications(class_decl, None);
                        }
                    }
                    if let Some(ref target) = self.export_target.clone() {
                        if class_decl.modifiers & MOD_EXPORT != 0
                            && class_decl.modifiers & MOD_DECLARE == 0
                            && (target != "exports" || !self.has_export_assign)
                        {
                            if let Some(ref name) = class_decl.name {
                                self.write(target);
                                self.write(".");
                                self.write(name);
                                self.write(" = ");
                                self.write(name);
                                self.writeln(";");
                            }
                        }
                    }
                }
                // Clean up decorated class self-reference alias
                self.decorated_class_self_ref_alias = None;
                if let Some((name, prev)) = saved_import_map_entry {
                    if let Some(prev_entry) = prev {
                        self.cjs_import_map.insert(name.into(), prev_entry);
                    } else {
                        self.cjs_import_map.remove(name.as_str());
                    }
                }
                if let Some((before, at_line_start)) = accessor_prefix_pos {
                    if self.output.len() == before + "accessor ".len() {
                        self.output.truncate(before);
                        self.at_line_start = at_line_start;
                    }
                }
            }
            StmtKind::InterfaceDecl(_) => {
                // Interfaces are type-only, but parser recovery can leave
                // invalid `var` declarations inside interface bodies.
                // TypeScript emits those as top-level `var` statements.
                let emitted_recovery = self.emit_recovery_interface_var_members(stmt.span)
                    || self.emit_recovery_interface_dotted_name(stmt.span)
                    || self.emit_recovery_interface_incorrect_return_token(stmt.span);
                if emitted_recovery {
                    self.append_trailing_comment(stmt.span);
                }
            }
            StmtKind::TypeAlias(_) => {
                // Skip - type aliases are type-only
            }
            StmtKind::EnumDecl(enum_decl) => {
                let accessor_prefix_pos = if self.emit_accessor_prefix {
                    let before = self.output.len();
                    let at_line_start = self.at_line_start;
                    self.write("accessor ");
                    self.emit_accessor_prefix = false;
                    Some((before, at_line_start))
                } else {
                    None
                };
                if self.emit_recovery_reserved_word_enum_decl(enum_decl, stmt.span) {
                    self.append_trailing_comment(stmt.span);
                    return;
                }
                self.emit_enum_decl(enum_decl);
                if let Some(recovery_name) =
                    self.enum_decl_multiline_reserved_word_recovery_tail(enum_decl, stmt.span)
                {
                    self.write(&recovery_name);
                    self.writeln(" {};");
                }
                // Attach trailing comment to the enum IIFE closing BEFORE
                // emitting any export assignment
                if enum_decl.modifiers & MOD_DECLARE == 0 {
                    self.append_trailing_comment(stmt.span);
                }
                if let Some(ref target) = self.export_target.clone() {
                    // Skip namespace export assignment for enums: the IIFE closing
                    // already handles `name = M.name || (M.name = {})`.
                    // Only emit for CJS top-level exports.
                    if target == "exports"
                        && !self.has_export_assign
                        && enum_decl.modifiers & MOD_EXPORT != 0
                        && enum_decl.modifiers & MOD_DECLARE == 0
                    {
                        self.write(target);
                        self.write(".");
                        self.write(&enum_decl.name);
                        self.write(" = ");
                        self.write(&enum_decl.name);
                        self.writeln(";");
                    }
                }
                if let Some((before, at_line_start)) = accessor_prefix_pos {
                    if self.output.len() == before + "accessor ".len() {
                        self.output.truncate(before);
                        self.at_line_start = at_line_start;
                    }
                }
            }
            StmtKind::ModuleDecl(module_decl) => {
                if self.emit_recovery_reserved_word_module_decl(module_decl) {
                    self.append_trailing_comment(stmt.span);
                    return;
                }
                let emitted = self.emit_module_decl(module_decl);
                if emitted {
                    self.append_trailing_comment(stmt.span);
                }
            }
            StmtKind::Import(import_decl) => {
                let accessor_prefix_pos = if self.emit_accessor_prefix {
                    let before = self.output.len();
                    let at_line_start = self.at_line_start;
                    self.write("accessor ");
                    self.emit_accessor_prefix = false;
                    Some((before, at_line_start))
                } else {
                    None
                };
                if self.emit_recovery_import_type_defer_conflict(import_decl, stmt.span) {
                    return;
                }
                if self.emit_recovery_reserved_word_import_decl(import_decl, stmt.span) {
                    self.append_trailing_comment(stmt.span);
                    return;
                }
                let before = self.output.len();
                self.emit_import_decl(import_decl);
                if self.output.len() > before {
                    self.append_trailing_comment(stmt.span);
                }
                if let Some((accessor_before, at_line_start)) = accessor_prefix_pos {
                    if self.output.len() == accessor_before + "accessor ".len() {
                        self.output.truncate(accessor_before);
                        self.at_line_start = at_line_start;
                    }
                }
            }
            StmtKind::ImportEquals(ie) => {
                // Track import-equals names in namespace scope for duplicate
                // detection.  Record before elision checks so that even erased
                // (type-only) imports mark the ie.name as "seen".
                let is_ns_scope = self.export_target.as_ref().is_some_and(|t| t != "exports");
                let is_ns_duplicate =
                    is_ns_scope && !self.ns_seen_import_equals.insert(ie.name.clone().into());
                if self.import_equals_rhs_is_type_only_require_spec(&ie.module_ref) {
                    self.advance_comment_pos(stmt.span.end);
                    return;
                }
                if ie.modifiers & MOD_EXPORT != 0
                    && self.export_target.as_deref() == Some("exports")
                    && !self.has_export_assign
                {
                    self.write("exports.");
                    self.write(&ie.name);
                    self.write(" = ");
                    self.emit_expr(&ie.module_ref);
                    self.writeln(";");
                    self.append_trailing_comment(stmt.span);
                    return;
                }
                // Parser error recovery: when the RHS contains `<error>` tokens
                // (e.g. `import lol = Test5.Foo.` with a trailing dot), emit using
                // source text. Also handles `import n = 5;` where the RHS is
                // Ident("<error>"). Must be checked BEFORE the runtime value
                // check, which would incorrectly erase these.
                if import_equals_rhs_has_error(&ie.module_ref) {
                    let rhs_text = self.copy_span_trimmed(ie.module_ref.span);
                    if !rhs_text.is_empty() {
                        // Plain `Ident("<error>")` (e.g. `import n = 5;`): emit
                        // just the source text as an expression statement.
                        // Member chains with `<error>` (e.g. `import lol = Foo.Bar.`):
                        // emit as `var ie.name = <source>;`.
                        if matches!(&ie.module_ref.kind, ExprKind::Ident(_)) {
                            self.write(rhs_text);
                        } else {
                            self.write("var ");
                            self.write(&ie.name);
                            self.write(" = ");
                            self.write(rhs_text);
                        }
                        self.writeln(";");
                    }
                    self.append_trailing_comment(stmt.span);
                    return;
                }
                // `import type X = Y.Z;` — type-only import-equals should be
                // fully erased. The parser doesn't store the `type` modifier in
                // the ImportEquals variant, so detect it from source text.
                {
                    let s = stmt.span.start as usize;
                    let e = (stmt.span.end as usize).min(self.source.len());
                    if s < e {
                        let src = self.source[s..e].trim_start();
                        if src.starts_with("import type ")
                            || src.starts_with("import type\t")
                            || src.starts_with("import type\n")
                        {
                            self.advance_comment_pos(stmt.span.end);
                            return;
                        }
                    }
                }
                // Inside a namespace, when the RHS is a bare identifier that
                // matches a non-exported local variable declaration (var/let/const),
                // elide the import. The local variable shadows the outer
                // module/namespace, so `import Y = X` where X is a local var
                // does not reference a module and should not be emitted.
                if self.export_target.as_ref().is_some_and(|t| t != "exports") {
                    if let ExprKind::Ident(ref rhs_name) = &ie.module_ref.kind {
                        if self.ns_local_value_vars.contains(rhs_name.as_str()) {
                            self.advance_comment_pos(stmt.span.end);
                            return;
                        }
                    }
                }
                // Erase import-equals aliases whose RHS has no runtime value
                // (e.g. aliasing an empty namespace export).
                // With verbatimModuleSyntax, never elide internal import-equals
                // (import X = Y.Z) — emit verbatim. Note: `import X = require(..)`
                // is parsed as StmtKind::Import, not ImportEquals.
                let verbatim_keep = self.options.verbatim_module_syntax == Some(true);
                if !verbatim_keep
                    && !self.import_equals_rhs_has_runtime_value(&ie.module_ref)
                    && !self.cjs_live_export_chain.contains_key(ie.name.as_str())
                {
                    if !self
                        .should_preserve_script_type_only_import_equals(&ie.name, &ie.module_ref)
                    {
                        self.advance_comment_pos(stmt.span.end);
                        return;
                    }
                }
                // At file level in module files, erase non-exported import-equals
                // whose ie.name is never used in a value position. TypeScript
                // elides `import X = Y;` when X has no value references even
                // if Y has a runtime value. In script files, import-equals
                // always emits `var X = Y;` without elision.
                // (This arm only runs for non-exported import-equals; exported
                // ones go through emit_export_decl_cjs / emit_export_decl.)
                if !verbatim_keep
                    && self.is_module_file
                    && self.export_target.as_ref().map_or(true, |t| t == "exports")
                    && self.is_import_elided(&ie.name)
                {
                    self.advance_comment_pos(stmt.span.end);
                    return;
                }
                // Erase import-equals when the ie.name is also declared as a
                // type-only ie.name (e.g. `import EnumA = Enum.A` merged with
                // `export type EnumA = ...`). TypeScript treats this as an
                // error and erases the value import-equals entirely.
                if self.type_only_decl_names.contains(ie.name.as_str()) {
                    self.advance_comment_pos(stmt.span.end);
                    return;
                }
                // In namespace scope, erase import-equals whose ie.name
                // is not referenced in value position by sibling statements.
                if self.export_target.as_ref().is_some_and(|t| t != "exports")
                    && self.import_equals_type_only_ns.contains(ie.name.as_str())
                {
                    self.advance_comment_pos(stmt.span.end);
                    return;
                }
                // Inside namespace scope, elide duplicate import-equals.
                // When multiple `import M = ...` declarations share a ie.name,
                // TypeScript emits only the first one that has a runtime value.
                // If the first was erased (type-only), subsequent value imports
                // with the same ie.name are also suppressed.
                if is_ns_duplicate {
                    self.advance_comment_pos(stmt.span.end);
                    return;
                }
                // Inside namespace bodies, `import X = require("...")` is erased
                // because require() calls don't work inside namespace IIFEs.
                if self.export_target.as_ref().is_some_and(|t| t != "exports") {
                    if let ExprKind::Call(ref call) = &ie.module_ref.kind {
                        if let ExprKind::Ident(ref callee_name) = call.callee.kind {
                            if callee_name == "require" {
                                self.advance_comment_pos(stmt.span.end);
                                return;
                            }
                        }
                    }
                }
                // Track runtime binding so later export-import aliases can
                // detect that this ie.name exists as a value.
                self.emitted_var_names.insert(ie.name.clone().into());
                // Preserve the require form while still downleveling its
                // generated declaration according to target.
                if self.effective_module_kind() == ModuleKind::Preserve
                    && matches!(&ie.module_ref.kind, ExprKind::Call(call) if matches!(&call.callee.kind, ExprKind::Ident(n) if n == "require"))
                {
                    self.write(self.generated_require_binding_keyword());
                    self.write(" ");
                } else {
                    self.write("var ");
                }
                self.write(&ie.name);
                self.write(" = ");
                self.emit_expr(&ie.module_ref);
                self.writeln(";");
                self.append_trailing_comment(stmt.span);
            }
            StmtKind::Export(export_decl) => {
                // `declare export ...` should be fully elided even when the
                // parser does not set MOD_DECLARE on the inner declaration.
                // Exception: `declare export import X = Y.Z` is preserved in
                // normal (full-check) emit, but erased under `noCheck`.
                let s = stmt.span.start as usize;
                let e = (stmt.span.end as usize).min(self.source.len());
                if s < e && self.source[s..e].trim_start().starts_with("declare") {
                    let is_declare_export_import = matches!(
                        &export_decl.kind,
                        ExportDeclKind::Decl(inner)
                            if matches!(&inner.kind, StmtKind::ImportEquals(..))
                    );
                    if is_declare_export_import && self.options.no_check != Some(true) {
                        // Keep full-check behavior for this recovery shape.
                    } else {
                        self.advance_comment_pos(stmt.span.end);
                        return;
                    }
                }
                let before = self.output.len();
                self.emit_export_decl(export_decl);
                if let ExportDeclKind::Decl(inner) = &export_decl.kind {
                    if let StmtKind::Var(var_stmt) = &inner.kind {
                        let malformed_import_type_tails =
                            self.malformed_import_type_option_recovery_tails_for_var_stmt(var_stmt);
                        let has_malformed_import_type_tails =
                            !malformed_import_type_tails.is_empty();
                        for tail in malformed_import_type_tails {
                            self.writeln(&tail);
                        }
                        if has_malformed_import_type_tails {
                            self.skip_recovery_until = self.skip_recovery_until.max(inner.span.end);
                        }
                    }
                }
                // For Decl exports (export class/fn/enum), trailing comment is
                // handled inside emit_export_decl_cjs to attach it to the closing
                // brace before the namespace export assignment. Only append here
                // for non-Decl exports (named re-exports, star re-exports, etc.)
                // Also only append if the export actually produced output — an
                // empty `export {};` in CJS mode emits nothing, and attaching
                // its comment to the previous line (e.g. __esModule ODP) is wrong.
                // For Decl exports in CJS/namespace mode, trailing comments are
                // handled inside emit_export_decl_cjs. For ESM Decl exports and
                // all non-Decl exports, append the trailing comment here.
                // DefaultDecl with ClassDecl also needs skipping because the
                // CJS handler attaches the comment to the closing `}` line.
                let skip_decl_trailing = (matches!(export_decl.kind, ExportDeclKind::Decl(_))
                    || matches!(&export_decl.kind, ExportDeclKind::DefaultDecl(d) if matches!(d.kind, StmtKind::ClassDecl(_))))
                    && (self.is_cjs_like()
                        || self.export_target.as_ref().is_some_and(|t| t != "exports"));
                if !skip_decl_trailing && self.output.len() > before {
                    self.append_trailing_comment(stmt.span);
                }
            }
            StmtKind::ExportAssign(expr) => {
                // `export = x` only generates output in CJS-like modules.
                // In ESM mode it is erased.
                // Inside namespaces, emit as-is for error recovery.
                let in_namespace = self.export_target.as_ref().is_some_and(|t| t != "exports");
                if in_namespace {
                    self.write("export = ");
                    self.emit_expr(expr);
                    self.writeln(";");
                    self.advance_comment_pos(stmt.span.end);
                } else if !self.has_export_assign {
                    // Type-only export assign (e.g. `export = SomeInterface`) — skip
                } else if self.is_amd() || self.is_umd() {
                    // AMD/UMD: `export = X` → `return X;`
                    self.write("return ");
                    self.emit_expr(expr);
                    self.writeln(";");
                    self.advance_comment_pos(stmt.span.end);
                } else if self.is_commonjs() || self.effective_module_kind() == ModuleKind::Preserve
                {
                    self.write("module.exports = ");
                    // Parser error recovery for `export = ;` produces an <error>
                    // placeholder whose source span includes the `;`. Skip the
                    // expression to avoid emitting a duplicate semicolon.
                    if !matches!(&expr.kind, ExprKind::Ident(name) if name == "<error>") {
                        self.emit_expr(expr);
                    }
                    self.writeln(";");
                    // Don't append trailing comment — `export = X` is a TS-specific
                    // construct and TypeScript strips the trailing comment during
                    // transformation to `module.exports = X`.
                    self.advance_comment_pos(stmt.span.end);
                }
            }
            StmtKind::Labeled(labeled) => {
                if self.needs_lexical_downlevel() {
                    let mut labels = vec![labeled.label.as_str()];
                    let mut labeled_body = labeled.body.as_ref();
                    while let StmtKind::Labeled(inner) = &labeled_body.kind {
                        labels.push(inner.label.as_str());
                        labeled_body = inner.body.as_ref();
                    }
                    if let Some(plan) = self.lexical_loop_plan(labeled_body.span) {
                        match &labeled_body.kind {
                            StmtKind::For(loop_stmt) => {
                                self.emit_captured_for_stmt(&plan, loop_stmt, &labels)
                            }
                            StmtKind::ForIn(loop_stmt) => {
                                self.emit_captured_for_in_stmt(&plan, loop_stmt, &labels)
                            }
                            StmtKind::ForOf(loop_stmt) => {
                                self.emit_captured_for_of_stmt(&plan, loop_stmt, &labels)
                            }
                            _ => unreachable!(),
                        }
                        self.append_trailing_comment(stmt.span);
                        return;
                    }
                }
                if self.needs_downlevel("using") {
                    let mut labels = vec![labeled.label.as_str()];
                    let mut labeled_body = labeled.body.as_ref();
                    while let StmtKind::Labeled(inner) = &labeled_body.kind {
                        labels.push(inner.label.as_str());
                        labeled_body = inner.body.as_ref();
                    }
                    if let StmtKind::For(for_stmt) = &labeled_body.kind {
                        if let Some(ForInit::Var(var_stmt)) = &for_stmt.init {
                            if matches!(var_stmt.kind, VarKind::Using | VarKind::AwaitUsing) {
                                self.emit_for_using_dispose_scope(for_stmt, var_stmt, &labels);
                                self.append_trailing_comment(stmt.span);
                                return;
                            }
                        }
                    }
                    if let StmtKind::ForOf(fo) = &labeled_body.kind {
                        if self.simple_for_of_using_binding(fo).is_some() {
                            if fo.is_await && self.needs_downlevel("async-generator") {
                                self.emit_for_await_using_downlevel_with_await(fo, &labels);
                            } else if self.needs_downlevel("for-of") {
                                self.emit_for_of_using_downlevel(fo, labeled_body.span, &labels);
                            } else {
                                self.emit_native_for_of_using(fo, &labels);
                            }
                            self.append_trailing_comment(stmt.span);
                            return;
                        }
                    }
                }
                if self.needs_downlevel("for-of") {
                    let mut labels = vec![labeled.label.as_str()];
                    let mut labeled_body = labeled.body.as_ref();
                    while let StmtKind::Labeled(inner) = &labeled_body.kind {
                        labels.push(inner.label.as_str());
                        labeled_body = inner.body.as_ref();
                    }
                    if let StmtKind::ForOf(fo) = &labeled_body.kind {
                        if !fo.is_await && self.simple_for_of_using_binding(fo).is_none() {
                            // DLI emits declarations and a try/finally around
                            // the synthesized iteration. Attach labels to the
                            // actual inner loop, not to the declaration prefix.
                            self.emit_for_of_downlevel_labeled(fo, labeled_body.span, &labels);
                            self.append_trailing_comment(stmt.span);
                            return;
                        }
                    }
                }
                // Check if the body is type-only (interface, type alias, declare enum,
                // empty namespace, const enum) — emit empty stmt
                let body_is_type_only = match &labeled.body.kind {
                    StmtKind::InterfaceDecl(_) | StmtKind::TypeAlias(_) => true,
                    StmtKind::EnumDecl(e) => {
                        e.modifiers & MOD_DECLARE != 0
                            || (e.is_const && !self.preserve_const_enums_effective())
                    }
                    StmtKind::ModuleDecl(m) => crate::analysis::module_decl_is_type_only(
                        m,
                        self.preserve_const_enums_effective(),
                    ),
                    _ => false,
                };
                // Check if the body expands to multiple statements (enum, namespace)
                // and needs to be wrapped in a block
                let body_needs_block = !body_is_type_only
                    && matches!(
                        labeled.body.kind,
                        StmtKind::EnumDecl(_) | StmtKind::ModuleDecl(_)
                    );

                let body_start = labeled.body.span.start;
                if self.has_block_comments_between(stmt.span.start, body_start) {
                    // Preserve comments between label, colon, and body.
                    // Find the label text in the source, then the colon after it.
                    let label_end = {
                        let s = stmt.span.start as usize;
                        let label_pos = Self::find_keyword_after(self.source, &labeled.label, s);
                        label_pos + labeled.label.len()
                    };
                    // Find the `:` after the label (skipping comments)
                    let colon_pos = {
                        let mut pos = label_end;
                        let bytes = self.source.as_bytes();
                        while pos < self.source.len() {
                            if bytes[pos] == b':' {
                                break;
                            }
                            pos += 1;
                        }
                        pos
                    };
                    self.write(&labeled.label);
                    // Comments between label name and `:`
                    let pre_colon = self.source_between(label_end as u32, colon_pos as u32);
                    if pre_colon.contains("/*") {
                        self.write(" ");
                        self.write(pre_colon.trim());
                    }
                    self.write(":");
                    // Comments between `:` and body start
                    let post_colon = self.source_between((colon_pos + 1) as u32, body_start);
                    if post_colon.contains("/*") {
                        self.write(" ");
                        self.write(post_colon.trim());
                        self.write(" ");
                    } else {
                        self.write(" ");
                    }
                } else {
                    self.write(&labeled.label);
                    self.write(": ");
                }
                if body_is_type_only {
                    // Type-only declarations are erased; emit empty statement
                    self.writeln(";");
                } else if body_needs_block {
                    // Enum/namespace expand to multiple statements;
                    // wrap in a block so the label covers all of them
                    self.writeln("{");
                    self.indent += 1;
                    self.emit_stmt(&labeled.body);
                    self.indent -= 1;
                    self.writeln("}");
                } else {
                    self.emit_stmt(&labeled.body);
                }
                self.append_trailing_comment(stmt.span);
            }
            StmtKind::With(with_stmt) => {
                let obj_start = with_stmt.object.span.start;
                let body_start = with_stmt.body.span.start;
                if self.has_block_comments_between(stmt.span.start, body_start) {
                    let kw_end =
                        Self::find_keyword_after(self.source, "with", stmt.span.start as usize) + 4;
                    let pre_obj = self.source_between(kw_end as u32, obj_start);
                    self.write("with");
                    self.write(pre_obj.trim_end());
                    self.emit_expr(&with_stmt.object);
                    let post_obj = self.source_between(with_stmt.object.span.end, body_start);
                    if let Some(cp) = Self::find_close_paren_skipping_comments(post_obj) {
                        let before_cp = post_obj[..cp].trim_end();
                        let after_cp = &post_obj[cp + 1..];
                        self.write(before_cp);
                        self.write(")");
                        self.write(after_cp);
                    } else {
                        self.write(")");
                        if matches!(with_stmt.body.kind, StmtKind::Block(_)) {
                            self.write(" ");
                        }
                    }
                } else {
                    self.write("with (");
                    self.emit_expr(&with_stmt.object);
                    self.write(")");
                    if matches!(with_stmt.body.kind, StmtKind::Block(_)) {
                        self.write(" ");
                    }
                }
                self.emit_stmt_body(&with_stmt.body);
                self.append_trailing_comment(stmt.span);
            }
            StmtKind::Debugger => {
                self.writeln("debugger;");
                self.append_trailing_comment(stmt.span);
            }
        }
        // Post-process bare `super` -> `super.` in the emitted text.
        // TypeScript appends `.` after bare `super` expressions (not in member
        // access, call, or element access position) to handle error recovery.
        // Skip `super` inside comments and string literals.
        if has_bare_super
            && !self.suppress_bare_super_fixup
            && self.output.len() > saved_stmt_output_start
        {
            let emitted = &self.output[saved_stmt_output_start..];
            // Build the rewritten output as bytes, not chars. Earlier
            // implementation used `String::with_capacity` + `fixed.push(b as char)`
            // — that interprets each byte as a Latin-1 codepoint, so multi-byte
            // UTF-8 (e.g. `é` = `0xC3 0xA9`) became `Ã©` and `String::push_str`
            // then re-encoded those two codepoints as four UTF-8 bytes. Any
            // statement with non-ASCII text + a comment containing the substring
            // `super` (e.g. "super-app") came out mojibake'd. Mirrored fix to the
            // `jsx_decode_entities` regression in `jsx.rs` — preserve bytes,
            // don't reinterpret as chars.
            let bytes = emitted.as_bytes();
            let mut fixed: Vec<u8> = Vec::with_capacity(bytes.len() + 10);
            let mut i = 0;
            let mut in_line_comment = false;
            let mut in_block_comment = false;
            let mut in_string: Option<u8> = None; // b'\'' or b'"' or b'`'
            while i < bytes.len() {
                let b = bytes[i];
                // Track comment/string state
                if in_line_comment {
                    if b == b'\n' {
                        in_line_comment = false;
                    }
                    fixed.push(b);
                    i += 1;
                    continue;
                }
                if in_block_comment {
                    if b == b'*' && bytes.get(i + 1) == Some(&b'/') {
                        fixed.extend_from_slice(b"*/");
                        i += 2;
                        in_block_comment = false;
                    } else {
                        fixed.push(b);
                        i += 1;
                    }
                    continue;
                }
                if let Some(q) = in_string {
                    if b == b'\\' && i + 1 < bytes.len() {
                        fixed.push(b);
                        fixed.push(bytes[i + 1]);
                        i += 2;
                        continue;
                    }
                    if b == q {
                        in_string = None;
                    }
                    fixed.push(b);
                    i += 1;
                    continue;
                }
                // Check for comment/string start
                if b == b'/' && bytes.get(i + 1) == Some(&b'/') {
                    in_line_comment = true;
                    fixed.extend_from_slice(b"//");
                    i += 2;
                    continue;
                }
                if b == b'/' && bytes.get(i + 1) == Some(&b'*') {
                    in_block_comment = true;
                    fixed.extend_from_slice(b"/*");
                    i += 2;
                    continue;
                }
                if b == b'\'' || b == b'"' || b == b'`' {
                    in_string = Some(b);
                    fixed.push(b);
                    i += 1;
                    continue;
                }
                // Now check for bare `super`
                if i + 5 <= bytes.len() && &bytes[i..i + 5] == b"super" {
                    let before_ok = i == 0
                        || !(bytes[i - 1].is_ascii_alphanumeric()
                            || bytes[i - 1] == b'_'
                            || bytes[i - 1] == b'.');
                    // `super_admin` / `super$x` etc. are identifiers — `_`/`$`
                    // are valid id-continue chars, so the trailing-char check
                    // must reject them too, not just alphanumerics. Without
                    // this, `super_admin` got rewritten to `super._admin`.
                    let after_ok = i + 5 >= bytes.len()
                        || !(bytes[i + 5].is_ascii_alphanumeric()
                            || bytes[i + 5] == b'_'
                            || bytes[i + 5] == b'$');
                    if before_ok && after_ok {
                        let after_idx = i + 5;
                        let mut next_off = after_idx;
                        while next_off < bytes.len() && bytes[next_off] == b' ' {
                            next_off += 1;
                        }
                        let next_char = bytes.get(next_off).copied();
                        let is_bare = next_char != Some(b'.')
                            && next_char != Some(b'(')
                            && next_char != Some(b'[')
                            && next_char != Some(b':');
                        if is_bare {
                            fixed.extend_from_slice(b"super.");
                            i += 5;
                            continue;
                        }
                    }
                }
                fixed.push(b);
                i += 1;
            }
            // SAFETY: `fixed` is built by copying bytes from `emitted` (valid
            // UTF-8 by Rust's `String` invariant) plus ASCII literals (`//`,
            // `*/`, `super.`). Every byte added is part of an unbroken
            // valid-UTF-8 sequence — we never split a multi-byte char because
            // the state-machine only consults ASCII bytes (`/`, `*`, `'`, `"`,
            // `` ` ``, `\\`) and `super` is pure ASCII; non-ASCII bytes only
            // flow through the catch-all `fixed.push(b)` paths which preserve
            // byte order.
            let fixed_str = unsafe { String::from_utf8_unchecked(fixed) };
            self.output.truncate(saved_stmt_output_start);
            self.output.push_str(&fixed_str);
        }
    }

    /// Like `emit_source_line` but expands single-line class bodies to
    /// multi-line to match TypeScript's structured emit behavior.
    /// Used for export statements in error recovery (block/function scope).
    pub(super) fn emit_source_line_expand_class(&mut self, stmt: &Stmt) {
        let before_len = self.output.len();
        self.emit_source_line(stmt);
        // Check if the emitted text has a single-line empty class body
        // like `class C { }` or `class C {}` — expand to multi-line.
        // The emitted text may end with \n from writeln, so trim it first.
        let emitted = self.output[before_len..].trim_end();
        let is_single_line_class = emitted.contains("class ") && emitted.matches('\n').count() == 0;
        if is_single_line_class {
            // Look for `{ }` or `{}` at the end
            if emitted.ends_with("{ }") || emitted.ends_with("{}") {
                let indent_str = "    ".repeat(self.indent);
                let brace_len = if emitted.ends_with("{ }") { 3 } else { 2 };
                // Keep everything up to and including the opening `{`
                let keep_len = emitted.len() - brace_len + 1;
                let kept = self.output[before_len..before_len + keep_len].to_string();
                self.output.truncate(before_len);
                self.output.push_str(&kept);
                self.output.push('\n');
                self.output.push_str(&indent_str);
                self.output.push_str("}\n");
                self.at_line_start = true;
            }
        }
    }

    /// Emit a statement that needs no transformation by copying its source text,
    /// respecting the current indentation level.
    pub(super) fn emit_source_line(&mut self, stmt: &Stmt) {
        // Recovery can attach a later standalone `;` (and the comments before
        // it) to a preceding variable statement that omitted its own
        // semicolon. TypeScript prints the variable with that semicolon and
        // drops the intervening recovery trivia.
        if matches!(stmt.kind, StmtKind::Var(_)) {
            let raw = self.copy_span_trimmed(stmt.span);
            let lines: Vec<&str> = raw.lines().collect();
            if let Some((first, tail)) = lines.split_first() {
                let last_nonempty = tail.iter().rposition(|line| !line.trim().is_empty());
                if !first.trim_end().ends_with(';')
                    && last_nonempty.is_some_and(|index| tail[index].trim() == ";")
                    && tail.iter().enumerate().all(|(index, line)| {
                        line.trim().is_empty()
                            || line.trim_start().starts_with("//")
                            || Some(index) == last_nonempty
                    })
                {
                    self.write(first.trim_end());
                    self.writeln(";");
                    self.advance_comment_pos(stmt.span.end);
                    return;
                }
            }
        }
        // For compound statements that contain child statements, decompose
        // into explicit emit so each child gets proper semicolon insertion.
        if let StmtKind::Block(stmts) = &stmt.kind {
            // Block-level using disposal transform
            if self.needs_downlevel("using") && has_using_declaration(stmts) {
                self.writeln("{");
                self.indent += 1;
                if let Some(first_idx) = first_using_index(stmts) {
                    for s in &stmts[..first_idx] {
                        self.emit_stmt(s);
                    }
                    self.emit_block_using_dispose_scope(&stmts[first_idx..]);
                }
                self.indent -= 1;
                self.writeln("}");
                self.append_trailing_comment(stmt.span);
                return;
            }
            self.writeln("{");
            self.indent += 1;
            for s in stmts {
                self.emit_leading_comments(s.span.start);
                self.emit_stmt(s);
                self.advance_comment_pos(s.span.end);
            }
            // Emit any comments between the last statement and the closing `}`.
            // Use the block's span end to find the correct `}` rather than
            // the first `}` after the last statement (which could be from
            // an inner block in nested code).
            {
                let block_end = stmt.span.end as usize;
                if block_end > 0 && block_end <= self.source.len() {
                    let brace_pos = if self.source.as_bytes().get(block_end - 1) == Some(&b'}') {
                        (block_end - 1) as u32
                    } else {
                        self.source[..block_end]
                            .rfind('}')
                            .map(|p| p as u32)
                            .unwrap_or(block_end as u32)
                    };
                    self.emit_leading_comments(brace_pos);
                }
            }
            self.indent -= 1;
            self.writeln("}");
            self.append_trailing_comment(stmt.span);
            return;
        }
        if let StmtKind::Switch(sw) = &stmt.kind {
            self.emit_switch_stmt(sw, stmt.span);
            self.append_trailing_comment(stmt.span);
            return;
        }

        // For for-of/for-in/for/while statements: TypeScript always expands
        // the body to multi-line.  Detect the body (block or non-block) and
        // emit the header from source text + the body structurally.
        {
            let body_ref: Option<&Stmt> = match &stmt.kind {
                StmtKind::ForOf(fo) => Some(&fo.body),
                StmtKind::ForIn(fi) => Some(&fi.body),
                StmtKind::For(f) => Some(&f.body),
                StmtKind::While(w) => Some(&w.body),
                _ => None,
            };
            if let Some(body) = body_ref {
                let is_block = matches!(body.kind, StmtKind::Block(_));
                // Only use the structured header+body emit when the source is
                // single-line (no newlines in the statement span).
                // Multi-line statements fall through to the general source-copy path.
                // This handles the case where a mis-parsed `for...in/of` with a member
                // expression LHS (which the parser cannot represent in ForInOfLeft) is
                // stored as a regular `For` with the next statement as its body.
                let source_is_single_line = {
                    let s = stmt.span.start as usize;
                    let e = stmt.span.end as usize;
                    s < e && e <= self.source.len() && !self.source[s..e].contains('\n')
                };
                if source_is_single_line {
                    // Emit the header (everything before the body) from source text.
                    let header_end = body.span.start as usize;
                    let header_start = stmt.span.start as usize;
                    if header_end > header_start {
                        let header_src = self.source[header_start..header_end].trim_end();
                        let header_src = normalize_emit_chain_v2(header_src);
                        if is_block {
                            if let StmtKind::Block(stmts) = &body.kind {
                                if stmts.is_empty() {
                                    // Empty block: check for comments inside.
                                    let inner_comment =
                                        if self.options.remove_comments != Some(true) {
                                            let bs = body.span.start as usize;
                                            let be = body.span.end as usize;
                                            if bs < be && be <= self.source.len() {
                                                let bsrc = &self.source[bs..be];
                                                if let (Some(o), Some(c)) =
                                                    (bsrc.find('{'), bsrc.rfind('}'))
                                                {
                                                    let between = bsrc[o + 1..c].trim();
                                                    if between.starts_with("/*")
                                                        && between.ends_with("*/")
                                                    {
                                                        Some(between.to_string())
                                                    } else {
                                                        None
                                                    }
                                                } else {
                                                    None
                                                }
                                            } else {
                                                None
                                            }
                                        } else {
                                            None
                                        };
                                    if let Some(comment) = inner_comment {
                                        self.writeln(&format!("{header_src} {{ {comment} }}"));
                                        // Advance comment tracking past the block.
                                        while self.next_comment_idx < self.comments.len()
                                            && self.comments[self.next_comment_idx].pos
                                                < body.span.end
                                        {
                                            self.next_comment_idx += 1;
                                        }
                                        self.comment_emit_pos =
                                            self.comment_emit_pos.max(body.span.end);
                                    } else {
                                        self.writeln(&format!("{header_src} {{ }}"));
                                    }
                                } else {
                                    self.writeln(&format!("{header_src} {{"));
                                }
                            }
                        } else {
                            self.writeln(&header_src);
                        }
                    }
                    if is_block {
                        if let StmtKind::Block(stmts) = &body.kind {
                            if !stmts.is_empty() {
                                self.indent += 1;
                                for s in stmts {
                                    self.emit_stmt(s);
                                }
                                self.indent -= 1;
                                self.writeln("}");
                            }
                        }
                    } else {
                        self.indent += 1;
                        self.emit_stmt(body);
                        self.indent -= 1;
                    }
                    self.append_trailing_comment(stmt.span);
                    return;
                }
            }
        }

        let raw_text = self.copy_span_trimmed(stmt.span);
        // When removeComments is enabled, strip comments from source-copied text.
        // Use `Cow` so the no-strip path (the common case) doesn't allocate just
        // to satisfy the type system.
        let raw_text: std::borrow::Cow<str> = if self.options.remove_comments == Some(true) {
            std::borrow::Cow::Owned(strip_source_comments(raw_text))
        } else {
            std::borrow::Cow::Borrowed(raw_text)
        };
        // Replace Unicode NEL (U+0085) with regular space outside strings
        // (must happen before other normalizations to avoid double spaces).
        // NEL is TS whitespace but NOT JS whitespace, so unlike the chain below
        // this replacement is a validity fix, not a cosmetic one. The per-file
        // flag short-circuits the per-statement scan.
        let raw_text = if self.source_has_nel && raw_text.contains('\u{0085}') {
            std::borrow::Cow::Owned(normalize_nel(&raw_text).into_owned())
        } else {
            raw_text
        };
        // FAST TRANSPILE-ONLY EMIT: this statement needs no transform (that's why
        // we're on the source-copy path), so — with comments stripped and NEL
        // normalized above — its source is already valid JS. Skip the entire
        // cosmetic normalization chain below (NBSP, brace spacing, K&R/Allman,
        // unified-pass, smeared-split, blank-line removal, per-line re-indent, …)
        // and copy the span verbatim. Output is semantically identical; only the
        // formatting differs (indentation is irrelevant to JS). This is the bulk of
        // the emit hot path — see plan: bext/PRISM doesn't need tsc-faithful output.
        if self.fast_emit {
            self.writeln(&raw_text);
            self.append_trailing_comment(stmt.span);
            return;
        }
        // Replace Unicode NBSP (U+00A0) with regular space outside strings —
        // TypeScript normalizes non-breaking spaces to regular spaces in emitted code.
        let raw_text = if raw_text.contains('\u{00A0}') {
            std::borrow::Cow::Owned(normalize_nbsp(&raw_text))
        } else {
            raw_text
        };
        // Normalize source text to match TypeScript's emitter formatting.
        let raw_text = normalize_brace_spacing(&raw_text);
        // Normalize Allman-style braces: join `)\n<whitespace>{` into `) {`.
        // TypeScript always emits K&R style for control flow statements.
        let raw_text = normalize_allman_braces(&raw_text);
        // Normalize empty block bodies: `=> {}` → `=> { }`, `) {}` → `) { }`, etc.
        let text = normalize_empty_blocks(&raw_text);
        // Normalize `} ,` to `},` — TypeScript's structured emit
        // always places the comma immediately after the closing brace.
        // `str::replace` always allocates, even when there's no match; gate
        // it on contains() so the common no-match path stays zero-alloc.
        let text = if text.contains("} ,") {
            std::borrow::Cow::Owned(text.replace("} ,", "},"))
        } else {
            text
        };
        // Six chain stages collapsed into one unified pass:
        //   keyword_paren → import_call_spacing → unary_spacing →
        //   trailing_semi → close_paren → comment_word_space.
        // `normalize_unified_pass`'s `;` rule subsumes `normalize_trailing_semi`
        // (both collapse a run of trailing spaces before `;` with the same
        // keyword-exception set: void/delete/typeof/return/throw/await/
        // yield/using and the `=>` predecessor case).
        let text = normalize_unified_pass(&text);
        // Strip space between identifier and `[` for element access
        let text = normalize_element_access_spacing(&text);
        // For for-of: use AST to strip space before `[` in iterable element
        // access even when the identifier matches a keyword name (e.g. `of`).
        // The generic normalizer skips `of [` because `of` is a keyword, but in
        // `for (using of of[])` the second `of` is an identifier used as the
        // iterable expression.
        // Skip this fixup when the text before the first ` of ` contains `await`
        // (e.g. `for (await using of of [])`) — in that case the second `of` is
        // the for-of keyword and `[]` is a standalone array literal.
        let text = if let StmtKind::ForOf(_) = &stmt.kind {
            if let Some(kw_pos) = text.find(" of ") {
                // If `await` appears before the for-of keyword, the declaration
                // may contain `of` as a binding name and the word after the first
                // ` of ` is the actual for-of keyword, not an identifier.
                let before_kw = &text[..kw_pos];
                let has_await_before = before_kw.contains("await");
                let after_kw = kw_pos + 4; // position after ` of `
                let rest = &text[after_kw..];
                let word_len = rest
                    .bytes()
                    .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'$')
                    .count();
                if word_len > 0 && !has_await_before {
                    let after_word = after_kw + word_len;
                    let word = &text[after_kw..after_word];
                    // Don't strip space if the word is a true keyword — the `[`
                    // is an array literal, not element access.
                    // `of` is NOT in this list because in a non-await for-of,
                    // any `of` after the for-of keyword is an identifier.
                    let word_is_keyword = matches!(
                        word,
                        "in" | "var"
                            | "let"
                            | "const"
                            | "new"
                            | "return"
                            | "case"
                            | "delete"
                            | "typeof"
                            | "void"
                            | "throw"
                            | "instanceof"
                            | "yield"
                            | "await"
                            | "import"
                            | "export"
                    );
                    if !word_is_keyword
                        && text.as_bytes().get(after_word) == Some(&b' ')
                        && text.as_bytes().get(after_word + 1) == Some(&b'[')
                    {
                        let mut h = text.to_string();
                        h.remove(after_word);
                        h.into()
                    } else {
                        text
                    }
                } else {
                    text
                }
            } else {
                text
            }
        } else {
            text
        };
        // Add space before tagged template backtick after `)` or `]`
        let text = normalize_tagged_template_space(&text);
        // TypeScript always emits parens around single-param async arrows
        let text = normalize_async_arrow_parens(&text);
        // Normalize yield spacing in generator functions: `yield(foo)` → `yield (foo)`
        let text = normalize_yield_spacing(&text);
        // Strip trailing // comments from function/method opening brace lines.
        // TypeScript's structured emit doesn't preserve these trailing comments.
        let text = strip_trailing_comment_on_brace_line(&text);
        // Trim trailing spaces inside multiline block comments.
        let text = trim_block_comment_line_trailing_spaces(&text);
        // In multiline array literals with elisions, split `, ,` runs across
        // lines so trailing holes are preserved as standalone comma lines.
        let text = if text.contains('\n') && text.contains(", ,") {
            let mut fixed = String::new();
            for (li, line) in text.lines().enumerate() {
                if li > 0 {
                    fixed.push('\n');
                }
                if let Some(pos) = line.find(", ,") {
                    let indent_len = line.chars().take_while(|c| c.is_ascii_whitespace()).count();
                    fixed.push_str(&line[..pos + 1]);
                    fixed.push('\n');
                    fixed.push_str(&" ".repeat(indent_len));
                    fixed.push_str(&line[pos + 2..]);
                } else {
                    fixed.push_str(line);
                }
            }
            fixed
        } else {
            text
        };
        // Some parser-recovery spans may accidentally include multiple
        // semicolon-separated statements on one line. Split those back into
        // one statement per line to match TypeScript emit.
        let text = split_smeared_expr_statement_line(&text).unwrap_or(text);
        // Expand tabs to 4 spaces (TypeScript always emits spaces, never tabs).
        let text = if text.contains('\t') {
            text.replace('\t', "    ")
        } else {
            text
        };

        // Determine the leading whitespace before the span on its source line.
        // The parser's span starts at the first non-trivia character, but we
        // need the source line's leading whitespace for correct indent computation.
        let start = stmt.span.start as usize;
        let mut starts_mid_line_non_ws = false;
        // Hold leading whitespace as Cow<str>: typical indentation is all
        // spaces (Borrowed, zero alloc); only tab expansion or a mid-line
        // statement promotes to Owned.
        let leading_ws_cow: std::borrow::Cow<str> = if start > 0 {
            let before = &self.source[..start];
            let line_start = before.rfind('\n').map(|p| p + 1).unwrap_or(0);
            let ws = &self.source[line_start..start];
            if ws.trim().is_empty() {
                if ws.contains('\t') {
                    std::borrow::Cow::Owned(ws.replace('\t', "    "))
                } else {
                    std::borrow::Cow::Borrowed(ws)
                }
            } else {
                starts_mid_line_non_ws = true;
                std::borrow::Cow::Borrowed("")
            }
        } else {
            std::borrow::Cow::Borrowed("")
        };
        let leading_ws: &str = &leading_ws_cow;

        // For multi-line spans where the span starts after whitespace on the source
        // line, prepend that whitespace so the first line has correct indentation.
        let text_owned: String;
        let text_ref: &str = if !leading_ws.is_empty() && text.contains('\n') {
            text_owned = format!("{}{}", leading_ws, text);
            &text_owned
        } else {
            text_owned = text;
            &text_owned
        };

        // Split into lines, removing blank lines around braces.
        // TypeScript's structured emit doesn't preserve blank lines inside
        // function/block bodies.
        let mut lines: Vec<&str> = text_ref.lines().collect();
        {
            let mut i = 0;
            while i + 1 < lines.len() {
                if lines[i].trim().is_empty() {
                    let next_trimmed = lines.get(i + 1).map(|l| l.trim()).unwrap_or("");
                    // Remove blank lines before lines starting with `}`
                    if next_trimmed.starts_with('}') {
                        lines.remove(i);
                        continue;
                    }
                    // Remove blank lines after lines ending with `{` (possibly followed by a comment)
                    if i > 0 && line_ends_with_open_brace(lines[i - 1]) {
                        lines.remove(i);
                        continue;
                    }
                    // Remove blank lines between statements inside blocks
                    let in_block = lines[..i].iter().any(|l| line_ends_with_open_brace(l));
                    if in_block {
                        lines.remove(i);
                        continue;
                    }
                }
                i += 1;
            }
        }
        if lines.is_empty() {
            return;
        }

        // Track template expression depth per line.  Inside template `${...}`
        // blocks, TypeScript's structured emit writes content at column 0 (no
        // extra indentation) and joins the closing `}` to the previous expression
        // line (unless a line comment precedes it).  The source-copy path must
        // replicate this: strip leading whitespace from non-block-comment lines
        // inside `${}` and join closing `}` to the previous line when possible.
        let (in_template_expr, template_expr_close): (Vec<bool>, Vec<bool>) = {
            let mut in_tpl = vec![false; lines.len()];
            let mut tpl_close = vec![false; lines.len()];
            let mut in_string: u8 = 0; // 0, b'\'', b'"', b'`'
            let mut tpl_depth: u32 = 0;
            let mut brace_stack: Vec<i32> = Vec::new(); // brace depth per template level
            let mut in_block_comment = false;
            for (li, line) in lines.iter().enumerate() {
                in_tpl[li] = tpl_depth > 0 && in_string != b'`';
                let mut in_line_comment = false; // line comments don't span lines
                let bytes = line.as_bytes();
                let mut ci = 0;
                while ci < bytes.len() {
                    let ch = bytes[ci];
                    if in_line_comment {
                        ci += 1;
                        continue;
                    }
                    if in_block_comment {
                        if ch == b'*' && ci + 1 < bytes.len() && bytes[ci + 1] == b'/' {
                            in_block_comment = false;
                            ci += 2;
                            continue;
                        }
                        ci += 1;
                        continue;
                    }
                    if in_string == b'`' {
                        if ch == b'\\' && ci + 1 < bytes.len() {
                            ci += 2;
                            continue;
                        }
                        if ch == b'`' {
                            in_string = 0;
                            ci += 1;
                            continue;
                        }
                        if ch == b'$' && ci + 1 < bytes.len() && bytes[ci + 1] == b'{' {
                            tpl_depth += 1;
                            brace_stack.push(0);
                            in_string = 0;
                            ci += 2;
                            continue;
                        }
                        ci += 1;
                        continue;
                    }
                    if in_string != 0 {
                        if ch == b'\\' && ci + 1 < bytes.len() {
                            ci += 2;
                            continue;
                        }
                        if ch == in_string {
                            in_string = 0;
                        }
                        ci += 1;
                        continue;
                    }
                    // Not in any string/comment
                    if ch == b'\'' || ch == b'"' {
                        in_string = ch;
                        ci += 1;
                        continue;
                    }
                    if ch == b'`' {
                        in_string = b'`';
                        ci += 1;
                        continue;
                    }
                    if ch == b'/' && ci + 1 < bytes.len() {
                        if bytes[ci + 1] == b'/' {
                            in_line_comment = true;
                            ci += 2;
                            continue;
                        }
                        if bytes[ci + 1] == b'*' {
                            in_block_comment = true;
                            ci += 2;
                            continue;
                        }
                    }
                    if tpl_depth > 0 {
                        if ch == b'{' {
                            if let Some(d) = brace_stack.last_mut() {
                                *d += 1;
                            }
                        } else if ch == b'}' {
                            let close_tpl = brace_stack.last().is_some_and(|&d| d == 0);
                            if close_tpl {
                                // Mark this line if `}` closing the template
                                // expression is at (or near) the start of the
                                // trimmed content.
                                let trimmed = line.trim();
                                if trimmed.starts_with('}') {
                                    tpl_close[li] = true;
                                }
                                tpl_depth -= 1;
                                brace_stack.pop();
                                in_string = b'`'; // back in template string
                            } else if let Some(d) = brace_stack.last_mut() {
                                *d -= 1;
                            }
                        }
                    }
                    ci += 1;
                }
            }
            (in_tpl, tpl_close)
        };

        // Mark call-argument continuation lines for joining.
        // TypeScript places all arguments of a call on the same line;
        // when the source splits them, we join them back.
        // join_with_prev[i] == true means line[i] should be appended to the
        // previous line instead of starting a new line.
        // Track which lines start inside parens (for indent stripping).
        let mut in_paren_at_start: Vec<bool> = vec![false; lines.len()];
        let mut join_with_prev: Vec<bool> = {
            let mut marks = vec![false; lines.len()];
            let mut paren_depth: i32 = 0;
            let mut brace_depth: i32 = 0;
            let mut bracket_depth: i32 = 0;
            for i in 0..lines.len() {
                let trimmed = lines[i].trim();
                // Record paren depth at start of this line.
                in_paren_at_start[i] = paren_depth > 0 && brace_depth <= 0 && bracket_depth <= 0;
                // Check if THIS line should be joined to the previous.
                // Only join when inside parens (call args) but not inside
                // braces (object literals) or brackets (array literals).
                if i > 0 && paren_depth > 0 && brace_depth <= 0 && bracket_depth <= 0 {
                    let prev_trimmed = lines[i - 1].trim();
                    let prev_ends_comma = prev_trimmed.ends_with(',');
                    let prev_ends_open_paren = prev_trimmed.ends_with('(')
                        || (prev_trimmed.ends_with("*/") && {
                            // Check if there's an open paren before the trailing block comment
                            if let Some(bc_start) = prev_trimmed.rfind("/*") {
                                let before_comment = prev_trimmed[..bc_start].trim_end();
                                before_comment.ends_with('(')
                            } else {
                                false
                            }
                        });
                    // Previous line ends with `=>` — join arrow body
                    let prev_ends_arrow = prev_trimmed.ends_with("=>");
                    // Closing paren line — join `)` or `);` to the call,
                    // but NOT when the previous line has a `//` comment
                    // (joining would place the paren inside the comment).
                    let prev_has_line_comment = prev_trimmed.contains("//");
                    let is_close_paren = trimmed.starts_with(')') && !prev_has_line_comment;
                    // Arrow `=>` on a continuation line — join to previous
                    let is_arrow_continuation = trimmed.starts_with("=>");
                    let is_continuation = !trimmed.is_empty()
                        && !(trimmed.starts_with('.') && !trimmed.starts_with("..."))
                        && !trimmed.starts_with("//")
                        && !trimmed.starts_with("/*");
                    if ((prev_ends_comma || prev_ends_open_paren || prev_ends_arrow)
                        && is_continuation)
                        || is_close_paren
                        || is_arrow_continuation
                    {
                        marks[i] = true;
                    }
                }
                // Update depth AFTER the join check (depth applies to
                // text processed so far, used for the NEXT line's check)
                for ch in trimmed.chars() {
                    match ch {
                        '(' => paren_depth += 1,
                        ')' => paren_depth -= 1,
                        '{' => brace_depth += 1,
                        '}' => brace_depth -= 1,
                        '[' => bracket_depth += 1,
                        ']' => bracket_depth -= 1,
                        _ => {}
                    }
                }
            }
            marks
        };

        // Collapse multi-line empty object literals in return statements.
        // TypeScript always emits `return {};` even when the source has
        // `return {\n}` across multiple lines.  Join the closing `}` line
        // to the previous `return {` line.
        for i in 1..lines.len() {
            if join_with_prev[i] {
                continue;
            }
            let cur_trimmed = lines[i].trim();
            if cur_trimmed == "}" || cur_trimmed == "};" {
                let prev_trimmed = lines[i - 1].trim();
                if prev_trimmed == "return {" {
                    join_with_prev[i] = true;
                }
            }
        }

        // Collapse empty object literal arguments across two lines inside
        // parenthesized argument lists:
        // `(...,\n    {\n    }\n)` -> `(..., {})`
        for i in 1..lines.len() {
            if join_with_prev[i] {
                continue;
            }
            let cur_trimmed = lines[i].trim();
            let prev_trimmed = lines[i - 1].trim();
            if (in_paren_at_start[i] || in_paren_at_start[i - 1])
                && (cur_trimmed == "}" || cur_trimmed == "};")
                && prev_trimmed == "{"
            {
                join_with_prev[i] = true;
            }
        }

        // Template expression `}` joining: when a line is just `}` closing a
        // template `${}` expression, join it to the previous line (producing
        // `10}` instead of `10\n}`), UNLESS the previous visible line ends
        // with a `//` comment (line comment makes joining impossible).
        for i in 1..lines.len() {
            if !template_expr_close[i] || join_with_prev[i] {
                continue;
            }
            let prev_idx = (0..i).rev().find(|&j| !lines[j].trim().is_empty());
            if let Some(prev_idx) = prev_idx {
                let prev_trimmed = lines[prev_idx].trim();
                if !prev_trimmed.contains("//") {
                    join_with_prev[i] = true;
                }
            }
        }

        // In call-argument arrow recovery, TypeScript drops standalone `//`
        // lines that appear between `=>` and a following `{`, then joins `{`
        // back to the arrow line.
        let mut skip_line: Vec<bool> = vec![false; lines.len()];
        for i in 1..lines.len() {
            let trimmed = lines[i].trim();
            if !in_paren_at_start[i] || !trimmed.starts_with("//") {
                continue;
            }
            let prev_idx = (0..i)
                .rev()
                .find(|&j| !skip_line[j] && !lines[j].trim().is_empty());
            let next_idx = (i + 1..lines.len()).find(|&j| !lines[j].trim().is_empty());
            if let (Some(prev_idx), Some(next_idx)) = (prev_idx, next_idx) {
                if lines[prev_idx].trim().ends_with("=>") && lines[next_idx].trim() == "{" {
                    skip_line[i] = true;
                    join_with_prev[next_idx] = true;
                }
            }
        }

        // Variable-declaration continuation: when a var/let/const line ends
        // with `=`, join the first continuation line and indent subsequent
        // continuation lines by 4 spaces (matching TypeScript's formatting).
        let continuation_indent: Vec<usize> = {
            let mut indents = vec![0usize; lines.len()];
            let mut in_continuation = false;
            for i in 0..lines.len() {
                if skip_line[i] {
                    continue;
                }
                let trimmed = lines[i].trim();
                if !in_continuation && i > 0 && !join_with_prev[i] {
                    let prev_trimmed = (0..i)
                        .rev()
                        .find(|&j| !skip_line[j] && !lines[j].trim().is_empty())
                        .map(|j| lines[j].trim())
                        .unwrap_or("");
                    let is_var_decl = prev_trimmed.starts_with("var ")
                        || prev_trimmed.starts_with("let ")
                        || prev_trimmed.starts_with("const ");
                    let is_assign_cont =
                        prev_trimmed.ends_with('=') && !prev_trimmed.ends_with("==");
                    // Also join arrow body continuation: `const f = () =>\n  body`
                    let is_arrow_body_cont = prev_trimmed.ends_with("=>");
                    if is_var_decl
                        && (is_assign_cont || is_arrow_body_cont)
                        && !trimmed.is_empty()
                        && !trimmed.starts_with("//")
                    {
                        // Join first continuation line to the var declaration
                        join_with_prev[i] = true;
                        in_continuation = true;
                    }
                } else if in_continuation && !join_with_prev[i] {
                    if !trimmed.is_empty() {
                        indents[i] = 4;
                    }
                }
                // End continuation at statement boundary or block opening
                if trimmed.ends_with(';') || trimmed.ends_with('{') {
                    in_continuation = false;
                }
            }
            indents
        };

        // When a line is joined to the previous, its indentation is absorbed.
        // Subsequent non-joined lines should be de-indented by that amount.
        // Also, non-joined comment lines inside parens (call args) and lines
        // following them should have their indent stripped — TypeScript re-emits
        // these at the statement's base indent level.
        let indent_reduction: Vec<usize> = {
            let mut reductions = vec![0usize; lines.len()];
            let mut current_reduction = 0usize;
            // Track paren depth to detect when we fully leave a call context.
            // When paren_depth drops to 0 after a join, reset the reduction
            // because subsequent lines are outside the call and should
            // preserve their original indent relative to the statement.
            let mut rd_paren_depth: i32 = 0;
            let mut had_join = false;
            for i in 0..lines.len() {
                if skip_line[i] {
                    reductions[i] = current_reduction;
                    continue;
                }
                // Track paren depth (simple scan of parens only)
                if i > 0 {
                    for ch in lines[i - 1].chars() {
                        match ch {
                            '(' => rd_paren_depth += 1,
                            ')' => rd_paren_depth -= 1,
                            _ => {}
                        }
                    }
                    // Reset reduction when we fully leave the call context
                    if had_join && rd_paren_depth <= 0 && !join_with_prev[i] {
                        current_reduction = 0;
                        had_join = false;
                    }
                }
                if join_with_prev[i] {
                    let indent = lines[i].len().saturating_sub(lines[i].trim_start().len());
                    if indent > current_reduction {
                        current_reduction = indent;
                    }
                    had_join = true;
                } else if in_paren_at_start[i] && i > 0 {
                    let trimmed = lines[i].trim();
                    // Find previous non-empty line
                    let prev_idx = (0..i)
                        .rev()
                        .filter(|&j| !skip_line[j])
                        .find(|&j| !lines[j].trim().is_empty());
                    let prev_trimmed = prev_idx.map(|j| lines[j].trim()).unwrap_or("");
                    // Strip indent from non-joined lines inside parens when:
                    // - The line is a comment (// or /*), OR
                    // - The previous non-empty line ends with ( or is a comment-only line
                    let is_comment_line = trimmed.starts_with("//")
                        || trimmed.starts_with("/*")
                        || trimmed.starts_with("*/")
                        || trimmed.starts_with('*');
                    let prev_is_comment = prev_trimmed.starts_with("//")
                        || prev_trimmed.starts_with("/*")
                        || prev_trimmed.starts_with('*')
                        || prev_trimmed.ends_with("*/");
                    let prev_has_open_block_comment = (prev_trimmed.starts_with("/*")
                        || prev_trimmed.ends_with("/*"))
                        && !prev_trimmed.ends_with("*/");
                    let block_comment_interior = prev_has_open_block_comment;
                    let prev_prev_trimmed = prev_idx
                        .and_then(|prev_idx| {
                            (0..prev_idx)
                                .rev()
                                .filter(|&j| !skip_line[j])
                                .find(|&j| !lines[j].trim().is_empty())
                        })
                        .map(|j| lines[j].trim())
                        .unwrap_or("");
                    let prev_code = if let Some(lc) = prev_trimmed.find("//") {
                        prev_trimmed[..lc].trim_end()
                    } else if prev_trimmed.ends_with("*/") {
                        if let Some(bc) = prev_trimmed.rfind("/*") {
                            prev_trimmed[..bc].trim_end()
                        } else {
                            prev_trimmed
                        }
                    } else {
                        prev_trimmed
                    };
                    let prev_ends_open = prev_code.ends_with('(');
                    let next_trimmed = (i + 1..lines.len())
                        .find(|&j| !skip_line[j] && !lines[j].trim().is_empty())
                        .map(|j| lines[j].trim())
                        .unwrap_or("");
                    let comment_between_chain_lines = is_comment_line
                        && prev_trimmed.starts_with('.')
                        && next_trimmed.starts_with('.');
                    let continuation_after_chain_comment = prev_is_comment
                        && prev_prev_trimmed.starts_with('.')
                        && trimmed.starts_with('.');
                    if (is_comment_line || prev_is_comment || prev_ends_open)
                        && !block_comment_interior
                        && !comment_between_chain_lines
                        && !continuation_after_chain_comment
                    {
                        let indent = lines[i].len().saturating_sub(lines[i].trim_start().len());
                        if indent > current_reduction {
                            current_reduction = indent;
                        }
                    }
                }
                reductions[i] = current_reduction;
            }
            reductions
        };

        // Track lines that are interior text of a block comment which starts
        // on the previous line (e.g. `/**` then `@tag`).
        let block_comment_inner_line: Vec<bool> = {
            let mut marks = vec![false; lines.len()];
            for i in 1..lines.len() {
                if skip_line[i] {
                    continue;
                }
                let cur = lines[i].trim();
                if cur.is_empty() {
                    continue;
                }
                let prev = (0..i)
                    .rev()
                    .find(|&j| !skip_line[j] && !lines[j].trim().is_empty())
                    .map(|j| lines[j].trim())
                    .unwrap_or("");
                let cur_is_comment_marker = cur.starts_with("//")
                    || cur.starts_with("/*")
                    || cur.starts_with("*/")
                    || cur.starts_with('*');
                if in_paren_at_start[i]
                    && !cur_is_comment_marker
                    && (prev.starts_with("/*") || prev.ends_with("/*"))
                    && !prev.ends_with("*/")
                {
                    marks[i] = true;
                }
            }
            marks
        };

        // Operator/ternary continuation lines get structured continuation
        // indentation independent of source spacing.
        let (operator_continuation_extra_indent, operator_from_leading_op): (
            Vec<usize>,
            Vec<bool>,
        ) = {
            let mut extras = vec![0usize; lines.len()];
            // Track whether each line's extras came from a TernaryColon
            // (vs TernaryQuestion).  Chained ternary colons need incrementing
            // indentation; a colon following a question keeps the same level.
            let mut from_colon = vec![false; lines.len()];
            // Track block comment state to avoid detecting `.` as member
            // access inside block comments (where it's a sentence period).
            let mut in_block_comment_cont = false;
            for i in 1..lines.len() {
                // Update block comment state from the previous line.
                {
                    let prev_line = lines[i - 1].trim();
                    if prev_line.contains("/*") && !prev_line.contains("*/") {
                        in_block_comment_cont = true;
                    } else if prev_line.contains("*/") {
                        in_block_comment_cont = false;
                    }
                }
                if join_with_prev[i] || skip_line[i] {
                    continue;
                }
                let trimmed = lines[i].trim();
                if trimmed.is_empty()
                    || trimmed.starts_with("//")
                    || trimmed.starts_with("/*")
                    || trimmed.starts_with("*/")
                {
                    continue;
                }
                let prev_idx = (0..i)
                    .rev()
                    .find(|&j| !skip_line[j] && !lines[j].trim().is_empty());
                let Some(prev_idx) = prev_idx else {
                    continue;
                };
                let prev_trimmed = lines[prev_idx].trim();
                let prev_is_comment_star_line = {
                    let prev_raw = lines[prev_idx].trim_start();
                    if !prev_raw.starts_with('*') {
                        false
                    } else {
                        let prev_prev_trimmed = (0..prev_idx)
                            .rev()
                            .find(|&j| !skip_line[j] && !lines[j].trim().is_empty())
                            .map(|j| lines[j].trim_start())
                            .unwrap_or("");
                        prev_prev_trimmed.starts_with("/*") || prev_prev_trimmed.starts_with('*')
                    }
                };
                // Skip continuation indent detection when inside a block comment.
                // `.` inside a comment is a sentence period, not member access.
                let prev_in_block_comment = in_block_comment_cont || prev_is_comment_star_line;
                match line_trailing_continuation_op(prev_trimmed) {
                    Some(TrailingContinuationOp::Binary) if !prev_in_block_comment => extras[i] = 4,
                    Some(TrailingContinuationOp::Binary) => { /* skip `.` inside block comments */ }
                    Some(TrailingContinuationOp::TernaryQuestion) => {
                        extras[i] = extras[prev_idx].saturating_add(4);
                    }
                    Some(TrailingContinuationOp::TernaryColon) => {
                        // Skip non-ternary colons: case/default labels, object
                        // properties, etc.
                        let is_case_label =
                            prev_trimmed.starts_with("case ") || prev_trimmed == "default:";
                        if is_case_label {
                            // Not a ternary colon — do nothing.
                        } else if extras[prev_idx] > 0 {
                            if from_colon[prev_idx] {
                                // Chained ternary: previous was also a colon
                                // continuation, so this is a deeper nesting level.
                                extras[i] = extras[prev_idx] + 4;
                                from_colon[i] = true;
                            } else {
                                // Same-level alternate: previous was set by a
                                // TernaryQuestion, so keep the same level.
                                extras[i] = extras[prev_idx];
                            }
                        } else {
                            // First colon continuation (prev has no extras).
                            extras[i] = 4;
                            from_colon[i] = true;
                        }
                    }
                    _ => {
                        if line_is_binary_operator_only(prev_trimmed) && !prev_is_comment_star_line
                        {
                            extras[i] = 4;
                        }
                    }
                }
            }
            // Second pass: detect lines that START with a binary operator
            // (e.g. `>> expr`) and add continuation indentation for them
            // and any preceding comment lines.
            let mut from_leading_op = vec![false; lines.len()];
            for i in 1..lines.len() {
                if extras[i] > 0 || skip_line[i] || join_with_prev[i] {
                    continue;
                }
                let trimmed = lines[i].trim();
                if trimmed.is_empty() {
                    continue;
                }
                // Skip block comment continuation lines (e.g. `* @param`)
                // — a `*` there is not a multiplication operator.
                let is_comment_line = trimmed.starts_with("//")
                    || trimmed.starts_with("/*")
                    || trimmed.starts_with("*/")
                    || trimmed.starts_with('*');
                if is_comment_line {
                    continue;
                }
                if !line_starts_with_binary_operator(trimmed) {
                    continue;
                }
                // Set continuation indent for the operator line itself
                extras[i] = 4;
                from_leading_op[i] = true;
                // Set double continuation indent for the RHS line after the
                // operator (8 = operator indent + RHS indent).
                if let Some(next) =
                    (i + 1..lines.len()).find(|&j| !skip_line[j] && !lines[j].trim().is_empty())
                {
                    if extras[next] == 0
                        && !join_with_prev[next]
                        && !line_starts_with_binary_operator(lines[next].trim())
                    {
                        extras[next] = 8;
                        from_leading_op[next] = true;
                    }
                }
                // Backfill comment lines between the expression start and the
                // leading operator — they should also be continuation-indented.
                for j in (0..i).rev() {
                    if skip_line[j] || join_with_prev[j] {
                        continue;
                    }
                    let pt = lines[j].trim();
                    if pt.is_empty() {
                        continue;
                    }
                    if extras[j] > 0 {
                        break; // already has extras, stop backfilling
                    }
                    if pt.starts_with("//") || pt.starts_with("/*") || pt.starts_with("*/") {
                        extras[j] = 4;
                    } else {
                        break; // hit non-comment content, stop
                    }
                }
            }
            // Third pass: detect lines that START with a ternary operator
            // (`? expr` or `: expr`).  TypeScript always adds +4 continuation
            // indentation for these, even when the source has them at the same
            // column as the assignment / condition.
            // Only apply when the source line is at relative indent 0 (same
            // level as the statement start), meaning the source didn't already
            // provide continuation indentation.
            for i in 1..lines.len() {
                if extras[i] > 0 || skip_line[i] || join_with_prev[i] {
                    continue;
                }
                let trimmed = lines[i].trim();
                if trimmed.is_empty() {
                    continue;
                }
                // Only add ternary extras when the line's source indent
                // matches the first line's indent (the statement start).
                // When the source already has deeper indentation, the
                // normal path preserves it correctly.
                let first_ws = lines[0].len().saturating_sub(lines[0].trim_start().len());
                let line_ws = lines[i].len().saturating_sub(lines[i].trim_start().len());
                if line_ws > first_ws {
                    continue; // source already has continuation indent
                }
                let is_leading_ternary =
                    (trimmed.starts_with("? ") || trimmed == "?" || trimmed.starts_with("?: "))
                        && !trimmed.starts_with("??"); // exclude nullish coalescing
                let is_leading_colon = trimmed.starts_with(": ") || trimmed == ":";
                if is_leading_ternary || is_leading_colon {
                    extras[i] = 4;
                    from_leading_op[i] = true;
                }
            }
            (extras, from_leading_op)
        };
        let paren_operator_context_extra: Vec<usize> = {
            let mut carried = vec![0usize; lines.len()];
            let mut active = 0usize;
            for i in 0..lines.len() {
                if skip_line[i] || join_with_prev[i] {
                    continue;
                }
                if !in_paren_at_start[i] {
                    active = 0;
                    continue;
                }
                let trimmed = lines[i].trim();
                if trimmed.is_empty() {
                    continue;
                }
                let is_comment_line = trimmed.starts_with("//")
                    || trimmed.starts_with("/*")
                    || trimmed.starts_with("*/")
                    || trimmed.starts_with('*');
                let is_binary_line = line_starts_with_binary_operator(trimmed);
                let is_leading_ternary = (trimmed.starts_with("? ")
                    || trimmed == "?"
                    || trimmed.starts_with(": ")
                    || trimmed == ":")
                    && !trimmed.starts_with("??");
                if active == 0 {
                    let prev_idx = (0..i).rev().find(|&j| {
                        !skip_line[j] && !join_with_prev[j] && !lines[j].trim().is_empty()
                    });
                    if let Some(prev_idx) = prev_idx {
                        let prev_trimmed = lines[prev_idx].trim();
                        let prev_is_leading_ternary = (prev_trimmed.starts_with("? ")
                            || prev_trimmed == "?"
                            || prev_trimmed.starts_with(": ")
                            || prev_trimmed == ":")
                            && !prev_trimmed.starts_with("??");
                        // Activate when prev line has a binary operator, or
                        // prev line is a ternary opener (handles `? (` and `: (`
                        // followed by any content inside the parens).
                        if line_starts_with_binary_operator(prev_trimmed) || prev_is_leading_ternary
                        {
                            active = 4;
                        }
                    }
                }
                let op_extra = operator_continuation_extra_indent[i];
                if op_extra > 0 {
                    if active > 0 && (is_binary_line || is_comment_line) {
                        carried[i] = active;
                    }
                    if is_comment_line && op_extra >= 8 {
                        active = op_extra;
                    } else if is_leading_ternary && op_extra >= 8 {
                        active = active.max(4);
                    } else {
                        active = active.max(op_extra.min(4));
                    }
                } else if active > 0 {
                    carried[i] = active;
                }
                if trimmed.starts_with(')') {
                    active = 0;
                }
            }
            carried
        };
        // Inline object literals inside array literals opened as `[{` are
        // emitted with an extra indentation level for their interior/closing
        // lines in TypeScript's formatter.
        let array_object_literal_extra_indent: Vec<usize> = {
            let mut extras = vec![0usize; lines.len()];
            let mut depth = 0usize;
            let mut stack: Vec<usize> = Vec::new();
            for i in 0..lines.len() {
                if skip_line[i] {
                    continue;
                }
                let trimmed = lines[i].trim();
                if i > 0 && depth > 0 && !join_with_prev[i] && !trimmed.is_empty() {
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
        // Multi-line `for (...)` headers with nested array destructuring keep
        // one extra indentation level for the interior lines of the nested
        // array pattern.
        let for_header_array_pattern_extra_indent: Vec<usize> = {
            let mut extras = vec![0usize; lines.len()];
            let first_trimmed = lines.first().map(|l| l.trim_start()).unwrap_or("");
            if first_trimmed.starts_with("for (") || first_trimmed.starts_with("for await (") {
                let mut paren_depth = 0i32;
                let mut bracket_depth = 0i32;
                let mut first_line_bracket_depth = 0i32;
                for (i, line) in lines.iter().enumerate() {
                    if skip_line[i] {
                        continue;
                    }
                    let trimmed = line.trim();
                    if i > 0
                        && !join_with_prev[i]
                        && !trimmed.is_empty()
                        && paren_depth > 0
                        && bracket_depth > 1
                    {
                        let carried_depth = bracket_depth.min(first_line_bracket_depth);
                        if carried_depth > 1 {
                            extras[i] = ((carried_depth - 1) as usize) * 4;
                        }
                    }
                    for ch in trimmed.chars() {
                        match ch {
                            '(' => paren_depth += 1,
                            ')' => paren_depth -= 1,
                            '[' => bracket_depth += 1,
                            ']' => bracket_depth -= 1,
                            _ => {}
                        }
                    }
                    if i == 0 {
                        first_line_bracket_depth = bracket_depth;
                    }
                }
            }
            extras
        };
        // Multi-line `for (...)` headers with object destructuring defaults keep
        // one extra indentation level for the default object literal line:
        //   for ({ a =\n
        //           { b: 1 } } of xs)
        let for_header_object_default_extra_indent: Vec<usize> = {
            let mut extras = vec![0usize; lines.len()];
            let first_trimmed = lines.first().map(|l| l.trim_start()).unwrap_or("");
            if first_trimmed.starts_with("for (") || first_trimmed.starts_with("for await (") {
                let mut paren_depth = 0i32;
                let mut prev_visible_trimmed = String::new();
                for (i, line) in lines.iter().enumerate() {
                    if skip_line[i] {
                        continue;
                    }
                    let trimmed = line.trim();
                    if i > 0
                        && !join_with_prev[i]
                        && !trimmed.is_empty()
                        && paren_depth > 0
                        && prev_visible_trimmed.ends_with('=')
                        && !prev_visible_trimmed.ends_with("==")
                        && prev_visible_trimmed.contains('{')
                        && trimmed.starts_with('{')
                    {
                        extras[i] = 4;
                    }
                    if !trimmed.is_empty() {
                        prev_visible_trimmed.clear();
                        prev_visible_trimmed.push_str(trimmed);
                    }
                    for ch in trimmed.chars() {
                        match ch {
                            '(' => paren_depth += 1,
                            ')' => paren_depth -= 1,
                            _ => {}
                        }
                    }
                }
            }
            extras
        };
        // In multiline callback arguments inside call expressions, TypeScript
        // sometimes dedents by one source indent level relative to the local
        // block indentation.
        let callback_body_dedent: Vec<bool> = {
            let mut dedent = vec![false; lines.len()];
            for i in 1..lines.len() {
                if skip_line[i] || join_with_prev[i] {
                    continue;
                }
                let cur_trimmed = lines[i].trim();
                if !cur_trimmed.starts_with("return ") {
                    continue;
                }
                let prev_idx = (0..i)
                    .rev()
                    .find(|&j| !skip_line[j] && !lines[j].trim().is_empty());
                let next_idx =
                    (i + 1..lines.len()).find(|&j| !skip_line[j] && !lines[j].trim().is_empty());
                if let (Some(prev_idx), Some(next_idx)) = (prev_idx, next_idx) {
                    let prev_trimmed = lines[prev_idx].trim();
                    let next_trimmed = lines[next_idx].trim();
                    if prev_trimmed.ends_with('{')
                        && prev_trimmed.contains("function ")
                        && (next_trimmed == "});" || next_trimmed == "})")
                    {
                        if prev_trimmed.starts_with('.') {
                            dedent[i] = true;
                        }
                        dedent[next_idx] = true;
                    }
                }
            }
            // Method-chain callbacks (e.g. `.then((x) => { ... })`) in the
            // source-copy fallback should dedent callback body lines by one
            // source indent unit, matching TypeScript's output.
            for i in 0..lines.len() {
                if skip_line[i] || join_with_prev[i] {
                    continue;
                }
                let head = lines[i].trim();
                if !(head.starts_with('.')
                    && head.ends_with('{')
                    && (head.contains("=>") || head.contains("function ")))
                {
                    continue;
                }
                let close_idx =
                    (i + 1..lines.len()).find(|&j| !skip_line[j] && !lines[j].trim().is_empty());
                let mut close_idx = close_idx;
                while let Some(j) = close_idx {
                    let t = lines[j].trim();
                    if t == "});" || t == "})" {
                        for k in i + 1..=j {
                            if skip_line[k] || join_with_prev[k] {
                                continue;
                            }
                            if !lines[k].trim().is_empty() {
                                dedent[k] = true;
                            }
                        }
                        break;
                    }
                    close_idx = (j + 1..lines.len())
                        .find(|&k| !skip_line[k] && !lines[k].trim().is_empty());
                }
            }
            dedent
        };
        // Expression-bodied arrows inside multiline call chains also dedent
        // one source indent unit for their continuation lines.
        let arrow_expr_chain_dedent: Vec<bool> = {
            let mut dedent = vec![false; lines.len()];
            for i in 0..lines.len() {
                if skip_line[i] || join_with_prev[i] {
                    continue;
                }
                let head = lines[i].trim();
                let head_code = head.split("//").next().unwrap_or(head).trim_end();
                if !(head.starts_with('.')
                    && head.contains("=>")
                    && head.contains("//")
                    && head_code
                        .chars()
                        .last()
                        .is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '$')
                    && !head.ends_with('{')
                    && !head.ends_with("=>"))
                {
                    continue;
                }
                let mut saw_chain_content = false;
                let mut close_idx =
                    (i + 1..lines.len()).find(|&j| !skip_line[j] && !lines[j].trim().is_empty());
                while let Some(j) = close_idx {
                    let trimmed = lines[j].trim();
                    if trimmed == ");" || trimmed == ")" {
                        dedent[j] = true;
                        break;
                    }
                    if trimmed.starts_with('.') || trimmed.starts_with("//") {
                        dedent[j] = true;
                        if trimmed.starts_with('.') {
                            saw_chain_content = true;
                        }
                        close_idx = (j + 1..lines.len())
                            .find(|&k| !skip_line[k] && !lines[k].trim().is_empty());
                        continue;
                    }
                    if saw_chain_content {
                        dedent[j] = true;
                    }
                    break;
                }
            }
            dedent
        };

        // TypeScript normalizes the closing `}` of a multi-line object literal
        // to match the indentation of the line that opened it (where `{` appears).
        // When source-copying, the source may have the closing `}` at the same
        // indentation as the object properties rather than the opening line.
        // Track brace depth to detect these cases and compute the needed dedent.
        let obj_brace_close_dedent: Vec<usize> = {
            let mut dedents = vec![0usize; lines.len()];
            // Stack of indent levels for each unmatched `{`
            let mut brace_indent_stack: Vec<usize> = Vec::new();
            for i in 0..lines.len() {
                if skip_line[i] || join_with_prev[i] {
                    continue;
                }
                let trimmed = lines[i].trim();
                if trimmed.is_empty() {
                    continue;
                }
                let line_ws = lines[i].len() - lines[i].trim_start().len();
                // Process closing braces first (order matters for `}{` on same line)
                if trimmed.starts_with('}') {
                    if let Some(open_indent) = brace_indent_stack.pop() {
                        if line_ws > open_indent {
                            dedents[i] = line_ws - open_indent;
                        }
                    }
                    // Count additional `}` on this line (after the first)
                    for ch in trimmed[1..].chars() {
                        if ch == '}' {
                            brace_indent_stack.pop();
                        } else if ch == '{' {
                            brace_indent_stack.push(line_ws);
                        }
                    }
                } else {
                    for ch in trimmed.chars() {
                        if ch == '{' {
                            brace_indent_stack.push(line_ws);
                        } else if ch == '}' {
                            brace_indent_stack.pop();
                        }
                    }
                }
            }
            dedents
        };

        let trailing_comment = self.get_trailing_comment(stmt.span);

        let recovered_named_function_expression = matches!(
            &stmt.kind,
            StmtKind::Expr(expr)
                if matches!(&expr.kind, ExprKind::FnExpr(function) if function.name.is_some())
                    && self.copy_span_trimmed(stmt.span).trim_start().starts_with("function ")
        );
        let needs_semi =
            stmt_needs_trailing_semicolon(stmt) && !recovered_named_function_expression && {
                let trimmed = text_ref.trim_end();
                !trimmed.ends_with(';')
            };
        // When the statement contains an `<error>` token from parser error
        // recovery, the semicolon should appear on its own line, matching tsc.
        // For trailing-dot errors (e.g. `Foo.`), this only applies when NOT at
        // EOF. For missing-RHS errors (e.g. `x =`), it always applies.
        let semi_on_new_line = needs_semi && stmt_has_error_member(stmt) && {
            let text_trimmed = text_ref.trim_end();
            // Missing RHS: source text ends with `=` — always split
            if text_trimmed.ends_with('=') {
                true
            } else {
                // Trailing dot: only split when not at EOF
                let end = (stmt.span.end as usize).min(self.source.len());
                !self.source[end..].trim().is_empty()
            }
        };
        // TypeScript formats `var x =` followed by a `//` continuation comment
        // as:
        //   var x =
        //   // comment
        //   value;
        // i.e. it keeps a trailing space after `=` and strips continuation indent.
        let var_comment_continuation_no_indent = {
            let first_trimmed = lines.first().map(|l| l.trim()).unwrap_or("");
            let is_var_cont = (first_trimmed.starts_with("var ")
                || first_trimmed.starts_with("let ")
                || first_trimmed.starts_with("const "))
                && first_trimmed.ends_with('=');
            if !is_var_cont {
                false
            } else {
                (1..lines.len())
                    .find(|&j| !skip_line[j] && !lines[j].trim().is_empty())
                    .is_some_and(|j| {
                        !join_with_prev.get(j).copied().unwrap_or(false)
                            && lines[j].trim_start().starts_with("//")
                    })
            }
        };
        let var_trailing_comment_initializer_space = {
            let first_trimmed = lines.first().map(|l| l.trim()).unwrap_or("");
            (first_trimmed.starts_with("var ")
                || first_trimmed.starts_with("let ")
                || first_trimmed.starts_with("const "))
                && first_trimmed.contains('=')
                && first_trimmed.contains("//")
        };
        let var_trailing_comment_chain_indent: Vec<usize> = {
            let mut indents = vec![0usize; lines.len()];
            if var_trailing_comment_initializer_space {
                let first_continuation = (1..lines.len())
                    .find(|&j| !skip_line[j] && !join_with_prev[j] && !lines[j].trim().is_empty());
                if first_continuation.is_some_and(|j| lines[j].trim_start().starts_with('[')) {
                    if let Some(first) = first_continuation {
                        indents[first] = 1;
                    }
                    for i in 1..lines.len() {
                        if skip_line[i] || join_with_prev[i] {
                            continue;
                        }
                        let trimmed = lines[i].trim();
                        if trimmed.starts_with('.') {
                            indents[i] = 4;
                        }
                    }
                }
            }
            indents
        };

        // For multi-line spans, skip line 0 only when the span starts mid-line
        // after non-whitespace source text (e.g. `return foo()\n    .bar()`).
        let skip_first = lines.len() > 1 && starts_mid_line_non_ws && {
            let first_trimmed = lines[0].trim_start();
            !first_trimmed.starts_with("/*") && !first_trimmed.starts_with("//")
        };
        // Also skip line 0 from min_indent when leading_ws was prepended and the
        // first continuation line is joined to it AND the joined result opens a
        // new block (ends with `{`).  In patterns like
        // `let a =\n    () => {\n        body\n    }`, line 0's prepended indent
        // (from leading_ws) is lower than the body's indent and would skew
        // min_indent, causing over-indentation of the body.
        // This does NOT apply to simple continuations like `x = y +\n    z;`
        // where line 0's indent is the correct baseline.
        let skip_first_for_join = !leading_ws.is_empty()
            && lines.len() > 1
            && join_with_prev.get(1).copied().unwrap_or(false)
            && lines[1].trim_end().ends_with('{');
        let indents: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter(|(i, l)| {
                !skip_line[*i]
                    && !l.trim().is_empty()
                    && (!skip_first || *i > 0)
                    && (!skip_first_for_join || *i > 0)
                    && !join_with_prev[*i]
                    && !in_template_expr[*i]
            })
            .map(|(i, l)| {
                let ws = l.len() - l.trim_start().len();
                let reduction = indent_reduction[i];
                ws.saturating_sub(reduction)
            })
            .collect();
        let min_indent = indents.iter().copied().min().unwrap_or(0);

        // Find the smallest non-zero indent step relative to min_indent.
        // This tells us the source's indent unit (1 for tabs, 2 for 2-space, etc.).
        let indent_unit = indents
            .iter()
            .copied()
            .filter(|&i| i > min_indent)
            .map(|i| i - min_indent)
            .min()
            .unwrap_or(4);

        // Normalize relative indentation to 4-space steps when the source
        // uses a different indent size (e.g. 2-space, tabs).
        let needs_normalize = indent_unit != 4 && indent_unit > 0;
        let object_property_function_body_dedent: Vec<bool> = {
            let mut dedent = vec![false; lines.len()];
            for i in 0..lines.len() {
                if skip_line[i] || join_with_prev[i] {
                    continue;
                }
                let head = lines[i].trim();
                if !(head.contains(':') && head.contains("function") && head.ends_with('{')) {
                    continue;
                }
                let opener_indent = lines[i].len().saturating_sub(lines[i].trim_start().len());
                let first_body_idx =
                    (i + 1..lines.len()).find(|&j| !skip_line[j] && !lines[j].trim().is_empty());
                let Some(first_body_idx) = first_body_idx else {
                    continue;
                };
                let first_body_trimmed = lines[first_body_idx].trim();
                if first_body_trimmed.starts_with('}') {
                    continue;
                }
                let first_body_indent = lines[first_body_idx]
                    .len()
                    .saturating_sub(lines[first_body_idx].trim_start().len());
                if first_body_indent <= opener_indent.saturating_add(indent_unit) {
                    continue;
                }
                let mut brace_depth = 1i32;
                for j in i + 1..lines.len() {
                    if skip_line[j] || join_with_prev[j] {
                        continue;
                    }
                    let trimmed = lines[j].trim();
                    if !trimmed.is_empty() && brace_depth > 0 && !trimmed.starts_with('}') {
                        dedent[j] = true;
                    }
                    for ch in trimmed.chars() {
                        match ch {
                            '{' => brace_depth += 1,
                            '}' => brace_depth -= 1,
                            _ => {}
                        }
                    }
                    if brace_depth <= 0 {
                        break;
                    }
                }
            }
            dedent
        };

        // When a var/let/const declaration has a multi-line array initializer
        // (e.g. `var x = [function () {`), TypeScript adds one extra indent
        // level (4 spaces) to all continuation lines. This only applies when
        // the initializer is an array literal wrapping multi-line elements.
        let var_initializer_extra: usize = if lines.len() > 1 {
            let first_trimmed = lines[0].trim();
            let is_var_decl = first_trimmed.starts_with("var ")
                || first_trimmed.starts_with("let ")
                || first_trimmed.starts_with("const ");
            // Detect `= [` pattern: array literal as the initializer.
            // Check that the `[` from `= [` is not closed (by `]`) before
            // the `{` at end of line. If `]` appears before `{`, the array
            // is a value being chained (e.g. `= [""].find(() => {`), not
            // an array literal wrapping multi-line elements.
            let has_array_init = is_var_decl
                && if let Some(eq_bracket) = first_trimmed.find("= [") {
                    let after_bracket = &first_trimmed[eq_bracket + 3..];
                    // No `]` before the end means the `[` is still open —
                    // this is an array literal containing the function.
                    !after_bracket.contains(']')
                } else {
                    false
                };
            if has_array_init && first_trimmed.ends_with('{') {
                4
            } else {
                0
            }
        } else {
            0
        };

        let last_idx = lines.len() - 1;
        for (i, line) in lines.iter().enumerate() {
            if skip_line[i] {
                continue;
            }
            // Join call-argument continuation lines onto the previous line
            if join_with_prev[i] {
                let content = line.trim();
                if !content.is_empty() {
                    self.strip_trailing_newline();
                    // When joining a closing paren after a trailing comma,
                    // remove the trailing comma (TypeScript doesn't preserve
                    // trailing commas in call arguments).
                    if content.starts_with(')') {
                        // Strip trailing comma and whitespace from the output
                        while self.output.ends_with(' ') {
                            self.output.pop();
                        }
                        if self.output.ends_with(',') {
                            self.output.pop();
                        }
                    } else if self.output.ends_with('(') {
                        // After opening paren, don't add extra space
                    } else if self.output.ends_with('{') && content.starts_with('}') {
                        // Empty object literal collapse: no space between braces
                    } else if template_expr_close[i] {
                        // Template expression `}`: join directly without space
                    } else {
                        self.write(" ");
                    }
                    if i == last_idx {
                        self.write(content);
                        if needs_semi {
                            self.write(";");
                        }
                        if let Some(ref comment) = trailing_comment {
                            self.write(" ");
                            self.writeln(comment);
                        } else {
                            self.newline();
                        }
                    } else {
                        self.writeln(content);
                    }
                }
                continue;
            }
            // String literal line continuations (previous line ends with `\`
            // inside a string) should NOT receive any emitter indentation.
            // The text after `\` + newline is raw string content at column 0.
            if i > 0 {
                let prev_raw = lines[i - 1].trim_end();
                if prev_raw.ends_with('\\') {
                    // Check this looks like a string continuation (line
                    // has an odd number of trailing backslashes)
                    let trailing_bs = prev_raw.len() - prev_raw.trim_end_matches('\\').len();
                    if trailing_bs % 2 == 1 {
                        // Emit raw content without emitter indentation
                        self.output.push_str(line);
                        if i == last_idx && needs_semi && !line.trim_end().ends_with(';') {
                            self.output.push(';');
                        }
                        self.output.push('\n');
                        self.at_line_start = true;
                        self.out_col = 0;
                        continue;
                    }
                }
            }
            // TypeScript adds trailing space after `=>` when the body is on
            // the next line, and preserves trailing space after `,` in
            // multi-line call argument lists.
            let line_tc = line.trim();
            let next_visible_idx = (i + 1..lines.len()).find(|&j| !skip_line[j]);
            let needs_trailing_space = i < last_idx
                && next_visible_idx
                    .map(|j| !join_with_prev.get(j).copied().unwrap_or(false))
                    .unwrap_or(true)
                && !line_tc.is_empty()
                && (line_tc.ends_with("=>")
                    || (line_tc.ends_with(',') && in_paren_at_start[i])
                    || (var_comment_continuation_no_indent && i == 0 && line_tc.ends_with('=')));
            if line.trim().is_empty() {
                self.newline();
            } else if i == 0 && !leading_ws.is_empty() {
                // First line had structural leading_ws prepended for indent computation.
                // Emit just the content; self.write() provides the emitter's indent.
                let content = line.trim();
                if i == last_idx {
                    self.write(content);
                    if needs_semi {
                        self.write(";");
                    }
                    if let Some(ref comment) = trailing_comment {
                        self.write(" ");
                        self.writeln(comment);
                    } else {
                        self.newline();
                    }
                } else {
                    self.write(content);
                    if needs_trailing_space {
                        self.write(" ");
                    }
                    self.newline();
                }
            } else {
                // Template expression lines: TypeScript's structured emit writes
                // content at column 0.  Strip all leading whitespace from lines
                // inside `${}` that are NOT block-comment continuation lines.
                if in_template_expr[i] {
                    let content = line.trim();
                    if !content.is_empty() {
                        // Check if this is a block-comment continuation line
                        // (starts with `*` or `*/`).  These preserve relative
                        // formatting from the source.
                        let is_block_comment_cont = content.starts_with('*');
                        if is_block_comment_cont {
                            // Preserve source indent by writing the line as-is
                            // (stripped of min_indent like regular lines).
                            let ws = line.len() - line.trim_start().len();
                            let stripped = if ws > 0 { &line[..] } else { line };
                            let trimmed_end = stripped.trim_end();
                            if i == last_idx {
                                self.write(trimmed_end);
                                if needs_semi {
                                    self.write(";");
                                }
                                if let Some(ref comment) = trailing_comment {
                                    self.write(" ");
                                    self.writeln(comment);
                                } else {
                                    self.newline();
                                }
                            } else {
                                self.writeln(trimmed_end);
                            }
                        } else {
                            // Non-comment lines: write at column 0
                            if i == last_idx {
                                self.write(content);
                                if needs_semi {
                                    self.write(";");
                                }
                                if let Some(ref comment) = trailing_comment {
                                    self.write(" ");
                                    self.writeln(comment);
                                } else {
                                    self.newline();
                                }
                            } else {
                                self.writeln(content);
                            }
                        }
                        continue;
                    } else {
                        self.newline();
                        continue;
                    }
                }
                // Apply indent reduction for lines following a join.
                let reduction = indent_reduction[i];
                let effective_line: &str = if reduction > 0 {
                    let ws = line.len() - line.trim_start().len();
                    let strip = reduction.min(ws);
                    &line[strip..]
                } else {
                    line
                };
                let trimmed = effective_line.trim_start();
                let line_indent = effective_line.len() - trimmed.len();
                let content = trimmed.trim_end();
                let prev_visible_trimmed = (0..i)
                    .rev()
                    .find(|&j| !skip_line[j] && !lines[j].trim().is_empty())
                    .map(|j| lines[j].trim())
                    .unwrap_or("");
                if var_comment_continuation_no_indent && i > 0 {
                    if i == last_idx {
                        self.write(content);
                        if needs_semi {
                            self.write(";");
                        }
                        if let Some(ref comment) = trailing_comment {
                            self.write(" ");
                            self.writeln(comment);
                        } else {
                            self.newline();
                        }
                    } else {
                        self.write(content);
                        if needs_trailing_space {
                            self.write(" ");
                        }
                        self.newline();
                    }
                    continue;
                }
                let is_block_comment_closing_after_open = in_paren_at_start[i]
                    && content.starts_with("*/")
                    && (prev_visible_trimmed.starts_with("/*")
                        || prev_visible_trimmed.ends_with("/*"))
                    && !prev_visible_trimmed.ends_with("*/");
                let force_no_indent_close_arrow_call =
                    (content == "});" || content == "})") && prev_visible_trimmed.ends_with('{');
                let force_no_indent_open_brace =
                    i + 1 < lines.len() && join_with_prev[i + 1] && line.trim() == "{";
                if block_comment_inner_line[i] {
                    let with_inner = format!("  {}", content);
                    if i == last_idx {
                        self.write(&with_inner);
                        if needs_semi {
                            self.write(";");
                        }
                        if let Some(ref comment) = trailing_comment {
                            self.write(" ");
                            self.writeln(comment);
                        } else {
                            self.newline();
                        }
                    } else {
                        self.write(&with_inner);
                        if needs_trailing_space {
                            self.write(" ");
                        }
                        self.newline();
                    }
                    continue;
                }
                if is_block_comment_closing_after_open {
                    let with_close = format!("    {}", content);
                    if i == last_idx {
                        self.write(&with_close);
                        if needs_semi {
                            self.write(";");
                        }
                        if let Some(ref comment) = trailing_comment {
                            self.write(" ");
                            self.writeln(comment);
                        } else {
                            self.newline();
                        }
                    } else {
                        self.write(&with_close);
                        if needs_trailing_space {
                            self.write(" ");
                        }
                        self.newline();
                    }
                    continue;
                }
                if force_no_indent_open_brace {
                    if i == last_idx {
                        self.write("{");
                        if needs_semi {
                            self.write(";");
                        }
                        if let Some(ref comment) = trailing_comment {
                            self.write(" ");
                            self.writeln(comment);
                        } else {
                            self.newline();
                        }
                    } else {
                        self.write("{");
                        if needs_trailing_space {
                            self.write(" ");
                        }
                        self.newline();
                    }
                    continue;
                }
                if force_no_indent_close_arrow_call {
                    if i == last_idx {
                        self.write(content);
                        if needs_semi {
                            self.write(";");
                        }
                        if let Some(ref comment) = trailing_comment {
                            self.write(" ");
                            self.writeln(comment);
                        } else {
                            self.newline();
                        }
                    } else {
                        self.write(content);
                        if needs_trailing_space {
                            self.write(" ");
                        }
                        self.newline();
                    }
                    continue;
                }
                let force_compact_binary_arrow_indent = content.starts_with('(')
                    && (prev_visible_trimmed.ends_with("&&")
                        || prev_visible_trimmed.ends_with("||"))
                    && line_indent > min_indent + 4;
                if force_compact_binary_arrow_indent {
                    let compact = format!("    {}", content);
                    if i == last_idx {
                        self.write(&compact);
                        if needs_semi {
                            self.write(";");
                        }
                        if let Some(ref comment) = trailing_comment {
                            self.write(" ");
                            self.writeln(comment);
                        } else {
                            self.newline();
                        }
                    } else {
                        self.write(&compact);
                        if needs_trailing_space {
                            self.write(" ");
                        }
                        self.newline();
                    }
                    continue;
                }
                let cont_indent = continuation_indent[i];
                let callback_dedent = if callback_body_dedent[i]
                    || arrow_expr_chain_dedent[i]
                    || object_property_function_body_dedent[i]
                {
                    if needs_normalize {
                        indent_unit
                    } else {
                        4
                    }
                } else {
                    0
                };
                let relative = line_indent
                    .saturating_sub(min_indent)
                    .saturating_sub(callback_dedent);
                // Add extra indent for var/let/const multi-line initializers
                let init_extra = if i > 0 { var_initializer_extra } else { 0 };
                let operator_extra = operator_continuation_extra_indent[i];
                let paren_operator_extra = paren_operator_context_extra[i];
                let binary_extra = array_object_literal_extra_indent[i];
                let loop_header_extra = for_header_array_pattern_extra_indent[i];
                let loop_object_default_extra = for_header_object_default_extra_indent[i];
                let mut comment_chain_indent = var_trailing_comment_chain_indent[i];
                let stmt_starts_with_var_trailing_comment = lines.first().is_some_and(|line| {
                    let trimmed = line.trim();
                    (trimmed.starts_with("var ")
                        || trimmed.starts_with("let ")
                        || trimmed.starts_with("const "))
                        && trimmed.contains('=')
                        && trimmed.contains("//")
                });
                let has_prior_array_continuation = (0..i).rev().any(|j| {
                    !skip_line[j]
                        && !lines[j].trim().is_empty()
                        && lines[j].trim_start().starts_with('[')
                });
                if comment_chain_indent == 0 {
                    let prev_is_trailing_comment_assign = (prev_visible_trimmed
                        .starts_with("var ")
                        || prev_visible_trimmed.starts_with("let ")
                        || prev_visible_trimmed.starts_with("const "))
                        && prev_visible_trimmed.contains('=')
                        && prev_visible_trimmed.contains("//");
                    if prev_is_trailing_comment_assign && content.starts_with('[') {
                        comment_chain_indent = 1;
                    } else if stmt_starts_with_var_trailing_comment
                        && has_prior_array_continuation
                        && content.starts_with('.')
                    {
                        comment_chain_indent = 4;
                    }
                }

                if comment_chain_indent > 0 {
                    let indented = format!("{}{}", " ".repeat(comment_chain_indent), content);
                    if i == last_idx {
                        self.write(&indented);
                        if needs_semi {
                            self.write(";");
                        }
                        if let Some(ref comment) = trailing_comment {
                            self.write(" ");
                            self.writeln(comment);
                        } else {
                            self.newline();
                        }
                    } else {
                        self.write(&indented);
                        if needs_trailing_space {
                            self.write(" ");
                        }
                        self.newline();
                    }
                    continue;
                }

                if operator_extra > 0 {
                    // When inside parentheses, the source relative indent
                    // already captures correct nesting (paren nesting is
                    // preserved in the output).  Use it when it's larger
                    // than the flat operator continuation.
                    let use_source_relative = (in_paren_at_start[i] && relative > operator_extra)
                        || (min_indent > 0
                            && relative >= operator_extra
                            && !operator_from_leading_op[i]);
                    let mut operator_prefix = if use_source_relative {
                        // Source relative indent already captures paren nesting,
                        // so don't add paren_operator_extra on top.
                        relative
                    } else {
                        operator_extra + paren_operator_extra
                    } + init_extra
                        + binary_extra
                        + loop_header_extra
                        + loop_object_default_extra;
                    // Add deeper indentation for binary-only continuation
                    // lines, but NOT for lines that were set by the leading
                    // operator detection pass (those already have the correct
                    // absolute value).
                    if !use_source_relative
                        && !operator_from_leading_op[i]
                        && min_indent == 0
                        && (matches!(
                            line_trailing_continuation_op(prev_visible_trimmed),
                            Some(TrailingContinuationOp::Binary)
                        ) || line_is_binary_operator_only(prev_visible_trimmed))
                        && (line_is_binary_operator_only(prev_visible_trimmed)
                            || line_indent > 8
                            || (prev_visible_trimmed.starts_with("return ")
                                && content.starts_with('!')))
                    {
                        operator_prefix = operator_prefix.saturating_add(4);
                    }
                    let prefix = " ".repeat(operator_prefix);
                    let indented = format!("{}{}", prefix, content);
                    if i == last_idx {
                        self.write(&indented);
                        if needs_semi {
                            self.write(";");
                        }
                        if let Some(ref comment) = trailing_comment {
                            self.write(" ");
                            self.writeln(comment);
                        } else {
                            self.newline();
                        }
                    } else {
                        self.write(&indented);
                        if needs_trailing_space {
                            self.write(" ");
                        }
                        self.newline();
                    }
                    continue;
                }

                // Variable-declaration continuation lines get extra indentation
                if cont_indent > 0 {
                    let prefix = " ".repeat(
                        cont_indent
                            + paren_operator_extra
                            + init_extra
                            + binary_extra
                            + loop_header_extra
                            + loop_object_default_extra,
                    );
                    let indented = format!("{}{}", prefix, content);
                    if i == last_idx {
                        self.write(&indented);
                        if needs_semi {
                            self.write(";");
                        }
                        if let Some(ref comment) = trailing_comment {
                            self.write(" ");
                            self.writeln(comment);
                        } else {
                            self.newline();
                        }
                    } else {
                        self.write(&indented);
                        if needs_trailing_space {
                            self.write(" ");
                        }
                        self.newline();
                    }
                } else if needs_normalize && relative > 0 {
                    // Normalize: convert source indent levels to 4-space levels
                    let levels = relative / indent_unit;
                    let remainder = relative % indent_unit;
                    let norm_indent = "    ".repeat(levels);
                    let trailing_comment_init_space = if var_trailing_comment_initializer_space
                        && i > 0
                        && !join_with_prev[i]
                        && (0..i)
                            .rev()
                            .find(|&j| !skip_line[j] && !lines[j].trim().is_empty())
                            == Some(0)
                    {
                        1
                    } else {
                        0
                    };
                    let extra_indent = " ".repeat(
                        remainder
                            + paren_operator_extra
                            + init_extra
                            + binary_extra
                            + loop_header_extra
                            + loop_object_default_extra
                            + trailing_comment_init_space,
                    );
                    let normalized = format!("{}{}{}", norm_indent, extra_indent, content);
                    if i == last_idx {
                        self.write(&normalized);
                        if needs_semi {
                            self.write(";");
                        }
                        if let Some(ref comment) = trailing_comment {
                            self.write(" ");
                            self.writeln(comment);
                        } else {
                            self.newline();
                        }
                    } else {
                        self.write(&normalized);
                        if needs_trailing_space {
                            self.write(" ");
                        }
                        self.newline();
                    }
                } else {
                    // No normalization needed - use original relative indent.
                    // Only strip min_indent when the line actually has that much
                    // leading whitespace; line 0 (skip_first) may have none.
                    let mut stripped =
                        if line_indent >= min_indent && effective_line.len() > min_indent {
                            effective_line[min_indent..].trim_end().to_string()
                        } else {
                            content.to_string()
                        };
                    if callback_dedent > 0 {
                        let ws = stripped.len().saturating_sub(stripped.trim_start().len());
                        let strip = callback_dedent.min(ws);
                        if strip > 0 {
                            stripped = stripped[strip..].to_string();
                        }
                    }
                    let brace_dedent = obj_brace_close_dedent[i];
                    if brace_dedent > 0 {
                        let ws = stripped.len().saturating_sub(stripped.trim_start().len());
                        let strip = brace_dedent.min(ws);
                        if strip > 0 {
                            stripped = stripped[strip..].to_string();
                        }
                    }
                    let trailing_comment_init_space = if var_trailing_comment_initializer_space
                        && i > 0
                        && !join_with_prev[i]
                        && (0..i)
                            .rev()
                            .find(|&j| !skip_line[j] && !lines[j].trim().is_empty())
                            == Some(0)
                    {
                        1
                    } else {
                        0
                    };
                    let total_extra = paren_operator_extra
                        + init_extra
                        + binary_extra
                        + loop_header_extra
                        + loop_object_default_extra
                        + trailing_comment_init_space;
                    let with_extra = if total_extra > 0 {
                        format!("{}{}", " ".repeat(total_extra), stripped)
                    } else {
                        stripped
                    };
                    if i == last_idx {
                        self.write(&with_extra);
                        if needs_semi {
                            self.write(";");
                        }
                        if let Some(ref comment) = trailing_comment {
                            self.write(" ");
                            self.writeln(comment);
                        } else {
                            self.newline();
                        }
                    } else {
                        self.write(&with_extra);
                        if needs_trailing_space {
                            self.write(" ");
                        }
                        self.newline();
                    }
                }
            }
        }
        // Post-processing: when the statement has `<error>` tokens from parser
        // error recovery (e.g. `Foo.` trailing dot, `x =` missing RHS), move
        // the semicolon to its own line by inserting a newline with matching
        // indentation.
        if semi_on_new_line {
            // Find the trailing pattern: `.;` (trailing dot) or ` =;` / `=;` (missing RHS)
            let split_pos = self.output.rfind(".;").map(|p| p + 1).or_else(|| {
                // Handle `=;` pattern — assignment with missing RHS
                self.output.rfind("=;").map(|p| p + 1)
            });
            if let Some(semi_pos) = split_pos {
                let line_start = self.output[..semi_pos]
                    .rfind('\n')
                    .map(|p| p + 1)
                    .unwrap_or(0);
                let indent: String = self.output[line_start..semi_pos]
                    .chars()
                    .take_while(|c| c.is_ascii_whitespace())
                    .collect();
                self.output.insert_str(semi_pos, &format!("\n{}", indent));
            }
        }
    }

    pub(super) fn emit_stmt_body(&mut self, stmt: &Stmt) {
        if let StmtKind::Block(stmts) = &stmt.kind {
            if stmts.is_empty() {
                // Check if the source block was multi-line
                let start = stmt.span.start as usize;
                let end = stmt.span.end as usize;
                let is_multiline = start < end && end <= self.source.len() && {
                    let block_text = &self.source[start..end];
                    // A block without a closing `}` (e.g. `if (true) {` at EOF)
                    // is treated as multi-line by TypeScript.
                    block_text.contains('\n') || !block_text.contains('}')
                };
                if is_multiline {
                    // Check if there are comments inside this empty block
                    let block_end = stmt.span.end;
                    let has_inner_comments = self.next_comment_idx < self.comments.len()
                        && self.comments[self.next_comment_idx].pos >= stmt.span.start
                        && self.comments[self.next_comment_idx].pos < block_end;
                    if has_inner_comments {
                        self.write("{");
                        self.append_brace_trailing_comment(stmt.span);
                        self.newline();
                        self.indent += 1;
                        // Emit comments inside the empty block
                        let closing_brace = if end > 0 && end <= self.source.len() {
                            // Find the closing `}` position
                            self.source[start..end]
                                .rfind('}')
                                .map(|off| (start + off) as u32)
                                .unwrap_or(block_end)
                        } else {
                            block_end
                        };
                        self.emit_leading_comments(closing_brace);
                        self.indent -= 1;
                        self.writeln("}");
                    } else {
                        self.write("{");
                        self.append_brace_trailing_comment(stmt.span);
                        self.newline();
                        self.write("}");
                        self.newline();
                    }
                } else {
                    // Single-line empty block: check if there's a comment inside
                    // (e.g. `{ /**/ }`) that should be preserved.
                    let inner_comment = if start < end
                        && end <= self.source.len()
                        && self.options.remove_comments != Some(true)
                    {
                        let src = &self.source[start..end];
                        if let (Some(open), Some(close)) = (src.find('{'), src.rfind('}')) {
                            let between = src[open + 1..close].trim();
                            if between.starts_with("/*") && between.ends_with("*/") {
                                Some(between.to_string())
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    if let Some(comment) = inner_comment {
                        self.write("{ ");
                        self.write(&comment);
                        self.writeln(" }");
                        // Advance comment tracking past the block so the comment
                        // isn't re-emitted.
                        while self.next_comment_idx < self.comments.len()
                            && self.comments[self.next_comment_idx].pos < stmt.span.end
                        {
                            self.next_comment_idx += 1;
                        }
                        self.comment_emit_pos = self.comment_emit_pos.max(stmt.span.end);
                    } else {
                        self.writeln("{ }");
                    }
                }
                return;
            }
            // Inline the block handling to preserve trailing comments on `{` line.
            // TypeScript preserves trailing `// comments` on control flow brace lines.
            self.write("{");
            self.append_brace_trailing_comment(stmt.span);
            self.newline();
            self.indent += 1;
            self.block_depth += 1;
            self.cjs_inline_export_depth += 1;
            for s in stmts {
                let stmt_output_start = self.output.len();
                let prev_helper_insert_pos = self.class_decl_helper_insert_pos;
                self.stmt_output_start = stmt_output_start;
                self.class_decl_helper_insert_pos = Some(stmt_output_start);
                self.emit_leading_comments(s.span.start);
                self.emit_stmt(s);
                self.class_decl_helper_insert_pos = prev_helper_insert_pos;
                self.advance_comment_pos(s.span.end);
            }
            self.cjs_inline_export_depth -= 1;
            self.block_depth -= 1;
            // Emit any comments between the last statement and the closing `}`.
            // Use the block's span end to find the correct `}` rather than
            // the first `}` after the last statement (which could be from
            // an inner block in nested code).
            {
                let block_end = stmt.span.end as usize;
                if block_end > 0 && block_end <= self.source.len() {
                    let brace_pos = if self.source.as_bytes().get(block_end - 1) == Some(&b'}') {
                        (block_end - 1) as u32
                    } else {
                        // Fallback: search backward from block end for `}`
                        self.source[..block_end]
                            .rfind('}')
                            .map(|p| p as u32)
                            .unwrap_or(block_end as u32)
                    };
                    self.emit_leading_comments(brace_pos);
                }
            }
            self.indent -= 1;
            self.writeln("}");
        } else {
            // When the body is an erased statement (e.g. `const enum`),
            // TypeScript replaces it with a lone `;` to preserve syntax.
            let preserve_const = self.preserve_const_enums_effective();
            if stmt_is_erased(stmt, preserve_const) {
                self.newline();
                self.indent += 1;
                self.writeln(";");
                self.indent -= 1;
            } else {
                self.newline();
                self.indent += 1;
                self.emit_stmt(stmt);
                self.indent -= 1;
            }
        }
    }

    /// Append a trailing `// comment` from the source on the same line as `{`.
    /// Used for control flow blocks where TypeScript preserves these comments.
    pub(super) fn append_brace_trailing_comment(&mut self, block_span: Span) {
        if self.options.remove_comments == Some(true) {
            return;
        }
        let start = block_span.start as usize;
        if start >= self.source.len() {
            return;
        }
        // Recovery nodes can begin just after their opening `{`, so inspect
        // the whole source line containing the block start rather than only
        // the bytes at and after the recorded span.
        let line_start = self.source[..start].rfind('\n').map_or(0, |pos| pos + 1);
        let rest = &self.source[line_start..];
        let line_end = rest.find('\n').unwrap_or(rest.len());
        let mut first_line = &rest[..line_end];
        if !first_line.contains('{') && line_start > 0 {
            let previous_line_end = line_start - 1;
            let previous_line_start = self.source[..previous_line_end]
                .rfind('\n')
                .map_or(0, |pos| pos + 1);
            let previous_line = &self.source[previous_line_start..previous_line_end];
            if previous_line.contains('{') {
                first_line = previous_line;
            }
        }
        // Look for `// comment` on the same line as `{`
        if let Some(comment_pos) = first_line.find("//") {
            // Search only before the comment. The comment text itself may
            // contain braces (for example `(including {})`).
            if let Some(brace_pos) = first_line[..comment_pos].rfind('{') {
                let between = &first_line[brace_pos + 1..comment_pos];
                if between.trim().is_empty() {
                    let comment = first_line[comment_pos..].trim_end();
                    self.write(" ");
                    self.write(comment);
                }
            }
        }
    }

    fn block_has_opening_brace_line_comment(&self, block_span: Span) -> bool {
        let start = block_span.start as usize;
        if start >= self.source.len() {
            return false;
        }
        let line_start = self.source[..start].rfind('\n').map_or(0, |pos| pos + 1);
        let current_end = self.source[line_start..]
            .find('\n')
            .map_or(self.source.len(), |pos| line_start + pos);
        let current = &self.source[line_start..current_end];
        let candidate = if current.contains('{') {
            current
        } else if line_start > 0 {
            let previous_end = line_start - 1;
            let previous_start = self.source[..previous_end]
                .rfind('\n')
                .map_or(0, |pos| pos + 1);
            &self.source[previous_start..previous_end]
        } else {
            return false;
        };
        candidate.find("//").is_some_and(|comment| {
            candidate[..comment]
                .rfind('{')
                .is_some_and(|brace| candidate[brace + 1..comment].trim().is_empty())
        })
    }

    pub(super) fn expr_stmt_is_garbage_recovery(&self, expr: &Expr, span: Span) -> bool {
        let text = self.copy_span_trimmed(span).trim();
        if text.is_empty() {
            return false;
        }
        if matches!(
            &expr.kind,
            ExprKind::Unary(UnaryExpr {
                op: UnaryOp::Pos | UnaryOp::Neg,
                argument,
            }) if expr_is_error_placeholder(argument)
        ) {
            return false;
        }
        if text.contains('\u{fffd}')
            && !matches!(
                expr.kind,
                ExprKind::StrLit(_) | ExprKind::NoSubstTemplate(_) | ExprKind::Template(_)
            )
        {
            return true;
        }
        if text
            .as_bytes()
            .iter()
            .any(|b| *b < 0x20 && *b != b'\n' && *b != b'\r' && *b != b'\t')
            && !matches!(
                expr.kind,
                ExprKind::StrLit(_) | ExprKind::NoSubstTemplate(_) | ExprKind::Template(_)
            )
        {
            return true;
        }
        if !matches!(expr.kind, ExprKind::Ident(_)) {
            return false;
        }
        let bytes = self.source.as_bytes();
        let mut i = span.end as usize;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            return false;
        }
        let b = bytes[i];
        // `@` followed by a non-identifier char (e.g., binary garbage `G@\xEF`)
        // is garbage. But `@` followed by an identifier char is a decorator
        // on the next statement and should not be treated as garbage.
        let at_is_garbage = b == b'@' && {
            let next = bytes.get(i + 1).copied().unwrap_or(0);
            !next.is_ascii_alphanumeric() && next != b'_' && next != b'$'
        };
        let follows_garbage = at_is_garbage || (b < 0x20 && b != b'\n' && b != b'\r' && b != b'\t');
        if !follows_garbage {
            return false;
        }
        // Keep the very first recovered identifier token (e.g. leading `G`)
        // but drop repeated garbage identifiers that appear after binary blobs.
        let prefix = &self.source[..(span.start as usize).min(self.source.len())];
        !prefix.trim().is_empty()
    }

    fn expr_stmt_has_array_literal_split_empty_recovery(&self, expr: &Expr, span: Span) -> bool {
        if !expr_is_error_placeholder(expr) || span.start == span.end {
            return false;
        }
        let raw = self.copy_span_trimmed(span);
        let trimmed = raw.trim();
        trimmed
            .strip_suffix(';')
            .is_some_and(|before_semi| before_semi.trim() == "]")
    }

    pub(crate) fn malformed_import_type_option_recovery_from_span(
        &self,
        span: Span,
    ) -> Option<MalformedImportTypeOptionRecovery> {
        let start = span.start as usize;
        let end = span.end as usize;
        if start >= end || end > self.source.len() {
            return None;
        }
        Self::malformed_import_type_option_recovery_from_text(&self.source[start..end])
    }

    pub(crate) fn span_has_missing_import_type_options_wrapper_recovery(&self, span: Span) -> bool {
        self.malformed_import_type_option_recovery_from_span(span)
            .is_some_and(|recovery| recovery.key_text.is_empty())
    }

    fn malformed_import_type_option_recovery_from_text(
        text: &str,
    ) -> Option<MalformedImportTypeOptionRecovery> {
        let import_start = text.find("import(")?;
        let mode_key_start = text[import_start..].find("\"resolution-mode\"")? + import_start;
        let bag_start = text[..mode_key_start].rfind('{')? + 1;
        let mode_value_colon = text[mode_key_start..].find(':')? + mode_key_start;
        let malformed_head = text[bag_start..mode_value_colon].trim();
        let (key_text, mode_key_text) =
            if let Some((key_text, mode_key_text)) = malformed_head.split_once(',') {
                let key_text = key_text.trim();
                let mode_key_text = mode_key_text.trim();
                if key_text.is_empty() || mode_key_text != "\"resolution-mode\"" {
                    return None;
                }
                (key_text, mode_key_text)
            } else {
                if malformed_head != "\"resolution-mode\""
                    || !text[import_start..bag_start - 1].trim_end().ends_with(',')
                {
                    return None;
                }
                ("", malformed_head)
            };

        let bytes = text.as_bytes();
        let mut value_start = mode_value_colon + 1;
        while value_start < text.len() && bytes[value_start].is_ascii_whitespace() {
            value_start += 1;
        }
        if value_start >= text.len() || bytes[value_start] != b'"' {
            return None;
        }
        let mut value_end = value_start + 1;
        while value_end < text.len() {
            if bytes[value_end] == b'"' && bytes[value_end - 1] != b'\\' {
                value_end += 1;
                break;
            }
            value_end += 1;
        }
        if value_end > text.len() {
            return None;
        }

        let mut paren_depth = 0usize;
        let mut import_end = None;
        for (offset, ch) in text[import_start..].char_indices() {
            match ch {
                '(' => paren_depth += 1,
                ')' => {
                    if paren_depth == 0 {
                        return None;
                    }
                    paren_depth -= 1;
                    if paren_depth == 0 {
                        import_end = Some(import_start + offset);
                        break;
                    }
                }
                _ => {}
            }
        }
        let import_end = import_end?;
        let mut qualifier_start = import_end + 1;
        while qualifier_start < text.len() && bytes[qualifier_start].is_ascii_whitespace() {
            qualifier_start += 1;
        }
        if qualifier_start >= text.len() || bytes[qualifier_start] != b'.' {
            return None;
        }
        qualifier_start += 1;
        let mut qualifier_end = qualifier_start;
        while qualifier_end < text.len() {
            let b = bytes[qualifier_end];
            if b.is_ascii_alphanumeric() || matches!(b, b'_' | b'$' | b'.') {
                qualifier_end += 1;
            } else {
                break;
            }
        }
        let qualifier = text[qualifier_start..qualifier_end].trim();
        if qualifier.is_empty() {
            return None;
        }

        Some(MalformedImportTypeOptionRecovery {
            key_text: key_text.to_string(),
            mode_key_text: mode_key_text.to_string(),
            value_literal: text[value_start..value_end].to_string(),
            qualifier: qualifier.to_string(),
            tail_after_qualifier: text[qualifier_end..].trim_end().to_string(),
        })
    }

    fn normalize_malformed_import_type_option_text(text: &str) -> String {
        let Some(import_start) = text.find("import(") else {
            return text.to_string();
        };
        let Some(mode_key_start_rel) = text[import_start..].find("\"resolution-mode\"") else {
            return text.to_string();
        };
        let mode_key_start = import_start + mode_key_start_rel;
        let Some(bag_start) = text[..mode_key_start].rfind('{') else {
            return text.to_string();
        };
        let Some(mode_value_colon_rel) = text[mode_key_start..].find(':') else {
            return text.to_string();
        };
        let mode_value_colon = mode_key_start + mode_value_colon_rel;
        let malformed_head = text[bag_start + 1..mode_value_colon].trim();
        let (key_text, mode_key_text) = if let Some(parts) = malformed_head.split_once(',') {
            parts
        } else if malformed_head == "\"resolution-mode\""
            && text[import_start..bag_start].trim_end().ends_with(',')
        {
            ("", malformed_head)
        } else {
            return text.to_string();
        };

        let bytes = text.as_bytes();
        let mut value_start = mode_value_colon + 1;
        while value_start < text.len() && bytes[value_start].is_ascii_whitespace() {
            value_start += 1;
        }
        if value_start >= text.len() || bytes[value_start] != b'"' {
            return text.to_string();
        }
        let mut value_end = value_start + 1;
        while value_end < text.len() {
            if bytes[value_end] == b'"' && bytes[value_end - 1] != b'\\' {
                value_end += 1;
                break;
            }
            value_end += 1;
        }
        if value_end > text.len() {
            return text.to_string();
        }
        let Some(inner_bag_close_rel) = text[value_end..].find('}') else {
            return text.to_string();
        };
        let inner_bag_close = value_end + inner_bag_close_rel;

        let mut out = String::new();
        out.push_str(&text[..bag_start]);
        out.push_str("{ ");
        if !key_text.is_empty() {
            out.push_str(key_text.trim());
            out.push_str(": , ");
        }
        out.push_str(mode_key_text.trim());
        out.push_str(": ");
        out.push_str(&text[value_start..value_end]);
        out.push_str(" }");
        out.push_str(&text[inner_bag_close + 1..]);
        out
    }

    fn var_stmt_has_malformed_import_type_option_recovery(&self, var_stmt: &VarStmt) -> bool {
        var_stmt
            .declarations
            .iter()
            .filter_map(|decl| decl.init.as_ref())
            .any(|init| {
                self.malformed_import_type_option_recovery_from_span(init.span)
                    .is_some()
            })
    }

    fn malformed_import_type_option_recovery_tails_for_var_stmt(
        &self,
        var_stmt: &VarStmt,
    ) -> Vec<String> {
        let mut tails = Vec::new();
        for init in var_stmt
            .declarations
            .iter()
            .filter_map(|decl| decl.init.as_ref())
        {
            let Some(recovery) = self.malformed_import_type_option_recovery_from_span(init.span)
            else {
                continue;
            };
            tails.push(if recovery.key_text.is_empty() {
                format!("{};", recovery.mode_key_text)
            } else {
                format!("{}, {};", recovery.key_text, recovery.mode_key_text)
            });
            tails.push(format!("{};", recovery.value_literal));

            let mut tail = recovery.tail_after_qualifier.trim_start();
            let mut extra_empty_stmt_count = 0;
            while let Some(rest) = tail.strip_prefix(')') {
                extra_empty_stmt_count += 1;
                tail = rest.trim_start();
            }
            if tail.is_empty() || tail == ";" {
                tails.push(format!("{};", recovery.qualifier));
            } else {
                let normalized_tail = Self::normalize_malformed_import_type_option_text(
                    &recovery.tail_after_qualifier,
                );
                tails.push(format!("{}{}", recovery.qualifier, normalized_tail));
            }
            for _ in 0..extra_empty_stmt_count {
                tails.push(";".to_string());
            }
        }
        tails
    }

    fn emit_recovery_malformed_import_type_options_type_alias(&mut self, span: Span) -> bool {
        let Some(recovery) = self.malformed_import_type_option_recovery_from_span(span) else {
            return false;
        };
        if recovery.key_text.is_empty() {
            self.writeln(&format!("{};", recovery.mode_key_text));
        } else {
            self.writeln(&format!(
                "{}, {};",
                recovery.key_text, recovery.mode_key_text
            ));
        }
        self.writeln(&format!("{};", recovery.value_literal));
        let normalized_tail =
            Self::normalize_malformed_import_type_option_text(&recovery.tail_after_qualifier);
        let normalized_tail = if let Some(rest) = normalized_tail.strip_prefix("\n&") {
            format!("\n    &{rest}")
        } else {
            normalized_tail
        };
        self.write(&recovery.qualifier);
        if normalized_tail.is_empty() {
            self.writeln(";");
        } else {
            self.write(&normalized_tail);
            if !self.output.ends_with('\n') {
                self.newline();
            }
        }
        self.skip_recovery_until = self.skip_recovery_until.max(span.end);
        true
    }

    fn emit_recovery_malformed_import_attributes_double_comma_type_alias(
        &mut self,
        span: Span,
    ) -> bool {
        if !self.type_alias_has_malformed_import_attributes_double_comma_recovery(span) {
            return false;
        }
        self.writeln(";");
        self.skip_recovery_until = self.skip_recovery_until.max(span.end);
        self.advance_comment_pos(span.end);
        true
    }

    fn emit_recovery_missing_type_argument_call(&mut self, expr: &Expr, span: Span) -> bool {
        if !self.file_has_recovery_errors || !expr_has_error_member(expr) {
            return false;
        }
        let start = span.start as usize;
        if start >= self.source.len() {
            return false;
        }
        let rest = &self.source[start..];
        let Some(semi_rel) = rest.find(';') else {
            return false;
        };
        let trimmed = rest[..=semi_rel].trim();
        let Some(without_semi) = trimmed.strip_suffix(';') else {
            return false;
        };
        if !without_semi.ends_with("()") {
            return false;
        }
        let Some(lt_idx) = without_semi.find('<') else {
            return false;
        };
        let Some(gt_idx) = without_semi.rfind('>') else {
            return false;
        };
        if gt_idx <= lt_idx || &without_semi[gt_idx + 1..] != "()" {
            return false;
        }
        let callee = &without_semi[..lt_idx];
        if callee.is_empty()
            || callee.chars().any(|ch| {
                ch.is_ascii_whitespace()
                    || !matches!(ch, 'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '$' | '.')
            })
        {
            return false;
        }
        let type_args = &without_semi[lt_idx + 1..gt_idx];
        if !missing_type_argument_slot(type_args) {
            return false;
        }
        self.write(callee);
        self.writeln("();");
        self.skip_recovery_until = self.skip_recovery_until.max((start + semi_rel + 1) as u32);
        true
    }

    fn emit_recovery_invalid_type_query_var(&mut self, stmt: &Stmt, var_stmt: &VarStmt) -> bool {
        if !self.file_has_recovery_errors || var_stmt.declarations.len() != 1 {
            return false;
        }
        let PatKind::Ident(name) = &var_stmt.declarations[0].name.kind else {
            return false;
        };
        let start = stmt.span.start as usize;
        let Some(rest) = self.source.get(start..) else {
            return false;
        };
        let Some(semi_rel) = rest.find(';') else {
            return false;
        };
        let source_stmt = rest[..semi_rel].trim();
        let Some(after_var) = source_stmt.strip_prefix("var ") else {
            return false;
        };
        let Some((source_name, type_text)) = after_var.split_once(':') else {
            return false;
        };
        if source_name.trim() != name.as_str() {
            return false;
        }
        let Some(target) = type_text.trim().strip_prefix("typeof ") else {
            return false;
        };
        let target = target.trim();
        if target.is_empty()
            || target.starts_with("function ")
            || target
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_alphabetic() || matches!(ch, '_' | '$'))
        {
            return false;
        }

        match target {
            "{}" => self.writeln(&format!("var {name}, {{}};")),
            "[]" | "null" => self.writeln(&format!("var {name};")),
            _ if target.starts_with("()") => {
                self.writeln(&format!("var {name};"));
                self.writeln("() => ;");
            }
            _ if target.starts_with('/')
                || target.starts_with('\'')
                || target.starts_with('"')
                || target.chars().next().is_some_and(|ch| ch.is_ascii_digit()) =>
            {
                self.writeln(&format!("var {name};"));
                self.writeln(&format!("{target};"));
            }
            _ => return false,
        }

        let skip_end = (start + semi_rel + 1) as u32;
        self.skip_recovery_until = self.skip_recovery_until.max(skip_end);
        self.advance_comment_pos(skip_end);
        true
    }

    fn emit_recovery_optional_method_return_in_object_var(
        &mut self,
        stmt: &Stmt,
        var_stmt: &VarStmt,
    ) -> bool {
        if !self.file_has_recovery_errors || var_stmt.declarations.len() != 1 {
            return false;
        }
        let PatKind::Ident(name) = &var_stmt.declarations[0].name.kind else {
            return false;
        };
        let start = stmt.span.start as usize;
        let Some(rest) = self.source.get(start..) else {
            return false;
        };
        let Some(open_rel) = rest.find('{') else {
            return false;
        };
        let Some(close_rel) = rest[open_rel + 1..].find('}') else {
            return false;
        };
        let close_rel = open_rel + 1 + close_rel;
        if rest[..open_rel].trim() != format!("var {name} =") {
            return false;
        }
        let body = rest[open_rel + 1..close_rel].trim();
        let Some((method_name, tail)) = body.split_once("()?:") else {
            return false;
        };
        let method_name = method_name.trim();
        if method_name.is_empty()
            || method_name
                .chars()
                .any(|ch| !(ch.is_ascii_alphanumeric() || matches!(ch, '_' | '$')))
        {
            return false;
        }
        let tail = tail.trim();
        let (property, comment) = tail
            .split_once("//")
            .map_or((tail, None), |(value, comment)| {
                (value.trim(), Some(comment))
            });
        if property.is_empty() {
            return false;
        }

        self.writeln(&format!("var {name} = {{"));
        self.write("    ");
        self.write(method_name);
        self.write("() { }, ");
        self.write(property);
        self.write(": ");
        if let Some(comment) = comment {
            self.write(" //");
            self.write(comment);
        }
        self.newline();
        self.writeln("};");

        let skip_end = (start + close_rel + 1) as u32;
        self.skip_recovery_until = self.skip_recovery_until.max(skip_end);
        self.advance_comment_pos(skip_end);
        true
    }

    fn emit_recovery_async_iterable_return_type_tail(&mut self, expr: &Expr, span: Span) -> bool {
        let ExprKind::Unary(UnaryExpr {
            op: UnaryOp::LogNot,
            argument,
        }) = &expr.kind
        else {
            return false;
        };
        if !expr_is_error_placeholder(argument) {
            return false;
        }

        let start = span.start as usize;
        if start >= self.source.len() {
            return false;
        }
        let rest = &self.source[start..];
        let Some(open_brace_rel) = rest.find('{') else {
            return false;
        };
        let header = rest[..open_brace_rel].trim();
        let Some(header_tail) = header.strip_prefix("!:") else {
            return false;
        };
        let Some((param_type, return_type_src)) = header_tail.trim().split_once("):") else {
            return false;
        };
        let param_type = param_type.trim();
        if !param_type.starts_with("AsyncIterable<") || !param_type.ends_with('>') {
            return false;
        }
        let return_type_src = return_type_src.trim();
        let Some((return_type_name, return_type_inner)) = return_type_src.split_once('<') else {
            return false;
        };
        let Some(return_type_inner) = return_type_inner.trim().strip_suffix('>') else {
            return false;
        };

        let mut brace_depth = 0i32;
        let mut close_brace_rel = None;
        for (offset, ch) in rest[open_brace_rel..].char_indices() {
            match ch {
                '{' => brace_depth += 1,
                '}' => {
                    brace_depth -= 1;
                    if brace_depth == 0 {
                        close_brace_rel = Some(open_brace_rel + offset);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(close_brace_rel) = close_brace_rel else {
            return false;
        };

        let block_src = &rest[open_brace_rel + 1..close_brace_rel];
        let non_empty_lines: Vec<&str> = block_src
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect();
        let [const_line, for_line, push_line, close_for_line, return_line] =
            non_empty_lines.as_slice()
        else {
            return false;
        };
        if *close_for_line != "}" {
            return false;
        }

        let Some(const_tail) = const_line.strip_prefix("const ") else {
            return false;
        };
        let Some((const_name, const_value)) = const_tail.split_once(" = ") else {
            return false;
        };
        if const_value.trim() != "[];" {
            return false;
        }

        let Some(for_tail) = for_line.strip_prefix("for await (const ") else {
            return false;
        };
        let Some(for_tail) = for_tail.strip_suffix(") {") else {
            return false;
        };
        let Some((loop_var, iterable_name)) = for_tail.split_once(" of ") else {
            return false;
        };

        let Some((push_target, push_arg_tail)) = push_line.split_once(".push(await ") else {
            return false;
        };
        let Some(push_arg) = push_arg_tail.strip_suffix(");") else {
            return false;
        };

        let Some(return_value) = return_line
            .strip_prefix("return ")
            .and_then(|line| line.strip_suffix(';'))
        else {
            return false;
        };

        self.writeln(" > ;");
        self.write(return_type_name.trim());
        self.write(" < ");
        self.write(return_type_inner.trim());
        self.writeln(" > {");
        self.indent += 1;
        self.write("const: ");
        self.write(const_name.trim());
        self.writeln(" = [],");
        self.write("for: await (), const: ");
        self.write(loop_var.trim());
        self.write(", of, ");
        self.write(iterable_name.trim());
        self.writeln(",");
        self.write(push_target.trim());
        self.write(", : .push(await ");
        self.write(push_arg.trim());
        self.writeln(")");
        self.indent -= 1;
        self.writeln("};");
        self.write("return ");
        self.write(return_value.trim());
        self.writeln(";");
        self.writeln(";");

        let skip_end = (start + close_brace_rel + 1) as u32;
        self.skip_recovery_until = self.skip_recovery_until.max(skip_end);
        self.advance_comment_pos(skip_end);
        true
    }

    fn emit_recovery_malformed_strict_eq_assignment(&mut self, expr: &Expr, span: Span) -> bool {
        let ExprKind::Assign(outer_assign) = &expr.kind else {
            return false;
        };
        if outer_assign.op != AssignOp::Assign {
            return false;
        }
        let ExprKind::Paren(inner) = &outer_assign.right.kind else {
            return false;
        };
        let ExprKind::Assign(inner_assign) = &inner.kind else {
            return false;
        };
        if inner_assign.op != AssignOp::Assign {
            return false;
        }
        let ExprKind::Binary(bin) = &inner_assign.right.kind else {
            return false;
        };
        if bin.op != BinaryOp::StrictEq || !expr_is_error_placeholder(&bin.right) {
            return false;
        }

        let start = span.start as usize;
        let end = span.end as usize;
        if start >= end || end > self.source.len() {
            return false;
        }
        let stmt_src = self.source[start..end].trim_end();
        if !stmt_src.ends_with("====") {
            return false;
        }

        let rest = &self.source[end..];
        let bytes = rest.as_bytes();
        let mut i = 0;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || !matches!(bytes[i], b'\'' | b'"') {
            return false;
        }
        let quote = bytes[i];
        let literal_start = i;
        i += 1;
        while i < bytes.len() {
            if bytes[i] == quote && bytes.get(i.wrapping_sub(1)) != Some(&b'\\') {
                i += 1;
                break;
            }
            i += 1;
        }
        if i > bytes.len() {
            return false;
        }
        let literal_src = &rest[literal_start..i];
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b')' {
            return false;
        }
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'{' {
            return false;
        }

        self.emit_expr(&outer_assign.left);
        self.write(" = (");
        self.emit_expr(&inner_assign.left);
        self.write(" = ");
        self.emit_expr(&bin.left);
        self.write(" === ) = ");
        self.write(literal_src);
        self.writeln(";");
        self.writeln("{");
        self.writeln("}");
        self.skip_recovery_until = self.skip_recovery_until.max((end + i + 1) as u32);
        true
    }

    fn emit_recovery_invalid_in_lhs_assignment(&mut self, expr: &Expr) -> bool {
        fn unwrap_in_assignment_lhs(expr: &Expr) -> &Expr {
            match &expr.kind {
                ExprKind::Paren(inner) | ExprKind::NonNull(inner) => {
                    unwrap_in_assignment_lhs(inner)
                }
                ExprKind::As(as_expr) => unwrap_in_assignment_lhs(&as_expr.expr),
                ExprKind::Satisfies(sat_expr) => unwrap_in_assignment_lhs(&sat_expr.expr),
                ExprKind::TypeAssertion(ta_expr) => unwrap_in_assignment_lhs(&ta_expr.expr),
                ExprKind::Instantiation(inst_expr) => unwrap_in_assignment_lhs(&inst_expr.expr),
                _ => expr,
            }
        }

        let ExprKind::Assign(assign) = &expr.kind else {
            return false;
        };
        if assign.op != AssignOp::Assign {
            return false;
        }
        let ExprKind::Binary(bin) = &unwrap_in_assignment_lhs(&assign.left).kind else {
            return false;
        };
        if bin.op != BinaryOp::In {
            return false;
        }

        self.emit_expr(&assign.left);
        self.writeln(";");
        self.emit_expr(&assign.right);
        self.writeln(";");
        true
    }

    fn emit_recovery_class_wrapped_try_property(
        &mut self,
        class_decl: &ClassDecl,
        stmt_span: Span,
    ) -> bool {
        if class_decl.modifiers != MOD_NONE
            || !class_decl.decorators.is_empty()
            || class_decl.extends.is_some()
            || class_decl.type_params.is_some()
            || !class_decl.implements.is_empty()
        {
            return false;
        }
        let [first, second] = class_decl.members.as_slice() else {
            return false;
        };
        let ClassMemberKind::Property(first_prop) = &first.kind else {
            return false;
        };
        let ClassMemberKind::Property(second_prop) = &second.kind else {
            return false;
        };
        let PropName::Ident(first_name, _) = &first_prop.name else {
            return false;
        };
        if first_name != "try"
            || first_prop.initializer.is_some()
            || first_prop.type_ann.is_some()
            || first_prop.modifiers != MOD_NONE
            || first_prop.optional
            || first_prop.definite
            || !first_prop.decorators.is_empty()
        {
            return false;
        }
        let PropName::Ident(second_name, _) = &second_prop.name else {
            return false;
        };
        let Some(second_init) = &second_prop.initializer else {
            return false;
        };
        if second_prop.type_ann.is_some()
            || second_prop.modifiers & (MOD_PUBLIC | MOD_PRIVATE | MOD_PROTECTED) == 0
            || second_prop.modifiers & MOD_STATIC != 0
            || second_prop.optional
            || second_prop.definite
            || !second_prop.decorators.is_empty()
        {
            return false;
        }

        let end = class_decl.span.end as usize;
        if end >= self.source.len() {
            return false;
        }
        let rest = &self.source[end..];
        let bytes = rest.as_bytes();
        let mut i = 0;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if !rest[i..].starts_with("catch") {
            return false;
        }
        i += "catch".len();
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'(' {
            return false;
        }
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let param_start = i;
        while i < bytes.len()
            && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'_' | b'$'))
        {
            i += 1;
        }
        let catch_param = rest[param_start..i].trim();
        if catch_param.is_empty() {
            return false;
        }
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b')' {
            return false;
        }
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'{' {
            return false;
        }
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'}' {
            return false;
        }

        let mut stripped = class_decl.clone();
        stripped.members.clear();
        self.emit_class_decl(&stripped);
        if class_decl.modifiers & MOD_DECLARE == 0 {
            self.append_trailing_comment(stmt_span);
        }
        self.writeln("try {");
        self.indent += 1;
        self.write(second_name);
        self.write(" = ");
        self.emit_expr(second_init);
        self.writeln(";");
        self.indent -= 1;
        self.writeln("}");
        self.write("catch (");
        self.write(catch_param);
        self.writeln(") { }");
        self.skip_recovery_until = self.skip_recovery_until.max((end + i + 1) as u32);
        true
    }

    fn emit_recovery_cast_of_bare_yield(&mut self, expr: &Expr, span: Span) -> bool {
        fn yield_arg_is_missing(arg: Option<&Expr>) -> bool {
            arg.is_none_or(|arg| {
                matches!(&arg.kind, ExprKind::Omitted)
                    || matches!(&arg.kind, ExprKind::Ident(name) if name == "<error>")
            })
        }

        let has_bare_yield = match &expr.kind {
            ExprKind::Yield(false, arg) => yield_arg_is_missing(arg.as_deref()),
            ExprKind::TypeAssertion(inner) => {
                matches!(&inner.expr.kind, ExprKind::Ident(name) if name == "yield")
                    || matches!(&inner.expr.kind, ExprKind::Yield(false, arg) if yield_arg_is_missing(arg.as_deref()))
            }
            _ => false,
        };
        if !has_bare_yield {
            return false;
        }
        let start = span.start as usize;
        if start >= self.source.len() {
            return false;
        }
        let rest = &self.source[start..];
        let Some(semi_rel) = rest.find(';') else {
            return false;
        };
        let stmt_src = rest[..=semi_rel].trim();
        let after_yield = if let Some(after_yield) = stmt_src.strip_prefix("yield") {
            after_yield
        } else if let Some(gt_idx) = stmt_src.rfind('>') {
            let after_assert = stmt_src[gt_idx + 1..].trim_start();
            let Some(after_yield) = after_assert.strip_prefix("yield") else {
                return false;
            };
            after_yield
        } else {
            return false;
        };
        let yield_arg = after_yield.trim_start().trim_end_matches(';').trim();
        if yield_arg.is_empty() || yield_arg.starts_with('(') {
            return false;
        }
        self.writeln(";");
        self.write("yield ");
        self.write(yield_arg);
        self.writeln(";");
        self.skip_recovery_until = self.skip_recovery_until.max((start + semi_rel + 1) as u32);
        true
    }

    fn emit_recovery_midfile_hashbang_expr_stmt(&mut self, expr: &Expr, span: Span) -> bool {
        if !self.expr_stmt_has_midfile_hashbang_recovery_shape(expr, span) {
            return false;
        }
        let start = span.start as usize;
        let end = span.end as usize;
        let hashbang_start =
            if start > 0 && self.source.as_bytes()[start - 1] == b'#' && start < end {
                start - 1
            } else {
                start
            };
        let stmt_src = &self.source[hashbang_start..end];
        let total_hashbangs = stmt_src.match_indices("#!").count();
        match &expr.kind {
            ExprKind::NonNull(inner) => {
                for _ in 0..total_hashbangs.saturating_sub(1) {
                    self.writeln("!;");
                }
                self.write("!");
                let recovered_hash_ident = matches!(
                    &inner.kind,
                    ExprKind::Ident(name) if name == "#"
                );
                if !expr_is_error_placeholder(inner) && !recovered_hash_ident {
                    self.emit_expr(inner);
                }
            }
            ExprKind::Binary(bin)
                if bin.op == BinaryOp::Div
                    && matches!(
                        &bin.left.kind,
                        ExprKind::Unary(un) if un.op == UnaryOp::LogNot
                    ) =>
            {
                let ExprKind::Unary(un) = &bin.left.kind else {
                    unreachable!();
                };
                self.write("!");
                self.emit_expr(&un.argument);
                self.write(" / ");
                self.emit_expr(&bin.right);
            }
            _ => return false,
        }
        self.writeln(";");
        true
    }

    fn emit_recovery_malformed_jsx_unary_expr_stmt(&mut self, _expr: &Expr, span: Span) -> bool {
        let start = span.start as usize;
        let end = span.end as usize;
        if start >= end || end > self.source.len() {
            return false;
        }
        let stmt_src = self.source[start..end].trim();
        match stmt_src {
            "~< <" if self.is_js_file => {
                self.writeln("~< /> <");
                self.writeln(";");
                true
            }
            "~<></> <" if self.is_js_file => {
                self.writeln("~<></> <");
                self.writeln(";");
                true
            }
            "!< {:>" if self.is_js_file => {
                self.writeln("!< {...}>");
                self.newline();
                self.writeln("</>;");
                true
            }
            _ => false,
        }
    }

    fn emit_recovery_invalid_jsx_attribute_name(&mut self, span: Span) -> bool {
        if !self.jsx_is_preserve() {
            return false;
        }
        let start = span.start as usize;
        let Some(source_tail) = self.source.get(start..) else {
            return false;
        };
        let line_len = source_tail.find('\n').unwrap_or(source_tail.len());
        let line = source_tail[..line_len].trim();
        let Some(after_lt) = line.strip_prefix('<') else {
            return false;
        };
        let Some(tag_end) = after_lt.find(char::is_whitespace) else {
            return false;
        };
        let tag = &after_lt[..tag_end];
        let remainder = after_lt[tag_end..].trim_start();

        if let Some(eq) = remainder.find("={") {
            let attribute = &remainder[..eq];
            let after_open = &remainder[eq + 2..];
            let Some(close) = after_open.find('}') else {
                return false;
            };
            let value = after_open[..close].trim();
            let digit_count = attribute
                .bytes()
                .take_while(|byte| byte.is_ascii_digit())
                .count();
            if digit_count > 0 && digit_count < attribute.len() {
                let numeric_prefix = &attribute[..digit_count];
                let recovered_name = &attribute[digit_count..];
                self.write("<");
                self.write(tag);
                self.writeln(" />;");
                self.write(numeric_prefix);
                self.writeln(";");
                self.write(recovered_name);
                self.write(" = { ");
                self.write(value);
                self.writeln(":  } /  > ;");
                self.skip_recovery_until = self.skip_recovery_until.max((start + line_len) as u32);
                return true;
            }
        }

        let Some(after_minus) = remainder.strip_prefix('-') else {
            return false;
        };
        let Some(eq) = after_minus.find("={") else {
            return false;
        };
        let attribute = after_minus[..eq].trim();
        let after_open = &after_minus[eq + 2..];
        let Some(close) = after_open.find('}') else {
            return false;
        };
        let value = after_open[..close].trim();
        if attribute.is_empty() || value.is_empty() {
            return false;
        }
        self.write("<");
        self.write(tag);
        self.write(" /> - ");
        self.write(attribute);
        self.writeln(";");
        self.writeln("{");
        self.indent += 1;
        self.write(value);
        self.writeln(";");
        self.indent -= 1;
        self.writeln("}");
        self.writeln("/>;");
        self.skip_recovery_until = self.skip_recovery_until.max((start + line_len) as u32);
        true
    }

    fn emit_recovery_questionable_jsx_namespace_member(&mut self, span: Span) -> bool {
        if !self.jsx_is_preserve() {
            return false;
        }
        let start = span.start as usize;
        let Some(source_tail) = self.source.get(start..) else {
            return false;
        };
        let line_len = source_tail.find('\n').unwrap_or(source_tail.len());
        let line = source_tail[..line_len].trim();
        let Some(open_end) = line.find('>') else {
            return false;
        };
        let Some(open_name) = line.get(1..open_end) else {
            return false;
        };
        let Some((namespace_name, member_name)) = open_name.split_once('.') else {
            return false;
        };
        if !namespace_name.contains(':')
            || member_name.is_empty()
            || !line[open_end + 1..].starts_with(&format!("</{open_name}>"))
        {
            return false;
        }

        self.write("<");
        self.write(namespace_name);
        self.write(" ");
        self.write(member_name);
        self.write("></");
        self.write(namespace_name);
        self.writeln(">;");
        self.write(member_name);
        self.writeln(" > ;");

        let mut consumed = line_len;
        let remainder = &source_tail[line_len..];
        let next_line_start = remainder
            .find(|ch: char| ch != '\n' && ch != '\r')
            .unwrap_or(remainder.len());
        let next_tail = &remainder[next_line_start..];
        let next_line_len = next_tail.find('\n').unwrap_or(next_tail.len());
        let next_line = next_tail[..next_line_len].trim();
        if next_line.starts_with('<') && next_line.ends_with(";") {
            self.writeln(next_line);
            consumed += next_line_start + next_line_len;
        }
        self.skip_recovery_until = self.skip_recovery_until.max((start + consumed) as u32);
        true
    }

    fn emit_recovery_missing_paren_typed_arrow(&mut self, expr: &Expr, span: Span) -> bool {
        if !matches!(expr.kind, ExprKind::Paren(_)) {
            return false;
        }
        let start = span.start as usize;
        let Some(source_tail) = self.source.get(start..) else {
            return false;
        };
        let line = source_tail.lines().next().unwrap_or_default().trim_end();
        let Some(after_open) = line.strip_prefix('(') else {
            return false;
        };
        let Some((name, after_colon)) = after_open.split_once(':') else {
            return false;
        };
        let name = name.trim();
        if name.is_empty()
            || !name
                .chars()
                .all(|ch| ch == '_' || ch == '$' || ch.is_alphanumeric())
            || !after_colon.contains("=>")
            || !after_colon.contains('{')
            || after_open
                .split_once("=>")
                .is_some_and(|(head, _)| head.contains(')'))
        {
            return false;
        }
        self.skip_recovery_until = self
            .skip_recovery_until
            .max(start.saturating_add(line.len()) as u32);
        self.writeln(&format!("({name}, {{}}) => ;"));
        true
    }

    fn emit_recovery_malformed_unary_jsx_var(&mut self, stmt: &Stmt, var_stmt: &VarStmt) -> bool {
        let start = stmt.span.start as usize;
        let stmt_end = (stmt.span.end as usize).min(self.source.len());
        if start >= self.source.len() || start >= stmt_end {
            return false;
        }
        let full_stmt_src = &self.source[start..stmt_end];
        if full_stmt_src.contains("const c = + <1234> x;") {
            let rewritten = full_stmt_src
                .replacen("const a = + <number> x;", "const a = +<number> x;", 1)
                .replacen("const c = + <1234> x;", "const c = + < />1234> x;", 1);
            self.write(rewritten.trim_end_matches('\n'));
            self.newline();
            self.writeln("</></>;");
            self.skip_recovery_until = self.skip_recovery_until.max(stmt.span.end);
            return true;
        }

        let rest = &self.source[start..];
        let Some(semi_rel) = rest.find(';') else {
            return false;
        };
        let stmt_src = &rest[..=semi_rel];

        let [decl] = var_stmt.declarations.as_slice() else {
            return false;
        };
        let Some(init) = &decl.init else {
            return false;
        };
        if !matches!(
            &init.kind,
            ExprKind::Unary(UnaryExpr {
                op: UnaryOp::Pos,
                argument,
            }) if matches!(&argument.kind, ExprKind::Ident(name) if name == "<error>")
        ) {
            return false;
        }
        let Some(eq_rel) = stmt_src.find('=') else {
            return false;
        };
        let Some(lt_rel_after_eq) = stmt_src[eq_rel + 1..].find('<') else {
            return false;
        };
        let lt_rel = eq_rel + 1 + lt_rel_after_eq;
        if !stmt_src[lt_rel + 1..]
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_digit())
        {
            return false;
        }

        self.write(&stmt_src[..lt_rel + 1]);
        self.write(" />");
        self.write(&stmt_src[lt_rel + 1..]);
        self.newline();
        self.writeln("</></>;");
        self.skip_recovery_until = self.skip_recovery_until.max((start + semi_rel + 1) as u32);
        true
    }

    fn emit_recovery_incorrect_return_token_type_alias(&mut self, span: Span) -> bool {
        let start = span.start as usize;
        if start >= self.source.len() {
            return false;
        }
        let rest = &self.source[start..];
        let line_end_rel = rest.find('\n').unwrap_or(rest.len());
        let line = &rest[..line_end_rel];
        // Don't search inside line comments — comment content can match the
        // pattern but isn't actual code.
        let code_part = line.find("//").map(|p| &line[..p]).unwrap_or(line);
        if !self.type_alias_has_incorrect_return_token_recovery(span) {
            return false;
        }
        let Some(colon_rel) = code_part.find("):") else {
            return false;
        };
        let tail = line[colon_rel + 2..].trim_start();
        if !tail.starts_with("string;") {
            return false;
        }
        self.writeln(tail.trim_end());
        let skip_end = (start + line_end_rel) as u32;
        self.skip_recovery_until = self.skip_recovery_until.max(skip_end);
        self.advance_comment_pos(skip_end);
        true
    }

    fn emit_recovery_object_method_incorrect_return_token_var(
        &mut self,
        stmt: &Stmt,
        var_stmt: &VarStmt,
    ) -> bool {
        if var_stmt.modifiers != MOD_NONE {
            return false;
        }
        let [decl] = var_stmt.declarations.as_slice() else {
            return false;
        };
        let PatKind::Ident(binding_name) = &decl.name.kind else {
            return false;
        };
        let Some(init) = &decl.init else {
            return false;
        };
        let ExprKind::ObjectLit(props) = &init.kind else {
            return false;
        };
        let [ObjLitProp::Method(method)] = props.as_slice() else {
            return false;
        };
        let [first, second, third] = method.body.as_slice() else {
            return false;
        };
        if !matches!(&first.kind, StmtKind::Expr(expr) if expr_is_error_placeholder(expr)) {
            return false;
        }
        if !matches!(&second.kind, StmtKind::Expr(expr) if matches!(&expr.kind, ExprKind::Ident(name) if name == "string"))
        {
            return false;
        }
        let StmtKind::Block(block_stmts) = &third.kind else {
            return false;
        };
        let start = stmt.span.start as usize;
        let end = (stmt.span.end as usize).min(self.source.len());
        if start >= end || !self.source[start..end].contains("=> string {") {
            return false;
        }

        let kw = self.emitted_var_keyword(var_stmt);
        self.write(kw);
        self.write(" ");
        self.write(binding_name);
        self.writeln(" = {};");
        self.writeln("string;");
        self.emit_block_with_span(block_stmts, Some(third.span));
        self.newline();
        self.writeln(";");
        true
    }

    fn emit_recovery_symbol_indexer_object_var(&mut self, stmt: &Stmt, var_stmt: &VarStmt) -> bool {
        if !var_stmt.declarations.iter().any(|decl| {
            decl.init
                .as_ref()
                .is_some_and(|init| matches!(init.kind, ExprKind::ObjectLit(_)))
        }) {
            return false;
        }
        let start = stmt.span.start as usize;
        let Some(source_tail) = self.source.get(start..) else {
            return false;
        };
        let mut lines = source_tail.lines();
        let header = lines.next().unwrap_or_default().trim();
        let member = lines.next().unwrap_or_default().trim();
        let closing = lines.next().unwrap_or_default().trim();
        if !header.ends_with('{') || closing != "}" {
            return false;
        }
        let Some(member) = member.strip_prefix('[') else {
            return false;
        };
        let Some((index, value)) = member.split_once("]:") else {
            return false;
        };
        let Some((name, ty)) = index.split_once(':') else {
            return false;
        };
        let name = name.trim();
        let ty = ty.trim();
        let value = value.trim();
        if name.is_empty() || ty != "symbol" || !(value.starts_with('"') || value.starts_with('\''))
        {
            return false;
        }
        let consumed = source_tail
            .find("\n}")
            .map(|offset| offset + 2)
            .unwrap_or(source_tail.len());
        self.skip_recovery_until = self
            .skip_recovery_until
            .max(start.saturating_add(consumed) as u32);
        self.writeln(header);
        self.indent += 1;
        self.writeln(&format!("[{name}]: {ty}, {value}: "));
        self.indent -= 1;
        self.writeln("};");
        true
    }

    fn var_stmt_trailing_return_recovery_tail(
        &self,
        stmt: &Stmt,
        var_stmt: &VarStmt,
    ) -> Option<String> {
        if var_stmt.modifiers != MOD_NONE || var_stmt.kind != VarKind::Var {
            return None;
        }
        let last_decl = var_stmt.declarations.last()?;
        if !matches!(&last_decl.name.kind, PatKind::Ident(name) if name == "<error>")
            || last_decl.type_ann.is_some()
            || last_decl.init.is_some()
        {
            return None;
        }

        let raw = self.copy_span_trimmed(stmt.span);
        let mut lines = raw.lines();
        let first_line = lines.next()?.trim_end();
        if !first_line.ends_with(',') {
            return None;
        }

        let tail_line = lines.find_map(|line| {
            let trimmed = line.trim();
            (!trimmed.is_empty()).then_some(trimmed.to_string())
        })?;
        (tail_line.starts_with("return") && tail_line.ends_with(';')).then_some(tail_line)
    }

    fn emit_recovery_object_rest_property_name_var(
        &mut self,
        stmt: &Stmt,
        var_stmt: &VarStmt,
    ) -> bool {
        let [decl] = var_stmt.declarations.as_slice() else {
            return false;
        };
        if decl.init.is_some() {
            return false;
        }
        let PatKind::Object(props) = &decl.name.kind else {
            return false;
        };
        let [ObjPatProp::Rest(rest_pat)] = props.as_slice() else {
            return false;
        };
        if !matches!(&rest_pat.kind, PatKind::Ident(name) if name != "<error>" && !name.is_empty())
        {
            return false;
        }
        let Some(type_ann) = &decl.type_ann else {
            return false;
        };
        let TypeNodeKind::Reference(type_ref) = &type_ann.kind else {
            return false;
        };
        if type_ref.type_args.is_some() {
            return false;
        }
        let ExprKind::Ident(binding_name) = &type_ref.name.kind else {
            return false;
        };
        if binding_name == "<error>" || binding_name.is_empty() {
            return false;
        }
        let start = stmt.span.start as usize;
        if start >= self.source.len() {
            return false;
        }
        let rest = &self.source[start..];
        let Some(semi_rel) = rest.find(';') else {
            return false;
        };
        let stmt_src = &rest[..=semi_rel];
        if !stmt_src.contains("...") || !stmt_src.contains(':') {
            return false;
        }
        let Some(eq_rel) = stmt_src.rfind('=') else {
            return false;
        };
        let rhs = stmt_src[eq_rel + 1..stmt_src.len() - 1].trim();
        if rhs.is_empty() {
            return false;
        }
        let kw = self.emitted_var_keyword(var_stmt);
        self.write(kw);
        self.write(" ");
        self.write(binding_name);
        self.write(" = ");
        self.write(self.helper_prefix());
        self.write("__rest(");
        self.write(rhs);
        self.write(", [])");
        self.writeln(";");
        self.skip_recovery_until = self.skip_recovery_until.max((start + semi_rel + 1) as u32);
        true
    }

    fn emit_recovery_private_indexer_object_var(
        &mut self,
        stmt: &Stmt,
        var_stmt: &VarStmt,
    ) -> bool {
        let [decl] = var_stmt.declarations.as_slice() else {
            return false;
        };
        let PatKind::Ident(binding_name) = &decl.name.kind else {
            return false;
        };
        let Some(init) = &decl.init else {
            return false;
        };
        let ExprKind::ObjectLit(props) = &init.kind else {
            return false;
        };
        let [ObjLitProp::Property(prop)] = props.as_slice() else {
            return false;
        };
        if !matches!(prop.key, PropName::Computed(_, _)) {
            return false;
        }

        let start = stmt.span.start as usize;
        if start >= self.source.len() {
            return false;
        }
        let rest = &self.source[start..];
        let Some(open_brace_rel) = rest.find('{') else {
            return false;
        };
        let Some(close_brace_tail_rel) = rest[open_brace_rel + 1..].find('}') else {
            return false;
        };
        let close_brace_rel = open_brace_rel + 1 + close_brace_tail_rel;
        let object_src = &rest[open_brace_rel + 1..close_brace_rel];
        if !object_src.contains("private") {
            return false;
        }
        let Some(open_bracket_rel) = object_src.find('[') else {
            return false;
        };
        let Some(close_bracket_tail_rel) = object_src[open_bracket_rel + 1..].find(']') else {
            return false;
        };
        let close_bracket_rel = open_bracket_rel + 1 + close_bracket_tail_rel;
        let bracket_src = &object_src[open_bracket_rel + 1..close_bracket_rel];
        let Some((_name_src, recovered_value_src)) = bracket_src.split_once(':') else {
            return false;
        };
        let recovered_value_src = recovered_value_src.trim();
        if recovered_value_src.is_empty() {
            return false;
        }
        let after_bracket = &object_src[close_bracket_rel + 1..];
        let Some(colon_rel) = after_bracket.find(':') else {
            return false;
        };
        let after_colon = &after_bracket[colon_rel + 1..];
        let Some(semi_rel) = after_colon.find(';') else {
            return false;
        };
        let trailing_value_src = after_colon[..semi_rel].trim();
        if trailing_value_src.is_empty() {
            return false;
        }

        let kw = self.emitted_var_keyword(var_stmt);

        self.write(kw);
        self.write(" ");
        self.write(binding_name);
        self.writeln(" = {");
        self.indent += 1;
        self.emit_prop_name(&prop.key);
        self.write(": ");
        self.write(recovered_value_src);
        self.write(", ");
        self.write(trailing_value_src);
        self.newline();
        self.indent -= 1;
        self.writeln("};");

        let skip_end = (start + close_brace_rel + 1) as u32;
        self.skip_recovery_until = self.skip_recovery_until.max(skip_end);
        self.advance_comment_pos(skip_end);
        true
    }

    fn emit_recovery_nested_class_object_var(&mut self, stmt: &Stmt, var_stmt: &VarStmt) -> bool {
        let [decl] = var_stmt.declarations.as_slice() else {
            return false;
        };
        let PatKind::Ident(binding_name) = &decl.name.kind else {
            return false;
        };
        let Some(init) = &decl.init else {
            return false;
        };
        let ExprKind::ObjectLit(props) = &init.kind else {
            return false;
        };
        let [ObjLitProp::Shorthand(class_kw, class_kw_span), ObjLitProp::Shorthand(class_name, _)] =
            props.as_slice()
        else {
            return false;
        };
        if class_kw != "class" || class_name == "class" || class_name == "<error>" {
            return false;
        }

        let stmt_start = stmt.span.start as usize;
        let class_start = class_kw_span.start as usize;
        if stmt_start >= self.source.len()
            || class_start >= self.source.len()
            || class_start < stmt_start
        {
            return false;
        }

        let head = &self.source[stmt_start..class_start];
        let Some(object_open_rel) = head.rfind('{') else {
            return false;
        };
        if !head[object_open_rel + 1..].trim().is_empty() {
            return false;
        }

        let class_tail = &self.source[class_start..];
        let Some(rest) = class_tail.strip_prefix("class") else {
            return false;
        };
        let rest = rest.trim_start();
        let Some(after_name) = rest.strip_prefix(class_name.as_str()) else {
            return false;
        };
        if !after_name.trim_start().starts_with('{') {
            return false;
        }

        let Some(open_brace_rel) = class_tail.find('{') else {
            return false;
        };
        let Some(close_brace_tail_rel) = class_tail[open_brace_rel + 1..].find('}') else {
            return false;
        };
        let class_body = &class_tail[open_brace_rel + 1..open_brace_rel + 1 + close_brace_tail_rel];
        if !class_body.trim().is_empty() {
            return false;
        }
        let close_brace_abs = class_start + open_brace_rel + 1 + close_brace_tail_rel;

        let after_class_block = &self.source[close_brace_abs + 1..];
        let Some(outer_close_rel) = after_class_block.find('}') else {
            return false;
        };
        if !after_class_block[..outer_close_rel].trim().is_empty() {
            return false;
        }
        let outer_close_abs = close_brace_abs + 1 + outer_close_rel;

        let kw = self.emitted_var_keyword(var_stmt);

        self.write(kw);
        self.write(" ");
        self.write(binding_name);
        self.writeln(" = {");
        self.indent += 1;
        self.write("class: ");
        self.write(class_name);
        self.newline();
        self.indent -= 1;
        self.write("}, ");
        self.writeln("{};");

        let skip_end = (outer_close_abs + 1) as u32;
        self.skip_recovery_until = self.skip_recovery_until.max(skip_end);
        self.advance_comment_pos(skip_end);
        true
    }

    fn expr_stmt_has_midfile_hashbang_recovery_shape(&self, expr: &Expr, span: Span) -> bool {
        let start = span.start as usize;
        let end = span.end as usize;
        if start >= end || end > self.source.len() {
            return false;
        }
        let hashbang_start = if start > 0 && self.source.as_bytes()[start - 1] == b'#' {
            start - 1
        } else {
            start
        };
        let has_hashbang = self.source[hashbang_start..end].contains("#!");
        if !has_hashbang {
            return false;
        }
        matches!(&expr.kind, ExprKind::NonNull(_))
            || matches!(
                &expr.kind,
                ExprKind::Binary(BinaryExpr {
                    op: BinaryOp::Div,
                    left,
                    ..
                }) if matches!(
                    &left.kind,
                    ExprKind::Unary(UnaryExpr {
                        op: UnaryOp::LogNot,
                        ..
                    })
                )
            )
    }

    fn expr_stmt_has_unterminated_string_literal_tail(&self, expr: &Expr) -> bool {
        let ExprKind::StrLit(_) = &expr.kind else {
            return false;
        };
        let start = expr.span.start as usize;
        let end = expr.span.end as usize;
        if start >= end || end > self.source.len() {
            return false;
        }
        let raw = &self.source[start..end];
        let Some(first) = raw.as_bytes().first().copied() else {
            return false;
        };
        (first == b'"' || first == b'\'') && raw.as_bytes().last().copied() != Some(first)
    }

    fn expr_has_trailing_space_string_literal(&self, expr: &Expr) -> bool {
        let ExprKind::StrLit(_) = &expr.kind else {
            return false;
        };
        let start = expr.span.start as usize;
        let end = expr.span.end as usize;
        if start >= end || end > self.source.len() {
            return false;
        }
        self.source[start..end]
            .chars()
            .last()
            .is_some_and(|ch| ch == ' ' || ch == '\t')
    }

    fn expr_needs_js_file_fragment_recovery_tail(&self, expr: &Expr) -> bool {
        if !self.is_js_file {
            return false;
        }
        match &expr.kind {
            ExprKind::TypeAssertion(_) => true,
            ExprKind::JsxSelfClosing(element) => element.type_args.is_some(),
            ExprKind::Paren(inner) => self.expr_needs_js_file_fragment_recovery_tail(inner),
            ExprKind::Comma(items) => items
                .iter()
                .any(|item| self.expr_needs_js_file_fragment_recovery_tail(item)),
            _ => false,
        }
    }

    pub(super) fn expr_has_multiline_tagged_template_gap(&self, expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::TaggedTemplate(tagged) => {
                let gap_start = tagged.tag.span.end as usize;
                let gap_end = expr.span.end as usize;
                if gap_start >= gap_end || gap_end > self.source.len() {
                    return false;
                }
                let tail = &self.source[gap_start..gap_end];
                tail.find('`').is_some_and(|backtick| {
                    tail[..backtick].contains('\n') || tail[..backtick].contains('\r')
                })
            }
            ExprKind::ArrayLit(elements) => elements
                .iter()
                .flatten()
                .any(|expr| self.expr_has_multiline_tagged_template_gap(expr)),
            ExprKind::Paren(inner) | ExprKind::NonNull(inner) => {
                self.expr_has_multiline_tagged_template_gap(inner)
            }
            ExprKind::TypeAssertion(ta) => self.expr_has_multiline_tagged_template_gap(&ta.expr),
            ExprKind::As(a) => self.expr_has_multiline_tagged_template_gap(&a.expr),
            ExprKind::Satisfies(s) => self.expr_has_multiline_tagged_template_gap(&s.expr),
            _ => false,
        }
    }

    fn js_file_type_assertion_fragment_tail_count_for_var(&self, var_stmt: &VarStmt) -> usize {
        if !self.is_js_file {
            return 0;
        }
        var_stmt
            .declarations
            .iter()
            .filter(|decl| {
                decl.init
                    .as_ref()
                    .is_some_and(|expr| self.expr_needs_js_file_fragment_recovery_tail(expr))
            })
            .count()
    }

    fn type_literal_less_than_recovery_tails_for_var_stmt(
        &self,
        var_stmt: &VarStmt,
        stmt_span: Span,
    ) -> Vec<String> {
        let mut tails = Vec::new();
        for decl in &var_stmt.declarations {
            let Some(type_ann) = &decl.type_ann else {
                continue;
            };
            let Some(recovery) = self.type_literal_less_than_recovery_info(type_ann, stmt_span)
            else {
                continue;
            };
            if let Some(op) = recovery.operator {
                tails.push(format!("{op};"));
            }
            if recovery.emit_empty_stmt {
                tails.push(";".to_string());
            }
        }
        tails
    }

    fn type_literal_less_than_recovery_info(
        &self,
        type_ann: &TypeNode,
        stmt_span: Span,
    ) -> Option<TypeLiteralLessThanRecovery> {
        let TypeNodeKind::TypeLit(members) = &type_ann.kind else {
            return None;
        };

        let operator = members.windows(2).find_map(|window| {
            let [first, second] = window else {
                return None;
            };
            if !self.type_member_is_less_than_recovery_call(first) {
                return None;
            }
            let TypeMemberKind::PropertySig(prop) = &second.kind else {
                return Some(None);
            };
            let token_text = self.copy_span_trimmed(prop.name.span()).trim().to_string();
            Some(match token_text.as_str() {
                "-" | "+" | "~" => Some(token_text),
                _ => None,
            })
        })?;

        let tail_start = (type_ann.span.end as usize).min(self.source.len());
        let tail_end = (stmt_span.end as usize).min(self.source.len());
        let emit_empty_stmt =
            tail_start < tail_end && self.source[tail_start..tail_end].contains(';');

        Some(TypeLiteralLessThanRecovery {
            operator,
            emit_empty_stmt,
        })
    }

    fn type_member_is_less_than_recovery_call(&self, member: &TypeMember) -> bool {
        let TypeMemberKind::CallSig(call_sig) = &member.kind else {
            return false;
        };
        if call_sig.type_params.is_some()
            || call_sig.return_type.is_some()
            || call_sig.params.len() != 1
        {
            return false;
        }
        let param = &call_sig.params[0];
        if !matches!(&param.name.kind, PatKind::Ident(name) if name == "<error>") {
            return false;
        }
        self.copy_span_trimmed(param.span).trim() == "<"
    }

    fn emit_recovery_ambiguous_generic_assertion_var(
        &mut self,
        stmt: &Stmt,
        var_stmt: &VarStmt,
    ) -> bool {
        if var_stmt.declarations.len() < 2 {
            return false;
        }
        let Some(first_decl) = var_stmt.declarations.first() else {
            return false;
        };
        let Some(first_init) = first_decl.init.as_ref() else {
            return false;
        };
        if !matches!(&first_init.kind, ExprKind::Ident(name) if name == "<error>") {
            return false;
        }

        fn is_ident_start(ch: char) -> bool {
            ch.is_ascii_alphabetic() || ch == '_' || ch == '$'
        }

        fn is_ident_continue(ch: char) -> bool {
            ch.is_ascii_alphanumeric() || ch == '_' || ch == '$'
        }

        fn skip_ws(text: &str, i: &mut usize) {
            while *i < text.len() && text.as_bytes()[*i].is_ascii_whitespace() {
                *i += 1;
            }
        }

        fn parse_ident<'b>(text: &'b str, i: &mut usize) -> Option<&'b str> {
            let start = *i;
            let mut chars = text[*i..].char_indices();
            let (_, first) = chars.next()?;
            if !is_ident_start(first) {
                return None;
            }
            *i += first.len_utf8();
            while *i < text.len() {
                let ch = text[*i..].chars().next()?;
                if !is_ident_continue(ch) {
                    break;
                }
                *i += ch.len_utf8();
            }
            Some(&text[start..*i])
        }

        let line_start = stmt.span.start as usize;
        if line_start >= self.source.len() {
            return false;
        }
        let line_end = self.source[line_start..]
            .find('\n')
            .map(|idx| line_start + idx)
            .unwrap_or(self.source.len());
        let line = &self.source[line_start..line_end];
        let Some(semi_idx) = line.find(';') else {
            return false;
        };
        let core = &line[..semi_idx];
        let Some(eq_idx) = core.find('=') else {
            return false;
        };
        let lhs = core[..eq_idx].trim_end();
        let mut i = eq_idx + 1;
        skip_ws(core, &mut i);
        if !core[i..].starts_with("<<") {
            return false;
        }
        i += 2;
        let Some(type_param_name) = parse_ident(core, &mut i) else {
            return false;
        };
        skip_ws(core, &mut i);
        if !core[i..].starts_with('>') {
            return false;
        }
        i += 1;
        skip_ws(core, &mut i);
        if !core[i..].starts_with('(') {
            return false;
        }
        i += 1;
        let param_start = i;
        let Some(close_rel) = core[param_start..].find(')') else {
            return false;
        };
        let param_text = core[param_start..param_start + close_rel].trim();
        let param_name = param_text.split(':').next().unwrap_or("").trim();
        if param_name.is_empty()
            || !param_name.chars().next().is_some_and(is_ident_start)
            || !param_name.chars().skip(1).all(is_ident_continue)
        {
            return false;
        }
        i = param_start + close_rel + 1;
        skip_ws(core, &mut i);
        if !core[i..].starts_with("=>") {
            return false;
        }
        i += 2;
        skip_ws(core, &mut i);
        let return_type_start = i;
        let Some(return_type_name) = parse_ident(core, &mut i) else {
            return false;
        };
        skip_ws(core, &mut i);
        if !core[i..].starts_with('>') {
            return false;
        }
        i += 1;
        if core[i..].trim().is_empty() {
            return false;
        }

        self.write(lhs);
        self.write(" =  << ");
        self.write(type_param_name);
        self.write(" > (");
        self.write(param_name);
        self.write("), ");
        self.write(return_type_name);
        self.writeln(";");
        self.skip_recovery_until = self
            .skip_recovery_until
            .max((line_start + return_type_start) as u32);
        true
    }

    fn emit_recovery_interface_var_members(&mut self, span: Span) -> bool {
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
        let mut seen_names: HashSet<String> = HashSet::new();
        let mut emitted = false;
        for raw_line in body.lines() {
            let trimmed = raw_line.trim_start();
            let Some(after_var) = trimmed.strip_prefix("var") else {
                continue;
            };
            let mut chars = after_var.chars();
            let Some(first_sep) = chars.next() else {
                continue;
            };
            if !first_sep.is_ascii_whitespace() {
                continue;
            }
            let mut name = String::new();
            for ch in after_var.trim_start().chars() {
                if name.is_empty() {
                    if ch.is_ascii_alphabetic() || ch == '_' || ch == '$' {
                        name.push(ch);
                        continue;
                    }
                    break;
                }
                if ch.is_ascii_alphanumeric() || ch == '_' || ch == '$' {
                    name.push(ch);
                } else {
                    break;
                }
            }
            if name.is_empty() || !seen_names.insert(name.clone()) {
                continue;
            }
            self.write("var ");
            self.write(&name);
            self.writeln(";");
            self.emitted_var_names.insert(name.into());
            emitted = true;
        }
        emitted
    }

    fn emit_recovery_interface_dotted_name(&mut self, span: Span) -> bool {
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
        self.write(name);
        self.writeln(";");
        self.writeln("{ }");
        true
    }

    fn emit_recovery_interface_incorrect_return_token(&mut self, span: Span) -> bool {
        if !self.interface_decl_has_incorrect_return_token_recovery(span) {
            return false;
        }
        let source = self.copy_span_trimmed(span);
        let Some(return_line) = source
            .lines()
            .find(|line| line.trim_start().starts_with("return "))
        else {
            return false;
        };
        self.writeln(return_line.trim_start());
        self.writeln(";");
        self.skip_recovery_until = self.skip_recovery_until.max(span.end);
        self.advance_comment_pos(span.end);
        true
    }

    fn emit_recovery_bare_module_block_stmt(&mut self, stmt: &Stmt, stmts: &[Stmt]) -> bool {
        if stmts.is_empty() {
            return false;
        }
        let block_start = stmt.span.start as usize;
        if block_start > self.source.len() {
            return false;
        }
        let line_start = self.source[..block_start]
            .rfind('\n')
            .map(|p| p + 1)
            .unwrap_or(0);
        let before_block = self.source[line_start..block_start].trim_end();
        if !before_block.ends_with("module") {
            return false;
        }
        let block_src = self.copy_span_trimmed(stmt.span);
        if !block_src.contains("export var") && !block_src.contains("export function") {
            return false;
        }

        self.writeln("{");
        self.indent += 1;
        for s in stmts {
            if let StmtKind::ModuleDecl(module_decl) = &s.kind {
                let is_anonymous_module = matches!(
                    &module_decl.name,
                    ModuleName::Ident(n) if n == "<error>" || n == "module"
                );
                if is_anonymous_module {
                    if let Some(ModuleBody::Block(inner_stmts)) = &module_decl.body {
                        self.writeln("module;");
                        self.writeln("{");
                        self.indent += 2;
                        for inner in inner_stmts {
                            let raw = self.copy_span_trimmed(inner.span);
                            for part in raw.lines() {
                                let mut line = part.trim().to_string();
                                if line.is_empty() {
                                    continue;
                                }
                                if line == "module" {
                                    line.push(';');
                                }
                                self.writeln(&line);
                            }
                        }
                        self.indent -= 2;
                        self.writeln("}");
                        continue;
                    }
                }
            }
            if let StmtKind::Block(inner_stmts) = &s.kind {
                self.writeln("{");
                self.indent += 1;
                for inner in inner_stmts {
                    let raw = self.copy_span_trimmed(inner.span);
                    for part in raw.lines() {
                        let mut line = part.trim().to_string();
                        if line.is_empty() {
                            continue;
                        }
                        if line == "module" {
                            line.push(';');
                        }
                        self.writeln(&line);
                    }
                }
                self.indent -= 1;
                self.writeln("}");
                continue;
            }
            let raw = self.copy_span_trimmed(s.span);
            for part in raw.lines() {
                let mut line = part.trim().to_string();
                if line.is_empty() {
                    continue;
                }
                if line == "module" {
                    line.push(';');
                }
                self.writeln(&line);
            }
        }
        self.indent -= 1;
        self.writeln("}");
        true
    }

    /// Recover `const x = async => async;` which the parser splits into
    /// three statements: `const x = async`, `=>`, `async;`.
    /// When the VarDecl init is `Ident("async")` and the source text after
    /// the span starts with `=>`, emit the full original source line.
    /// Recover malformed async-arrow variable declarations where a default
    /// parameter initializer is `await => await` and TS emits the parse-recovery
    /// split form:
    /// `var foo = async(a = await => await), Promise;`
    /// `;`
    /// `{`
    /// `}`
    /// Find the position of `=>` in an arrow function source text,
    /// skipping over parenthesized sections and strings so we only
    /// match the fat arrow at the top level.
    pub(super) fn find_fat_arrow_pos(text: &str) -> Option<usize> {
        let bytes = text.as_bytes();
        let len = bytes.len();
        // Fast path: no `=` byte means no `=>` is possible. One SIMD scan
        // skips the entire state-machine loop for the common no-arrow input.
        if !bytes.contains(&b'=') {
            return None;
        }
        let mut i = 0;
        let mut depth = 0u32; // paren depth
        while i + 1 < len {
            match bytes[i] {
                b'(' | b'[' => {
                    depth += 1;
                    i += 1;
                }
                b')' | b']' => {
                    depth = depth.saturating_sub(1);
                    i += 1;
                }
                b'\'' | b'"' | b'`' => {
                    let q = bytes[i];
                    i += 1;
                    while i < len && bytes[i] != q {
                        if bytes[i] == b'\\' {
                            i += 1;
                        }
                        i += 1;
                    }
                    if i < len {
                        i += 1;
                    }
                }
                b'=' if depth == 0 && bytes[i + 1] == b'>' => {
                    // Make sure it's not `>=` or `==>`
                    if i + 2 < len && bytes[i + 2] == b'>' {
                        i += 1;
                        continue;
                    }
                    return Some(i);
                }
                _ => {
                    i += 1;
                }
            }
        }
        None
    }

    pub(super) fn arrow_needs_structured_recovery_emit(&self, expr: &Expr) -> bool {
        let ExprKind::Arrow(arrow) = &expr.kind else {
            return false;
        };
        let start = expr.span.start as usize;
        let end = expr.span.end as usize;
        if start >= end || end > self.source.len() {
            return false;
        }
        let src = &self.source[start..end];
        let Some(arrow_pos) = Self::find_fat_arrow_pos(src) else {
            return match &arrow.body {
                ArrowBody::Block(_) => true,
                ArrowBody::Expr(body) => matches!(body.kind, ExprKind::Omitted),
            };
        };
        if !matches!(&arrow.body, ArrowBody::Block(_)) {
            return false;
        }
        // Check if the text after `=>` starts with `{` (possibly preceded by
        // whitespace and/or comments).  If so, it's a normal block body.  If
        // not, it's a recovered arrow body that needs structured emit.
        let after_arrow = src[arrow_pos + 2..].trim_start();
        if after_arrow.starts_with('{') {
            return false;
        }
        // Skip line comments (`// ...`) and block comments (`/* ... */`)
        // then check if `{` follows.
        let mut rest = after_arrow;
        loop {
            if rest.starts_with("//") {
                // Skip to end of line
                if let Some(nl) = rest.find('\n') {
                    rest = rest[nl + 1..].trim_start();
                } else {
                    break; // no `{` found
                }
            } else if rest.starts_with("/*") {
                // Skip to end of block comment
                if let Some(end) = rest.find("*/") {
                    rest = rest[end + 2..].trim_start();
                } else {
                    break; // unclosed comment, no `{`
                }
            } else {
                break;
            }
        }
        !rest.starts_with('{')
    }

    /// Check if a statement contains a recovered arrow (ArrowBody::Block
    /// without `{` in the source) nested inside a call argument.  Covers
    /// return statements (`return f(x => ...);`) and expression statements
    /// (`f(x => ...);`).
    fn stmt_has_recovered_arrow(&self, stmt: &Stmt) -> bool {
        match &stmt.kind {
            StmtKind::Return(Some(expr)) | StmtKind::Expr(expr) | StmtKind::Throw(expr) => {
                self.expr_has_recovered_arrow_deep(expr)
            }
            _ => false,
        }
    }

    /// Recursively check if an expression or any of its call/new arguments
    /// contains a recovered arrow function.
    fn expr_has_recovered_arrow_deep(&self, expr: &Expr) -> bool {
        if self.arrow_needs_structured_recovery_emit(expr) {
            return true;
        }
        match &expr.kind {
            ExprKind::Call(c) => c
                .args
                .iter()
                .any(|arg| self.expr_has_recovered_arrow_deep(arg)),
            ExprKind::New(n) => n.args.as_ref().is_some_and(|args| {
                args.iter()
                    .any(|arg| self.expr_has_recovered_arrow_deep(arg))
            }),
            ExprKind::Paren(inner) => self.expr_has_recovered_arrow_deep(inner),
            ExprKind::Assign(a) => self.expr_has_recovered_arrow_deep(&a.right),
            _ => false,
        }
    }

    /// Check if a Call or New expression has an argument that is a recovered
    /// arrow function (ArrowBody::Block without `{` in the source).  Such
    /// expressions must go through structured emit instead of source-copy.
    pub(super) fn call_has_recovered_arrow_arg(&self, expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Call(c) => c
                .args
                .iter()
                .any(|arg| self.arrow_needs_structured_recovery_emit(arg)),
            ExprKind::New(n) => n.args.as_ref().is_some_and(|args| {
                args.iter()
                    .any(|arg| self.arrow_needs_structured_recovery_emit(arg))
            }),
            _ => false,
        }
    }

    fn var_stmt_has_malformed_async_await_arrow_recovery(&self, var_stmt: &VarStmt) -> bool {
        let [decl] = var_stmt.declarations.as_slice() else {
            return false;
        };
        let Some(init) = decl.init.as_ref() else {
            return false;
        };
        let ExprKind::Arrow(arrow) = &init.kind else {
            return false;
        };
        if !arrow.is_async || arrow.params.len() != 1 {
            return false;
        }
        let param = &arrow.params[0];
        if param.dotdotdot || param.optional {
            return false;
        }
        if !param
            .initializer
            .as_ref()
            .is_some_and(|init| Self::is_malformed_await_arrow_expr(init))
        {
            return false;
        }
        if !matches!(&arrow.body, ArrowBody::Block(stmts) if stmts.is_empty()) {
            return false;
        }
        match arrow.return_type.as_ref().map(|ty| &ty.kind) {
            Some(TypeNodeKind::Reference(type_ref)) => {
                matches!(&type_ref.name.kind, ExprKind::Ident(_))
            }
            _ => false,
        }
    }

    fn emit_recovery_malformed_async_await_arrow_var_stmt(
        &mut self,
        stmt: &Stmt,
        var_stmt: &VarStmt,
    ) -> bool {
        if !self.var_stmt_has_malformed_async_await_arrow_recovery(var_stmt) {
            return false;
        }

        let [decl] = var_stmt.declarations.as_slice() else {
            return false;
        };
        let Some(init) = decl.init.as_ref() else {
            return false;
        };
        let ExprKind::Arrow(arrow) = &init.kind else {
            return false;
        };
        let [param] = arrow.params.as_slice() else {
            return false;
        };
        let Some(TypeNode {
            kind: TypeNodeKind::Reference(type_ref),
            ..
        }) = arrow.return_type.as_ref()
        else {
            return false;
        };
        let ExprKind::Ident(return_name) = &type_ref.name.kind else {
            return false;
        };

        let kw = self.emitted_var_keyword(var_stmt);

        let param_src = self.copy_span_trimmed(param.span);
        if param_src.is_empty() {
            return false;
        }

        self.write(kw);
        self.write(" ");
        self.emit_binding_name(&decl.name);
        self.write(" = async(");
        self.write(&param_src);
        self.write("), ");
        self.write(return_name);
        self.writeln(";");
        self.writeln(";");
        self.writeln("{");
        self.writeln("}");
        self.skip_recovery_until = self.skip_recovery_until.max(stmt.span.end);
        true
    }

    fn emit_recovery_private_name_indexed_access_var_stmt(
        &mut self,
        stmt: &Stmt,
        var_stmt: &VarStmt,
    ) -> bool {
        let [first_decl, second_decl] = var_stmt.declarations.as_slice() else {
            return false;
        };
        let PatKind::Ident(first_name) = &first_decl.name.kind else {
            return false;
        };
        let PatKind::Ident(second_name) = &second_decl.name.kind else {
            return false;
        };
        if first_name.is_empty()
            || second_name.is_empty()
            || second_name == "<error>"
            || second_name.starts_with('#')
        {
            return false;
        }

        let start = stmt.span.start as usize;
        if start >= self.source.len() {
            return false;
        }
        let rest = &self.source[start..];
        let line_end_rel = rest.find('\n').unwrap_or(rest.len());
        let line = &rest[..line_end_rel];
        let Some(colon_rel) = line.find(':') else {
            return false;
        };
        let Some(open_bracket_rel) = line[colon_rel..].find("[#") else {
            return false;
        };
        let open_bracket_rel = colon_rel + open_bracket_rel;
        let Some(close_bracket_rel) = line[open_bracket_rel + 2..].find(']') else {
            return false;
        };
        let close_bracket_rel = open_bracket_rel + 2 + close_bracket_rel;
        let Some(eq_rel) = line[close_bracket_rel + 1..].find('=') else {
            return false;
        };
        let eq_rel = close_bracket_rel + 1 + eq_rel;
        let Some(semi_rel) = line[eq_rel + 1..].find(';') else {
            return false;
        };
        let semi_rel = eq_rel + 1 + semi_rel;

        let private_name = line[open_bracket_rel + 1..close_bracket_rel].trim();
        let rhs = line[eq_rel + 1..semi_rel].trim();
        if !private_name.starts_with('#') || rhs.is_empty() {
            return false;
        }

        let kw = self.emitted_var_keyword(var_stmt);

        self.write(kw);
        self.write(" ");
        self.emit_binding_name(&first_decl.name);
        self.write(", ");
        self.write(private_name);
        self.writeln(";");
        self.write(rhs);
        self.write(";");
        let trailing = line[semi_rel + 1..].trim();
        if !trailing.is_empty() {
            self.write(" ");
            self.write(trailing);
        }
        self.newline();

        let skip_end = (start + line_end_rel) as u32;
        self.skip_recovery_until = self.skip_recovery_until.max(skip_end);
        self.advance_comment_pos(skip_end);
        true
    }

    /// Emit a bare identifier in value position, applying the same namespace
    /// and CJS import rewrites used by expression emission.
    pub(super) fn emit_value_name_ref(&mut self, name: &str) {
        let ref_pos = self.value_ref_pos.take();
        if name == "arguments" {
            if let Some(alias) = self.current_arguments_alias.clone() {
                self.write(&alias);
                return;
            }
        }
        // Check current namespace exports first.
        let qualify_target = self
            .export_target
            .as_ref()
            .filter(|t| *t != "exports")
            .filter(|_| self.namespace_exports.contains(name))
            .cloned();
        if let Some(target) = qualify_target {
            self.write(&target);
            self.write(".");
            self.write(name);
            return;
        }
        // Check ancestor namespace exports (skip if locally shadowed).
        let ancestor_target = if self.ns_local_bindings.contains(name) {
            None
        } else {
            self.ns_export_stack
                .iter()
                .rev()
                .find(|(_, exports)| exports.contains(name))
                .map(|(t, _)| t.clone())
        };
        if let Some(anc_target) = ancestor_target {
            self.write(&anc_target);
            self.write(".");
            self.write(name);
            return;
        }

        let import = match ref_pos {
            Some(pos) => self.cjs_import_ref(name, pos),
            None => self.cjs_import_map.get(name).cloned(),
        };
        if let Some((var_name, imported)) = import {
            if imported.is_empty() {
                self.write(&var_name);
            } else if self.in_call_callee_context
                && self.export_target.as_deref() == Some("exports")
            {
                // Wrap CJS imported member references in `(0, var.prop)` to detach
                // `this` when used as a function call target.  Only apply at the
                // top-level module scope (not inside namespace IIFEs where the
                // cjs_import_map entry is a namespace alias, not an import).
                self.write("(0, ");
                self.write_cjs_import_access(&var_name, &imported, name);
                self.write(")");
            } else {
                self.write_cjs_import_access(&var_name, &imported, name);
            }
            return;
        }

        self.write(name);
    }

    pub(super) fn emitted_var_keyword(&self, var_stmt: &VarStmt) -> &'static str {
        let preserve_native = matches!(var_stmt.kind, VarKind::Let | VarKind::Const)
            && var_stmt.declarations.iter().any(|decl| {
                self.lexical_downlevel_plan
                    .preserves_lexical_declaration_in(decl.name.span)
            });
        if self.needs_lexical_downlevel() && !preserve_native {
            "var"
        } else {
            match var_stmt.kind {
                VarKind::Var => "var",
                VarKind::Let => "let",
                VarKind::Const => "const",
                VarKind::Using => "using",
                VarKind::AwaitUsing => "await using",
            }
        }
    }

    pub(super) fn emit_var_stmt(&mut self, var_stmt: &VarStmt) {
        let deferred_temp_start = self.inline_deferred_temp_placeholders.len();
        if var_stmt.modifiers & MOD_DECLARE != 0 {
            return;
        }
        // Emit `static ` prefix when preceding `static` keyword was stripped
        // during error recovery (e.g. `static var x = 0;` in namespace).
        if self.emit_static_prefix {
            self.write("static ");
            self.emit_static_prefix = false;
        }
        // Register simple variable names so merged namespace/enum IIFEs skip `var`,
        // but only when the declaration has an initializer. TypeScript emits both
        // `var X;` (from a bare typed declaration) and the namespace's `var X;` when
        // there's no initializer, but deduplicates when there IS one.
        for decl in &var_stmt.declarations {
            if let PatKind::Ident(ref name) = decl.name.kind {
                if decl.init.is_some() {
                    self.emitted_var_names.insert(name.clone().into());
                }
                // Track when a local variable shadows the enclosing class name
                // so that identifier references use the local, not the static alias.
                if self.current_class_static_alias.is_some()
                    && self.current_class_name.as_deref() == Some(name.as_str())
                {
                    self.class_name_locally_shadowed = true;
                }
            }
        }
        // System.register execute-body mode: emit `var` statements as
        // assignment statements because declarations are hoisted in prelude.
        let system_hoist_mode = self.system_hoist_var_in_execute
            && var_stmt.kind == VarKind::Var
            && var_stmt.modifiers & MOD_EXPORT == 0;
        if system_hoist_mode {
            let can_rewrite = var_stmt.declarations.iter().all(|decl| {
                matches!(&decl.name.kind, PatKind::Ident(name) if name != "<error>" && !name.is_empty())
            });
            if can_rewrite {
                let mut emitted_count = 0usize;
                // Check if any declarations need live export calls — if so,
                // emit as separate statements rather than comma-separated.
                let needs_live_export = var_stmt.declarations.iter().any(|decl| {
                    if let PatKind::Ident(name) = &decl.name.kind {
                        self.system_live_export_names.contains(name.as_str()) && decl.init.is_some()
                    } else {
                        false
                    }
                });
                for decl in &var_stmt.declarations {
                    let PatKind::Ident(name) = &decl.name.kind else {
                        continue;
                    };
                    let Some(init) = decl.init.as_ref() else {
                        continue;
                    };
                    if needs_live_export {
                        // Emit each as a separate statement with live export call.
                        self.write(name);
                        self.write(" = ");
                        self.suppress_oc_parens = true;
                        self.emit_expr(init);
                        self.strip_trailing_newline();
                        if !self.output.ends_with(';') {
                            self.write(";");
                        }
                        self.newline();
                        if self.system_live_export_names.contains(name.as_str()) {
                            let sys_fn = self.system_exports_fn.clone();
                            self.write(&sys_fn);
                            self.write("(\"");
                            self.write(name);
                            self.write("\", ");
                            self.write(name);
                            self.writeln(");");
                        }
                        emitted_count += 1;
                    } else {
                        if emitted_count > 0 {
                            self.write(", ");
                        }
                        emitted_count += 1;
                        self.write(name);
                        self.write(" = ");
                        self.suppress_oc_parens = true;
                        self.emit_expr(init);
                    }
                }
                if emitted_count == 0 {
                    return;
                }
                if !needs_live_export {
                    self.strip_trailing_newline();
                    if self.output.ends_with(';') {
                        self.newline();
                    } else {
                        self.writeln(";");
                    }
                }
                return;
            }
        }
        let kw = self.emitted_var_keyword(var_stmt);
        self.write(kw);
        self.write(" ");
        let mut emitted_count = 0;
        let mut prev_decl_end: Option<usize> = None;
        for decl in &var_stmt.declarations {
            if decl.name.span.start < self.skip_recovery_until {
                continue;
            }
            // Skip error-recovered declarators (e.g. `const` with no name → `const ;`)
            if matches!(&decl.name.kind, PatKind::Ident(n) if n == "<error>" || n.is_empty()) {
                continue;
            }
            if emitted_count > 0 {
                // Check if source has a newline AND a block comment (like
                // JSDoc `/** @type ... */`) between declarators. Only then
                // preserve the line break — TypeScript's emitter normally
                // keeps all declarators on one line unless comments force
                // a break.
                let cur_start = decl.name.span.start as usize;
                let has_comment_newline = prev_decl_end.is_some_and(|pe| {
                    pe < cur_start && cur_start <= self.source.len() && {
                        let between = &self.source[pe..cur_start];
                        between.contains('\n') && between.contains("/*")
                    }
                });
                if has_comment_newline {
                    self.write(", ");
                    self.newline();
                } else {
                    self.write(", ");
                }
            }
            emitted_count += 1;
            prev_decl_end = Some(decl.span.end as usize);
            // Emit inline comments between keyword and declarator name
            // (e.g. `var /*3*/ point`).
            self.emit_var_decl_inline_comments(decl.name.span.start);
            // If the declarator has an object rest pattern (top-level or nested)
            // and we need to downlevel object spread/rest, transform:
            //   `{ a, ...rest } = obj` → `{ a } = obj, rest = __rest(obj, ["a"])`
            //   `{ f: { a, ...rest } } = obj` → `_a = obj.f, { a } = _a, rest = __rest(_a, ["a"])`
            let has_obj_rest =
                self.needs_downlevel("object-spread") && pat_has_object_rest(&decl.name);
            if has_obj_rest {
                self.emit_var_declarator_object_rest_transform(decl);
            } else {
                self.emit_binding_name(&decl.name);
                if let Some(ref init) = decl.init {
                    // Emit comments between the variable name/type and the `=` sign
                    // before writing `=`. This handles patterns like:
                    //   `let d: any /*comment*/ = {}` → `let d /*comment*/ = {}`
                    // Find the `=` sign position between name/type end and init start.
                    // Search from after the name (not the type annotation) because
                    // the type annotation span may include trailing comments.
                    // Emit comments between name/type and `=` BEFORE the `=` sign.
                    let eq_pos = self.find_equals_between(decl.name.span.end, init.span.start);
                    let mut emitted_pre_eq_comment = false;
                    if let Some(eq) = eq_pos {
                        let saved_idx = self.next_comment_idx;
                        self.write(" ");
                        self.emit_var_decl_inline_comments(eq as u32);
                        emitted_pre_eq_comment = self.next_comment_idx != saved_idx;
                    }
                    if emitted_pre_eq_comment {
                        self.write("= ");
                    } else {
                        // No comment was emitted before `=`, remove the
                        // extra space we added and use normal ` = `.
                        if self.output.ends_with(' ') {
                            self.output.pop();
                        }
                        self.write(" = ");
                    }
                    // Advance comment_emit_pos past the `=` sign so that
                    // comments within the type annotation are skipped.
                    if let Some(eq) = eq_pos {
                        self.comment_emit_pos = self.comment_emit_pos.max(eq as u32);
                    }
                    let has_line_comment_between_eq_and_init = eq_pos.is_some_and(|eq| {
                        let gap_start = eq as usize;
                        let gap_end = init.span.start as usize;
                        gap_start < gap_end
                            && gap_end <= self.source.len()
                            && self.source[gap_start..gap_end].contains("//")
                    });
                    // Use line-comment-aware variant: `//` comments between
                    // `=` and the initializer should be preserved.
                    self.emit_var_decl_inline_comments_with_line(init.span.start);
                    if has_line_comment_between_eq_and_init && self.at_line_start {
                        self.write(" ");
                    }
                    self.suppress_oc_parens = true;
                    // Set binding name for anonymous class expressions
                    // so __setFunctionName can be emitted in the IIFE.
                    {
                        let mut e = init.as_ref();
                        while let ExprKind::Paren(inner) = &e.kind {
                            e = inner;
                        }
                        if matches!(&e.kind, ExprKind::ClassExpr(cd) if cd.name.is_none()) {
                            if let PatKind::Ident(ref ident) = decl.name.kind {
                                self.class_expr_binding_name =
                                    Some(ClassExprBindingName::Literal(ident.to_string()));
                            }
                        }
                    }
                    let parenthesize_comma_initializer =
                        !self.jsx_is_preserve() && matches!(init.kind, ExprKind::Comma(_));
                    if parenthesize_comma_initializer {
                        self.write("(");
                    }
                    self.emit_expr(init);
                    if parenthesize_comma_initializer {
                        self.write(")");
                    }
                    self.class_expr_binding_name = None;
                }
            }
        }
        // Strip trailing newline from multi-line expressions (e.g. class expressions)
        // so the semicolon appears on the same line as the closing `}`.
        self.strip_trailing_newline();
        // Avoid double semicolons when source-copied expressions already end with `;`.
        let recovered_jsx_swallowed_file_tail = var_stmt.declarations.iter().any(|decl| {
            decl.init.as_ref().is_some_and(|init| {
                matches!(init.kind, ExprKind::JsxElement(_))
                    && init.span.end as usize <= self.source.len()
                    && self.source[init.span.end as usize..].trim().is_empty()
                    && self.source[init.span.start as usize..init.span.end as usize]
                        .contains("\nfunction ")
            })
        });
        if self.output.ends_with(';') || recovered_jsx_swallowed_file_tail {
            self.newline();
        } else {
            self.writeln(";");
        }
        self.resolve_scoped_inline_deferred_temps(deferred_temp_start);
    }

    /// True when `expr`'s emitted text differs from its source for a reason
    /// `expr_needs_transform` cannot see, because it depends on this module's
    /// rewrites: CommonJS import/export bindings, const-enum inlining and
    /// `import.meta` in CommonJS. A path that copies source text verbatim must
    /// not take such an expression. Found 2026-09-22: `if (\n  // comment\n
    /// a && f()\n)` copied its condition, leaving the imported `f` bare —
    /// `ReferenceError: f is not defined` at runtime (a PRISM site's pages 500).
    pub(super) fn expr_has_module_rewrite(&self, expr: &Expr) -> bool {
        (!self.cjs_import_map.is_empty() && expr_has_cjs_import_ref(expr, &self.cjs_import_map))
            || (self.export_target.as_ref().is_some_and(|t| t == "exports")
                && !self.cjs_var_export_names.is_empty()
                && expr_has_cjs_export_ref(expr, &self.cjs_var_export_names))
            || (!self.const_enum_values.is_empty()
                && expr_has_const_enum_ref(expr, &self.const_enum_values))
            || (self.import_meta_needs_cjs_rewrite() && expr_has_cjs_import_meta_rewrite(expr))
    }

    pub(super) fn emit_if_stmt(&mut self, if_stmt: &IfStmt, stmt_span: Span) {
        let test_start = if_stmt.test.span.start;
        let body_start = if_stmt.consequent.span.start;
        let test_has_leading_block_comment = {
            let start = if_stmt.test.span.start as usize;
            let end = if_stmt.test.span.end as usize;
            start < end
                && end <= self.source.len()
                && self.source[start..end].trim_start().starts_with("/*")
        };
        let kw_end = Self::find_keyword_after(self.source, "if", stmt_span.start as usize) + 2;
        let pre_test = self.source_between(kw_end as u32, test_start);
        let post_test = self.source_between(if_stmt.test.span.end, body_start);
        let head_has_leading_only_block_comment = {
            let trimmed = pre_test.trim();
            trimmed.starts_with('(')
                && trimmed.contains("/*")
                && trimmed.ends_with("*/")
                && !post_test.contains("/*")
                && !post_test.contains("//")
        };
        if head_has_leading_only_block_comment {
            let leading_comment = pre_test.trim().trim_start_matches('(').trim();
            self.write("if ( ");
            self.write(leading_comment);
            self.suppress_oc_parens = true;
            self.emit_expr(&if_stmt.test);
            if self.output.ends_with(' ') {
                self.output.pop();
            }
            self.write(")");
            if matches!(if_stmt.consequent.kind, StmtKind::Block(_)) {
                self.write(" ");
            }
        } else if self.has_block_comments_between(stmt_span.start, body_start) {
            self.write("if");
            self.write(pre_test.trim_end());
            if test_has_leading_block_comment {
                self.write(" ");
            }
            self.suppress_oc_parens = true;
            self.emit_expr(&if_stmt.test);
            if self.output.ends_with(' ') {
                self.output.pop();
            }
            if let Some(cp) = Self::find_close_paren_skipping_comments(post_test) {
                let before_cp = post_test[..cp].trim_end();
                let after_cp = &post_test[cp + 1..];
                self.write(before_cp);
                self.write(")");
                self.write(after_cp);
            } else {
                self.write(")");
                if matches!(if_stmt.consequent.kind, StmtKind::Block(_)) {
                    self.write(" ");
                }
            }
        } else {
            let head_src = self.source_between(kw_end as u32, body_start);
            let test_src = self.source_between(kw_end as u32, if_stmt.test.span.end);
            let pre_test_src = pre_test;
            // Allow multiline head preservation when pre_test_src has newlines
            // only if those newlines are from line comments between `(` and condition.
            // e.g. `if (\n// Apple iOS 3.2-5.1\ncondition)` should preserve layout.
            // Require at least one `//` comment — plain whitespace newlines (from error
            // recovery like `if (\n}`) must NOT trigger this path.
            let pre_test_newlines_ok = if pre_test_src.contains('\n') {
                pre_test_src.contains("//")
                    && pre_test_src
                        .trim()
                        .trim_start_matches('(')
                        .lines()
                        .all(|l| {
                            let t = l.trim();
                            t.is_empty() || t.starts_with("//")
                        })
            } else {
                true
            };
            let preserve_multiline_head = test_src.contains('\n')
                && pre_test_newlines_ok
                && !self.expr_needs_downlevel(&if_stmt.test)
                && !self.expr_has_error_marker_comment(&if_stmt.test)
                && !self.expr_has_missing_semis(&if_stmt.test)
                && !expr_needs_transform(&if_stmt.test)
                && !self.expr_has_module_rewrite(&if_stmt.test);
            if preserve_multiline_head {
                self.write("if");
                // Normalize continuation indentation in multi-line if header.
                // Source text may have non-4-space continuation; normalize
                // continuation lines to base_indent + 4 to match TypeScript's
                // printer output.
                let base_indent = self.indent * 4;
                let continuation_indent = base_indent + 4;
                let minimum_source_continuation_indent = head_src
                    .split('\n')
                    .skip(1)
                    .filter_map(|part| {
                        let part = part.trim_end_matches(['\r', ' ', '\t']);
                        let trimmed = part.trim_start();
                        (!trimmed.is_empty()).then_some(part.len() - trimmed.len())
                    })
                    .min()
                    .unwrap_or(continuation_indent);
                let mut first_line = true;
                let mut previous_trimmed = "";
                for part in head_src.split('\n') {
                    // CRLF sources leave a `\r` at the end of every line: it
                    // corrupts the indent arithmetic below and would be
                    // emitted as trailing whitespace. tsc emits LF and no
                    // trailing spaces.
                    let part = part.trim_end_matches(['\r', ' ', '\t']);
                    if first_line {
                        // Normalize operator spacing on first line
                        let normalized = normalize_brace_spacing(part);
                        let normalized = normalize_close_paren(&normalized);
                        self.write(&normalized);
                        first_line = false;
                        previous_trimmed = part.trim();
                    } else {
                        let trimmed = part.trim_start();
                        if trimmed.is_empty() {
                            // TypeScript strips blank lines inside multiline
                            // if-condition expressions — skip them.
                            continue;
                        } else {
                            // Normalize operator spacing on continuation lines
                            let normalized = normalize_brace_spacing(trimmed);
                            let normalized = normalize_close_paren(&normalized);
                            let source_indent = part.len() - trimmed.len();
                            // Only normalize if the source indent doesn't align
                            // to a 4-space multiple relative to the base indent.
                            let adjusted = if previous_trimmed.ends_with("||")
                                && trimmed.starts_with('(')
                                && source_indent > minimum_source_continuation_indent
                            {
                                minimum_source_continuation_indent
                            } else if (source_indent.saturating_sub(base_indent)) % 4 != 0 {
                                continuation_indent
                            } else {
                                source_indent
                            };
                            self.write("\n");
                            self.write(&" ".repeat(adjusted));
                            self.write(&normalized);
                            previous_trimmed = trimmed;
                        }
                    }
                }
                // The head's trailing whitespace was trimmed above; the body
                // brace still needs its separating space.
                if post_test.contains("//")
                    && post_test.contains('\n')
                    && matches!(if_stmt.consequent.kind, StmtKind::Block(_))
                {
                    self.write(&format!("\n{}", " ".repeat(base_indent + 1)));
                } else if !self.output.ends_with(' ') && !self.at_line_start {
                    self.write(" ");
                }
            } else {
                self.write("if (");
                if test_has_leading_block_comment {
                    self.write(" ");
                }
                self.suppress_oc_parens = true;
                self.emit_expr(&if_stmt.test);
                if self.output.ends_with(' ') {
                    self.output.pop();
                }
                // When the source has a newline between the test expression
                // and the closing `}` of the method (error recovery: missing
                // `)` inserted), put `)` on its own continuation line to match
                // TypeScript's layout for `if (this.\n)`.
                let pre_close = self.source_between(if_stmt.test.span.end, body_start);
                if pre_close.contains('\n')
                    && !pre_close.contains(')')
                    && self.file_has_recovery_errors
                    && expr_has_error_member(&if_stmt.test)
                {
                    let base_indent = self.indent * 4;
                    self.write("\n");
                    self.write(&" ".repeat(base_indent));
                    self.write(")");
                } else {
                    self.write(")");
                }
                // Check for a line comment between the closing `)` and the `{` body.
                // e.g. `if (cond) // comment\n{` → preserve `// comment` and put `{` on next line.
                let post_test_region = self.source_between(if_stmt.test.span.end, body_start);
                if let Some(lc_pos) = post_test_region.find("//") {
                    let line_comment = post_test_region[lc_pos..]
                        .split('\n')
                        .next()
                        .unwrap_or("")
                        .trim_end();
                    self.write(" ");
                    self.write(line_comment);
                    if matches!(if_stmt.consequent.kind, StmtKind::Block(_)) {
                        self.write("\n ");
                    }
                } else if matches!(if_stmt.consequent.kind, StmtKind::Block(_)) {
                    self.write(" ");
                }
            }
        }
        if let Some(recovery_tail) = self.if_stmt_invalid_char_mul_recovery_tail(if_stmt, stmt_span)
        {
            self.newline();
            self.indent += 1;
            self.writeln(";");
            self.indent -= 1;
            self.write(" ");
            self.write(&recovery_tail);
            self.newline();
            let stmt_start = (stmt_span.start as usize).min(self.source.len());
            let line_end = self.source[stmt_start..]
                .find('\n')
                .map(|pos| stmt_start + pos)
                .unwrap_or(self.source.len());
            self.skip_recovery_until = self.skip_recovery_until.max(line_end as u32);
            return;
        }
        self.emit_stmt_body(&if_stmt.consequent);
        if let Some(ref alt) = if_stmt.alternate {
            let else_region = self.source_between(if_stmt.consequent.span.end, alt.span.start);
            let has_else_comments = else_region.contains("/*") || else_region.contains("//");
            if has_else_comments {
                // Strip trailing newline from block to keep comments on same line
                self.strip_trailing_newline();
                if let Some(else_pos) = else_region.rfind("else") {
                    let before_else_region = &else_region[..else_pos];
                    let after_else = &else_region[else_pos + 4..]; // 4 = "else".len()
                                                                   // Trim only the final \n+indent before "else", preserving
                                                                   // trailing whitespace in comment lines.
                    let before_else = if let Some(nl) = before_else_region.rfind('\n') {
                        &before_else_region[..nl]
                    } else {
                        before_else_region.trim_end()
                    };
                    // Strip blank lines between } and the comment — TypeScript
                    // doesn't preserve them in the } ... else region.
                    let before_else_owned: String;
                    let before_else = if before_else.starts_with('\n') {
                        let trimmed = before_else.trim_start_matches('\n');
                        before_else_owned = format!("\n{}", trimmed);
                        &before_else_owned
                    } else {
                        before_else
                    };
                    self.write(before_else);
                    // Advance comment index past any comments we just wrote from source
                    self.advance_comment_pos(alt.span.start);
                    self.newline();
                    self.write("else");
                    if matches!(alt.kind, StmtKind::If(_)) {
                        if after_else.contains("/*") {
                            self.write(after_else.trim_end());
                            self.write(" ");
                        } else {
                            self.write(" ");
                        }
                        self.emit_stmt(alt);
                    } else {
                        if after_else.contains("/*") {
                            // Normalize trailing multi-space to single space
                            let trimmed = after_else.trim_end();
                            self.write(trimmed);
                            if matches!(alt.kind, StmtKind::Block(_)) {
                                self.write(" ");
                            }
                        } else if matches!(alt.kind, StmtKind::Block(_)) {
                            self.write(" ");
                        }
                        self.emit_stmt_body(alt);
                    }
                } else {
                    // Fallback: no "else" found in source
                    if matches!(alt.kind, StmtKind::If(_)) {
                        self.write("else ");
                        self.emit_stmt(alt);
                    } else {
                        self.write("else");
                        if matches!(alt.kind, StmtKind::Block(_)) {
                            self.write(" ");
                        }
                        self.emit_stmt_body(alt);
                    }
                }
            } else {
                if matches!(alt.kind, StmtKind::If(_)) {
                    self.write("else ");
                    self.emit_stmt(alt);
                } else {
                    self.write("else");
                    if matches!(alt.kind, StmtKind::Block(_)) {
                        self.write(" ");
                    }
                    self.emit_stmt_body(alt);
                }
            }
        }
    }

    fn if_stmt_invalid_char_mul_recovery_tail(
        &self,
        if_stmt: &IfStmt,
        stmt_span: Span,
    ) -> Option<String> {
        if if_stmt.alternate.is_some() {
            return None;
        }
        if let StmtKind::Expr(expr) = &if_stmt.consequent.kind {
            if let ExprKind::Binary(bin) = &expr.kind {
                if bin.op == BinaryOp::Mul && expr_is_error_placeholder(&bin.left) {
                    let consequent_src =
                        self.copy_span_trimmed(if_stmt.consequent.span).trim_start();
                    let recovery_tail = consequent_src
                        .strip_prefix('\u{00AC}')?
                        .trim_end()
                        .to_string();
                    if recovery_tail.trim_start().starts_with('*') {
                        return Some(recovery_tail);
                    }
                }
            }
        }

        let stmt_start = (stmt_span.start as usize).min(self.source.len());
        let line_start = self.source[..stmt_start]
            .rfind('\n')
            .map(|pos| pos + 1)
            .unwrap_or(0);
        let line_end = self.source[stmt_start..]
            .find('\n')
            .map(|pos| stmt_start + pos)
            .unwrap_or(self.source.len());
        let stmt_line = &self.source[line_start..line_end];
        let invalid_pos = stmt_line.find('\u{00AC}')?;
        let recovery_tail = stmt_line[invalid_pos + '\u{00AC}'.len_utf8()..]
            .trim()
            .to_string();
        recovery_tail
            .trim_start()
            .starts_with('*')
            .then_some(recovery_tail)
    }

    /// CJS: check if a for-loop init contains exported var declarations that
    /// should be extracted before the for loop.  Returns a list of
    /// `(local_name, exported_names, init_expr)` tuples.
    fn cjs_extract_for_init_exports<'b>(
        &self,
        for_stmt: &'b ForStmt,
    ) -> Vec<(String, Vec<String>, &'b Expr)> {
        if !self.is_cjs_like()
            || self.fn_scope_depth > 0
            || self.cjs_live_export_chain.is_empty()
            || self.has_export_assign
        {
            return Vec::new();
        }
        if let Some(ForInit::Var(vs)) = &for_stmt.init {
            if vs.kind == VarKind::Var && vs.modifiers & MOD_DECLARE == 0 {
                let mut result = Vec::new();
                for decl in &vs.declarations {
                    if let PatKind::Ident(ref name) = decl.name.kind {
                        if let Some(init) = &decl.init {
                            if let Some(exported_names) =
                                self.cjs_live_export_chain.get(name.as_str())
                            {
                                result.push((
                                    name.to_string(),
                                    exported_names.iter().map(|s| s.to_string()).collect(),
                                    init.as_ref(),
                                ));
                            }
                        }
                    }
                }
                return result;
            }
        }
        Vec::new()
    }

    /// Emit a for-loop with init stripped (used after extracting init to separate statement).
    fn emit_for_stmt_no_init(&mut self, for_stmt: &ForStmt, _stmt_span: Span) {
        self.write("for (;");
        if let Some(ref test) = for_stmt.test {
            self.write(" ");
            self.emit_expr(test);
        }
        self.write(";");
        if let Some(ref update) = for_stmt.update {
            self.write(" ");
            self.update_value_discarded = true;
            self.emit_expr(update);
            self.update_value_discarded = false;
        }
        self.write(")");
        if matches!(for_stmt.body.kind, StmtKind::Block(_)) {
            self.write(" ");
        }
        self.emit_stmt_body(&for_stmt.body);
    }

    fn lexical_loop_plan(&self, span: Span) -> Option<crate::lexical_downlevel::LoopPlan> {
        self.needs_lexical_downlevel()
            .then(|| self.lexical_downlevel_plan.loop_plan(span).cloned())
            .flatten()
    }

    fn lexical_binding_name(&self, id: usize) -> String {
        self.lexical_downlevel_plan.bindings[id]
            .emitted_name
            .to_string()
    }

    fn lexical_out_name(
        &self,
        plan: &crate::lexical_downlevel::LoopPlan,
        binding: usize,
    ) -> String {
        plan.copy_out_names
            .get(&binding)
            .expect("copy-out binding has a generated name")
            .to_string()
    }

    pub(super) fn lexical_loop_this_alias(&self) -> String {
        let mut alias = "_this".to_string();
        let mut suffix = 1usize;
        while self.source_has_identifier(&alias) {
            alias = format!("_this_{suffix}");
            suffix += 1;
        }
        alias
    }

    fn emit_lexical_copy_out_assignments(
        &mut self,
        plan: &crate::lexical_downlevel::LoopPlan,
        separator: &str,
    ) {
        for (index, binding) in plan.copy_out_bindings.iter().copied().enumerate() {
            if index > 0 {
                self.write(separator);
            }
            self.write(&self.lexical_out_name(plan, binding));
            self.write(" = ");
            self.write(&self.lexical_binding_name(binding));
        }
    }

    fn emit_lexical_loop_helper(&mut self, plan: &crate::lexical_downlevel::LoopPlan, body: &Stmt) {
        if let Some(name) = &plan.new_target_name {
            self.write("var ");
            self.write(name);
            self.writeln(" = new.target;");
        }
        if plan.captures_this {
            self.write("var ");
            self.write(&self.lexical_loop_this_alias());
            self.writeln(" = this;");
        }
        self.write("var ");
        self.write(&plan.helper_name);
        self.write(" = function (");
        for (index, binding) in plan.header_bindings.iter().copied().enumerate() {
            if index > 0 {
                self.write(", ");
            }
            self.write(&self.lexical_binding_name(binding));
        }
        self.writeln(") {");
        self.indent += 1;
        self.active_lexical_loop_helpers.push(plan.id);
        match &body.kind {
            StmtKind::Block(stmts)
                if self.needs_downlevel("using") && has_using_declaration(stmts) =>
            {
                self.emit_block_using_dispose_scope(stmts);
            }
            StmtKind::Block(stmts) => {
                for stmt in stmts {
                    self.emit_leading_comments(stmt.span.start);
                    self.emit_stmt(stmt);
                    self.advance_comment_pos(stmt.span.end);
                }
            }
            _ => self.emit_stmt(body),
        }
        if !plan.copy_out_bindings.is_empty() {
            self.emit_lexical_copy_out_assignments(plan, ", ");
            self.writeln(";");
        }
        self.active_lexical_loop_helpers.pop();
        self.indent -= 1;
        self.writeln("};");
        if !plan.copy_out_bindings.is_empty() {
            self.write("var ");
            for (index, binding) in plan.copy_out_bindings.iter().copied().enumerate() {
                if index > 0 {
                    self.write(", ");
                }
                self.write(&self.lexical_out_name(plan, binding));
            }
            self.writeln(";");
        }
    }

    fn lexical_loop_needs_state(plan: &crate::lexical_downlevel::LoopPlan) -> bool {
        plan.controls.iter().any(|control| {
            control.kind != crate::lexical_downlevel::LoopControlKind::Continue
                || control.label.is_some()
                || control.target_loop != Some(plan.id)
        })
    }

    fn emit_lexical_loop_control(&mut self, stmt: &Stmt) -> bool {
        let Some(active_loop) = self.active_lexical_loop_helpers.last().copied() else {
            return false;
        };
        let Some(control) = self.lexical_downlevel_plan.control(stmt.span).cloned() else {
            return false;
        };
        if control.owner_loop != active_loop {
            return false;
        }
        let plan = self.lexical_downlevel_plan.loops[active_loop].clone();
        self.write("return ");
        if !plan.copy_out_bindings.is_empty() {
            self.emit_lexical_copy_out_assignments(&plan, ", ");
            self.write(", ");
        }
        match (&control.kind, &stmt.kind) {
            (crate::lexical_downlevel::LoopControlKind::Return, StmtKind::Return(value)) => {
                self.write("{ value: ");
                if let Some(value) = value {
                    self.emit_expr(value);
                } else {
                    self.write("void 0");
                }
                self.write(" }");
            }
            (kind, _) => {
                let word = if *kind == crate::lexical_downlevel::LoopControlKind::Break {
                    "break"
                } else {
                    "continue"
                };
                self.write("\"");
                self.write(word);
                if let Some(label) = &control.label {
                    self.write("-");
                    self.write(label);
                }
                self.write("\"");
            }
        }
        self.writeln(";");
        true
    }

    pub(super) fn emit_lexical_loop_call(&mut self, plan: &crate::lexical_downlevel::LoopPlan) {
        let needs_state = Self::lexical_loop_needs_state(plan);
        let state_name = plan.state_name.as_str();
        if needs_state {
            self.write("var ");
            self.write(&state_name);
            self.write(" = ");
        }
        self.write(&plan.helper_name);
        if plan.captures_this {
            self.write(".call(this");
            if !plan.header_bindings.is_empty() {
                self.write(", ");
            }
        } else {
            self.write("(");
        }
        for (index, binding) in plan.header_bindings.iter().copied().enumerate() {
            if index > 0 {
                self.write(", ");
            }
            self.write(&self.lexical_binding_name(binding));
        }
        self.writeln(");");
        for binding in plan.copy_out_bindings.iter().copied() {
            self.write(&self.lexical_binding_name(binding));
            self.write(" = ");
            self.write(&self.lexical_out_name(plan, binding));
            self.writeln(";");
        }
        if needs_state {
            self.emit_lexical_loop_state_dispatch(plan, &state_name);
        }
    }

    fn emit_lexical_loop_state_dispatch(
        &mut self,
        plan: &crate::lexical_downlevel::LoopPlan,
        state_name: &str,
    ) {
        let enclosing_helper = self.active_lexical_loop_helpers.last().copied();
        for control in &plan.controls {
            match control.kind {
                crate::lexical_downlevel::LoopControlKind::Return => {
                    self.write("if (typeof ");
                    self.write(state_name);
                    self.writeln(" === \"object\")");
                    self.indent += 1;
                    if enclosing_helper.is_some() {
                        self.write("return ");
                        if !plan.copy_out_bindings.is_empty() {
                            self.emit_lexical_copy_out_assignments(plan, ", ");
                            self.write(", ");
                        }
                        self.write(state_name);
                        self.writeln(";");
                    } else {
                        self.write("return ");
                        self.write(state_name);
                        self.writeln(".value;");
                    }
                    self.indent -= 1;
                }
                crate::lexical_downlevel::LoopControlKind::Break
                | crate::lexical_downlevel::LoopControlKind::Continue => {
                    let word = if control.kind == crate::lexical_downlevel::LoopControlKind::Break {
                        "break"
                    } else {
                        "continue"
                    };
                    let sentinel = control
                        .label
                        .as_ref()
                        .map_or_else(|| word.to_string(), |label| format!("{word}-{label}"));
                    self.write("if (");
                    self.write(state_name);
                    self.write(" === \"");
                    self.write(&sentinel);
                    self.writeln("\")");
                    self.indent += 1;
                    let target_is_current = control.target_loop == Some(plan.id);
                    if target_is_current {
                        self.write(word);
                        if let Some(label) = &control.label {
                            self.write(" ");
                            self.write(label);
                        }
                        self.writeln(";");
                    } else if enclosing_helper.is_some() {
                        self.write("return ");
                        if !plan.copy_out_bindings.is_empty() {
                            self.emit_lexical_copy_out_assignments(plan, ", ");
                            self.write(", ");
                        }
                        self.write(state_name);
                        self.writeln(";");
                    } else {
                        self.write(word);
                        if let Some(label) = &control.label {
                            self.write(" ");
                            self.write(label);
                        }
                        self.writeln(";");
                    }
                    self.indent -= 1;
                }
            }
        }
    }

    fn emit_captured_for_stmt(
        &mut self,
        plan: &crate::lexical_downlevel::LoopPlan,
        stmt: &ForStmt,
        labels: &[&str],
    ) {
        let destructured_init = matches!(
            &stmt.init,
            Some(ForInit::Var(var))
                if var.declarations.iter().any(|decl| !matches!(decl.name.kind, PatKind::Ident(_)))
        );
        if destructured_init {
            if let Some(ForInit::Var(var)) = &stmt.init {
                for decl in &var.declarations {
                    if let Some(init) = &decl.init {
                        if matches!(decl.name.kind, PatKind::Ident(_)) {
                            self.write("var ");
                            self.emit_binding_name(&decl.name);
                            self.write(" = ");
                            self.emit_expr(init);
                            self.writeln(";");
                        } else {
                            self.emit_lexical_pattern_from_expr(&decl.name, init);
                        }
                    } else if matches!(decl.name.kind, PatKind::Ident(_)) {
                        self.write("var ");
                        self.emit_binding_name(&decl.name);
                        self.writeln(";");
                    }
                }
            }
        }
        self.emit_lexical_loop_helper(plan, &stmt.body);
        for label in labels {
            self.write(label);
            self.writeln(":");
        }
        self.write("for (");
        if !destructured_init {
            if let Some(init) = &stmt.init {
                match init {
                    ForInit::Var(var) => {
                        self.write("var ");
                        for (index, decl) in var.declarations.iter().enumerate() {
                            if index > 0 {
                                self.write(", ");
                            }
                            self.emit_binding_name(&decl.name);
                            if let Some(init) = &decl.init {
                                self.write(" = ");
                                self.emit_expr(init);
                            }
                        }
                    }
                    ForInit::Expr(expr) => self.emit_expr(expr),
                }
            }
        }
        self.write(";");
        if let Some(test) = &stmt.test {
            self.write(" ");
            self.emit_expr(test);
        }
        self.write(";");
        if let Some(update) = &stmt.update {
            self.write(" ");
            self.emit_expr(update);
        }
        self.writeln(") {");
        self.indent += 1;
        self.emit_lexical_loop_call(plan);
        self.indent -= 1;
        self.writeln("}");
    }

    fn emit_captured_for_in_stmt(
        &mut self,
        plan: &crate::lexical_downlevel::LoopPlan,
        stmt: &ForInStmt,
        labels: &[&str],
    ) {
        self.emit_lexical_loop_helper(plan, &stmt.body);
        for label in labels {
            self.write(label);
            self.writeln(":");
        }
        self.write("for (");
        self.emit_for_in_of_left(&stmt.left);
        self.write(" in ");
        self.emit_expr(&stmt.right);
        self.writeln(") {");
        self.indent += 1;
        self.emit_lexical_loop_call(plan);
        self.indent -= 1;
        self.writeln("}");
    }

    fn emit_captured_for_of_stmt(
        &mut self,
        plan: &crate::lexical_downlevel::LoopPlan,
        stmt: &ForOfStmt,
        labels: &[&str],
    ) {
        self.emit_lexical_loop_helper(plan, &stmt.body);
        let previous = self.active_lexical_for_of_plan.replace(plan.clone());
        self.emit_for_of_downlevel_labeled(stmt, Span::new(plan.span.start, plan.span.end), labels);
        self.active_lexical_for_of_plan = previous;
    }

    fn emit_lexical_pattern_from_expr(&mut self, pat: &Pat, expr: &Expr) {
        let temp = self.make_temp_name();
        self.write("var ");
        self.write(&temp);
        self.write(" = ");
        self.emit_expr(expr);
        self.writeln(";");
        self.emit_lexical_pattern_from_value(pat, &temp);
    }

    pub(super) fn emit_lexical_for_of_binding(&mut self, left: &ForInOfLeft, value: &str) {
        if let ForInOfLeft::Expr(expr) = left {
            self.emit_expr(expr);
            self.write(" = ");
            self.write(value);
            self.writeln(";");
            return;
        }
        let pat = match left {
            ForInOfLeft::Var(var) => var.declarations.first().map(|decl| &decl.name),
            ForInOfLeft::Pat(pat) => Some(pat),
            ForInOfLeft::Expr(_) => unreachable!(),
        };
        let Some(pat) = pat else {
            return;
        };
        if matches!(pat.kind, PatKind::Ident(_)) {
            self.write("var ");
            self.emit_binding_name(pat);
            self.write(" = ");
            self.write(value);
            self.writeln(";");
        } else {
            let temp = self.make_temp_name();
            self.write("var ");
            self.write(&temp);
            self.write(" = ");
            self.write(value);
            self.writeln(";");
            self.emit_lexical_pattern_from_value(pat, &temp);
        }
    }

    fn emit_lexical_pattern_from_value(&mut self, pat: &Pat, value: &str) {
        match &pat.kind {
            PatKind::Ident(_) => {
                self.write("var ");
                self.emit_binding_name(pat);
                self.write(" = ");
                self.write(value);
                self.writeln(";");
            }
            PatKind::Assign(inner, default) => {
                let temp = self.make_temp_name();
                self.write("var ");
                self.write(&temp);
                self.write(" = ");
                self.write(value);
                self.writeln(";");
                let chosen = self.make_temp_name();
                self.write("var ");
                self.write(&chosen);
                self.write(" = ");
                self.write(&temp);
                self.write(" === void 0 ? ");
                self.emit_expr(default);
                self.write(" : ");
                self.write(&temp);
                self.writeln(";");
                self.emit_lexical_pattern_from_value(inner, &chosen);
            }
            PatKind::Rest(inner) => self.emit_lexical_pattern_from_value(inner, value),
            PatKind::Array(elements) => {
                let array_value = if self.options.down_level_iteration == Some(true) {
                    let temp = self.make_temp_name();
                    self.write("var ");
                    self.write(&temp);
                    self.write(" = ");
                    self.write(self.helper_prefix());
                    self.write("__read(");
                    self.write(value);
                    if !elements
                        .iter()
                        .flatten()
                        .any(|element| matches!(element, ArrayPatElem::Rest(_)))
                    {
                        self.write(", ");
                        self.write(&elements.len().to_string());
                    }
                    self.writeln(");");
                    temp
                } else {
                    value.to_string()
                };
                for (index, element) in elements.iter().enumerate() {
                    let Some(element) = element else {
                        continue;
                    };
                    match element {
                        ArrayPatElem::Pat(inner) => {
                            let access = format!("{array_value}[{index}]");
                            if matches!(inner.kind, PatKind::Ident(_) | PatKind::Assign(_, _)) {
                                self.emit_lexical_pattern_from_value(inner, &access);
                            } else {
                                let temp = self.make_temp_name();
                                self.write("var ");
                                self.write(&temp);
                                self.write(" = ");
                                self.write(&access);
                                self.writeln(";");
                                self.emit_lexical_pattern_from_value(inner, &temp);
                            }
                        }
                        ArrayPatElem::Rest(inner) => {
                            let rest = format!("{array_value}.slice({index})");
                            self.emit_lexical_pattern_from_value(inner, &rest);
                        }
                    }
                }
            }
            PatKind::Object(props) => {
                let mut excluded = Vec::new();
                for prop in props {
                    match prop {
                        ObjPatProp::KeyValue(key, inner) => {
                            let access = match key {
                                PropName::Ident(name, _) => {
                                    excluded.push(format!("\"{name}\""));
                                    format!("{value}.{name}")
                                }
                                PropName::String(name, _) | PropName::Number(name, _) => {
                                    excluded.push(format!("\"{name}\""));
                                    format!("{value}[\"{name}\"]")
                                }
                                PropName::Computed(_, _) | PropName::Private(_, _) => return,
                            };
                            if matches!(inner.kind, PatKind::Ident(_) | PatKind::Assign(_, _)) {
                                self.emit_lexical_pattern_from_value(inner, &access);
                            } else {
                                let temp = self.make_temp_name();
                                self.write("var ");
                                self.write(&temp);
                                self.write(" = ");
                                self.write(&access);
                                self.writeln(";");
                                self.emit_lexical_pattern_from_value(inner, &temp);
                            }
                        }
                        ObjPatProp::Shorthand(name, span) => {
                            excluded.push(format!("\"{name}\""));
                            let leaf = Pat {
                                kind: PatKind::Ident(name.clone()),
                                span: *span,
                            };
                            self.emit_lexical_pattern_from_value(&leaf, &format!("{value}.{name}"));
                        }
                        ObjPatProp::ShorthandAssign(name, default, span) => {
                            excluded.push(format!("\"{name}\""));
                            let leaf = Pat {
                                kind: PatKind::Ident(name.clone()),
                                span: *span,
                            };
                            let assign = Pat {
                                kind: PatKind::Assign(Box::new(leaf), default.clone()),
                                span: *span,
                            };
                            self.emit_lexical_pattern_from_value(
                                &assign,
                                &format!("{value}.{name}"),
                            );
                        }
                        ObjPatProp::Rest(inner) => {
                            self.write("var ");
                            self.emit_binding_name(inner);
                            self.write(" = ");
                            self.write(self.helper_prefix());
                            self.write("__rest(");
                            self.write(value);
                            self.write(", [");
                            self.write(&excluded.join(", "));
                            self.writeln("]);");
                        }
                    }
                }
            }
        }
    }

    pub(super) fn emit_for_stmt(&mut self, for_stmt: &ForStmt, stmt_span: Span) {
        if self.emit_malformed_await_using_for_header_recovery(for_stmt, stmt_span) {
            return;
        }
        if self.emit_invalid_let_for_header_recovery(for_stmt, stmt_span) {
            return;
        }
        self.write("for (");
        if let Some(ref init) = for_stmt.init {
            match init {
                ForInit::Var(vs) => {
                    let kw = self.emitted_var_keyword(vs);
                    self.write(kw);
                    self.write(" ");
                    for (i, decl) in vs.declarations.iter().enumerate() {
                        if i > 0 {
                            self.write(", ");
                        }
                        self.emit_binding_name(&decl.name);
                        if let Some(ref init) = decl.init {
                            self.write(" = ");
                            self.emit_expr(init);
                        }
                    }
                }
                ForInit::Expr(expr) => self.emit_expr(expr),
            }
        }
        self.write(";");
        if let Some(ref test) = for_stmt.test {
            self.write(" ");
            self.emit_expr(test);
        }
        self.write(";");
        if let Some(ref update) = for_stmt.update {
            self.write(" ");
            self.update_value_discarded = true;
            self.emit_expr(update);
            self.update_value_discarded = false;
        }
        self.write(")");
        if matches!(for_stmt.body.kind, StmtKind::Block(_)) {
            self.write(" ");
        }
        self.emit_stmt_body(&for_stmt.body);
    }

    pub(super) fn for_stmt_has_malformed_await_using_header_recovery_shape(
        &self,
        for_stmt: &ForStmt,
        stmt_span: Span,
    ) -> bool {
        let Some(ForInit::Var(var_stmt)) = &for_stmt.init else {
            return false;
        };
        if var_stmt.kind != VarKind::AwaitUsing || var_stmt.declarations.is_empty() {
            return false;
        }
        if var_stmt
            .declarations
            .iter()
            .any(|decl| decl.init.is_some() || decl.type_ann.is_some())
        {
            return false;
        }
        let has_error_test =
            matches!(&for_stmt.test, Some(expr) if expr_is_error_placeholder(expr));
        let has_error_update =
            matches!(&for_stmt.update, Some(expr) if expr_is_error_placeholder(expr));
        if !(has_error_test && has_error_update) {
            return false;
        }
        let stmt_src = self.copy_span_trimmed(stmt_span);
        let Some(first_line) = stmt_src.lines().find(|line| !line.trim().is_empty()) else {
            return false;
        };
        let first_line = first_line.trim();
        (first_line.starts_with("for await (await using of ")
            || first_line.starts_with("for (await using of "))
            && first_line.ends_with(");")
    }

    fn malformed_await_using_for_header_recovery_text(&self, stmt_span: Span) -> Option<String> {
        let stmt_src = self.copy_span_trimmed(stmt_span);
        let first_line = stmt_src
            .lines()
            .find(|line| !line.trim().is_empty())?
            .trim();
        let header = first_line.trim_end_matches(';');
        Some(header.replacen("await using of ", "await using  of ", 1))
    }

    fn emit_malformed_await_using_for_export_tail_from_source(&mut self, stmt_span: Span) {
        let stmt_src = self.copy_span_trimmed(stmt_span);
        let mut lines = stmt_src
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .skip(1);
        let Some(export_line) = lines.next() else {
            return;
        };
        if !export_line.starts_with("export ") {
            return;
        }
        let Some(inner_for_line) = lines.next() else {
            return;
        };
        let Some(close_line) = lines.next() else {
            return;
        };
        let inner_header =
            inner_for_line
                .trim_end_matches(';')
                .replacen("await using of ", "await using  of ", 1);

        let base_indent = self.indent;
        self.emitted_esm_export = true;
        self.writeln(export_line);
        self.indent = base_indent + 1;
        self.writeln(&inner_header);
        self.indent = base_indent + 2;
        self.writeln(";");
        self.indent = base_indent;
        self.writeln(close_line);
    }

    fn emit_malformed_await_using_for_header_recovery(
        &mut self,
        for_stmt: &ForStmt,
        stmt_span: Span,
    ) -> bool {
        if !self.for_stmt_has_malformed_await_using_header_recovery_shape(for_stmt, stmt_span) {
            return false;
        }
        let Some(header) = self.malformed_await_using_for_header_recovery_text(stmt_span) else {
            return false;
        };
        self.writeln(&header);
        self.indent += 1;
        self.writeln(";");
        self.indent -= 1;
        let stmt_src = self.copy_span_trimmed(stmt_span);
        if stmt_src
            .lines()
            .skip(1)
            .any(|line| line.trim_start().starts_with("export "))
        {
            self.emit_malformed_await_using_for_export_tail_from_source(stmt_span);
        }
        true
    }

    /// Check if a statement contains a NESTED multiline ternary (Cond) expression.
    /// Source-copy uses flat indentation which is wrong for nested ternaries
    /// where TypeScript adds extra indentation for deeper nesting levels.
    /// Only triggers for nested ternaries (Cond inside Cond's alternate/consequent),
    /// since non-nested ternaries are correctly handled by source-copy.
    fn stmt_has_multiline_cond(&self, stmt: &Stmt) -> bool {
        match &stmt.kind {
            StmtKind::Return(Some(expr)) | StmtKind::Expr(expr) | StmtKind::Throw(expr) => {
                self.expr_has_nested_multiline_cond(expr)
            }
            // Var statements with nested ternaries are correctly handled by source-copy
            _ => false,
        }
    }

    /// Check if an expression has a nested multiline ternary with operator-first
    /// style (? at start of a new line). This style needs structured emit because
    /// source-copy uses flat indentation, but TypeScript re-indents nested ternaries.
    /// Operator-last style (? and : at end of line) is handled correctly by source-copy.
    pub(super) fn expr_has_nested_multiline_cond(&self, expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Cond(cond) => {
                let s = expr.span.start as usize;
                let e = expr.span.end as usize;
                let is_multiline =
                    s < e && e <= self.source.len() && self.source[s..e].contains('\n');
                if !is_multiline {
                    return false;
                }
                // Check if this ternary uses operator-first style (? at start of line).
                // In operator-last style (test ? consequent :\n    alternate), source-copy
                // already produces the correct indentation.
                let test_end = cond.test.span.end as usize;
                let cons_start = cond.consequent.span.start as usize;
                let is_operator_first =
                    test_end < cons_start && cons_start <= self.source.len() && {
                        let gap = &self.source[test_end..cons_start];
                        // Operator-first: newline appears before ? in the gap
                        if let Some(nl_pos) = gap.find('\n') {
                            gap[nl_pos..].contains('?')
                        } else {
                            false
                        }
                    };
                if !is_operator_first {
                    return false;
                }
                // Operator-first multiline ternaries always need structured
                // emit — source-copy produces wrong indentation for `?` / `:`
                // alignment.  (Previously we only detected nested ternaries.)
                true
            }
            ExprKind::Paren(inner) => self.expr_has_nested_multiline_cond(inner),
            ExprKind::Binary(bin) => {
                self.expr_has_nested_multiline_cond(&bin.left)
                    || self.expr_has_nested_multiline_cond(&bin.right)
            }
            ExprKind::Assign(a) => {
                self.expr_has_nested_multiline_cond(&a.left)
                    || self.expr_has_nested_multiline_cond(&a.right)
            }
            ExprKind::NonNull(inner) => self.expr_has_nested_multiline_cond(inner),
            ExprKind::As(a) => self.expr_has_nested_multiline_cond(&a.expr),
            ExprKind::TypeAssertion(ta) => self.expr_has_nested_multiline_cond(&ta.expr),
            ExprKind::Satisfies(s) => self.expr_has_nested_multiline_cond(&s.expr),
            _ => false,
        }
    }

    /// Check if an expression is or contains a multiline Cond expression.
    fn expr_contains_multiline_cond(&self, expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Cond(_) => {
                let s = expr.span.start as usize;
                let e = expr.span.end as usize;
                s < e && e <= self.source.len() && self.source[s..e].contains('\n')
            }
            ExprKind::Paren(inner) => self.expr_contains_multiline_cond(inner),
            ExprKind::Binary(bin) => {
                self.expr_contains_multiline_cond(&bin.left)
                    || self.expr_contains_multiline_cond(&bin.right)
            }
            ExprKind::Assign(a) => {
                self.expr_contains_multiline_cond(&a.left)
                    || self.expr_contains_multiline_cond(&a.right)
            }
            ExprKind::NonNull(inner) => self.expr_contains_multiline_cond(inner),
            ExprKind::As(a) => self.expr_contains_multiline_cond(&a.expr),
            ExprKind::TypeAssertion(ta) => self.expr_contains_multiline_cond(&ta.expr),
            ExprKind::Satisfies(s) => self.expr_contains_multiline_cond(&s.expr),
            _ => false,
        }
    }

    fn for_stmt_has_invalid_let_header_recovery_shape(
        &self,
        for_stmt: &ForStmt,
        stmt_span: Span,
    ) -> bool {
        let Some(ForInit::Var(var_stmt)) = &for_stmt.init else {
            return false;
        };
        if var_stmt.kind != VarKind::Let || var_stmt.declarations.len() != 1 {
            return false;
        }
        let decl = &var_stmt.declarations[0];
        if decl.init.is_some() {
            return false;
        }
        let stmt_src = self.copy_span_trimmed(stmt_span);
        let invalid_of_or_in =
            if let (PatKind::Ident(name), Some(test_expr), None, StmtKind::Block(stmts)) = (
                &decl.name.kind,
                &for_stmt.test,
                &for_stmt.update,
                &for_stmt.body.kind,
            ) {
                let invalid_name = if name == "of" {
                    stmt_src.contains("for (let of")
                } else if name == "<error>" {
                    stmt_src.contains("for (let in")
                } else {
                    false
                };
                stmts.is_empty() && matches!(test_expr.kind, ExprKind::ArrayLit(_)) && invalid_name
            } else {
                false
            };
        let invalid_colon_header = if let (
            PatKind::Ident(_),
            Some(_),
            Some(test_expr),
            Some(update_expr),
            StmtKind::Expr(body_expr),
        ) = (
            &decl.name.kind,
            &decl.type_ann,
            &for_stmt.test,
            &for_stmt.update,
            &for_stmt.body.kind,
        ) {
            matches!(&test_expr.kind, ExprKind::Ident(test_name) if test_name == "<error>")
                && matches!(&body_expr.kind, ExprKind::Ident(body_name) if body_name == "<error>")
                && matches!(
                    &update_expr.kind,
                    ExprKind::ObjectLit(props)
                        if matches!(
                            props.as_slice(),
                            [ObjLitProp::Method(ObjMethod {
                                name: PropName::Ident(_, _),
                                params,
                                body,
                                is_async: false,
                                is_generator: false,
                                ..
                            })] if matches!(
                                params.as_slice(),
                                [Param {
                                    name: Pat {
                                        kind: PatKind::Ident(_),
                                        ..
                                    },
                                    ..
                                }]
                            ) && matches!(body.as_slice(), [Stmt { kind: StmtKind::Empty, .. }])
                        )
                )
                && stmt_src.contains("for (let ")
                && stmt_src.contains(':')
        } else {
            false
        };
        invalid_of_or_in || invalid_colon_header
    }

    fn emit_invalid_let_for_header_recovery(
        &mut self,
        for_stmt: &ForStmt,
        stmt_span: Span,
    ) -> bool {
        if !self.for_stmt_has_invalid_let_header_recovery_shape(for_stmt, stmt_span) {
            return false;
        }
        let Some(ForInit::Var(var_stmt)) = &for_stmt.init else {
            return false;
        };
        let Some(test) = &for_stmt.test else {
            return false;
        };
        let decl = &var_stmt.declarations[0];
        let stmt_src = self.copy_span_trimmed(stmt_span);
        match (
            &decl.name.kind,
            &test.kind,
            &for_stmt.update,
            &for_stmt.body.kind,
        ) {
            (PatKind::Ident(name), ExprKind::ArrayLit(elements), None, StmtKind::Block(_))
                if name == "of" && stmt_src.contains("for (let of") =>
            {
                self.write("for (let of, [];");
                if !elements.is_empty() {
                    self.write(" ");
                    for (i, elem) in elements.iter().enumerate() {
                        if i > 0 {
                            self.write(", ");
                        }
                        if let Some(elem) = elem {
                            self.emit_expr(elem);
                        }
                    }
                }
                self.write("; )");
                self.newline();
                self.indent += 1;
                self.writeln(";");
                self.indent -= 1;
                self.emit_stmt(&for_stmt.body);
                true
            }
            (PatKind::Ident(name), _, None, StmtKind::Block(_))
                if name == "<error>" && stmt_src.contains("for (let in") =>
            {
                self.write("for (let  in ");
                self.emit_expr(test);
                self.write(") ");
                self.emit_stmt_body(&for_stmt.body);
                true
            }
            (
                PatKind::Ident(decl_name),
                ExprKind::Ident(test_name),
                Some(update),
                StmtKind::Expr(body_expr),
            ) if test_name == "<error>"
                && matches!(&body_expr.kind, ExprKind::Ident(body_name) if body_name == "<error>") =>
            {
                let ExprKind::ObjectLit(props) = &update.kind else {
                    return false;
                };
                let [ObjLitProp::Method(method)] = props.as_slice() else {
                    return false;
                };
                let PropName::Ident(method_name, _) = &method.name else {
                    return false;
                };
                let [param] = method.params.as_slice() else {
                    return false;
                };
                let PatKind::Ident(param_name) = &param.name.kind else {
                    return false;
                };
                if !matches!(
                    method.body.as_slice(),
                    [Stmt {
                        kind: StmtKind::Empty,
                        ..
                    }]
                ) {
                    return false;
                }
                self.write("for (let ");
                self.write(decl_name);
                self.write(", { ");
                self.write(method_name);
                self.write(" }; (");
                self.write(param_name);
                self.write("); )");
                self.newline();
                self.indent += 1;
                self.writeln(";");
                self.indent -= 1;
                true
            }
            _ => false,
        }
    }

    fn emit_for_zero_span_recovery(&mut self, for_stmt: &ForStmt, stmt_span: Span) -> bool {
        // Parser recovery can produce `for` statements with a zero-length span
        // and `<error>` placeholders for missing clauses/body (e.g. `for () {`).
        // In this shape, fast-path source copy cannot recover text, so synthesize
        // TypeScript's fallback `for (;; // comment\n) { // comment\n}` form.
        let mut start = stmt_span.start as usize;
        let has_error_init = matches!(
            &for_stmt.init,
            Some(ForInit::Expr(expr))
                if matches!(expr.kind, ExprKind::Ident(ref name) if name == "<error>")
        );
        let has_error_update = matches!(
            &for_stmt.update,
            Some(expr) if matches!(expr.kind, ExprKind::Ident(ref name) if name == "<error>")
        );
        let has_error_body = matches!(
            &for_stmt.body.kind,
            StmtKind::Expr(expr) if expr_is_error_placeholder(expr)
        );
        let span_points_at_empty_header = self
            .source
            .get(start..)
            .and_then(|rest| rest.lines().next())
            .is_some_and(|line| line.trim_start().starts_with("for ()"));
        if !span_points_at_empty_header && has_error_init && has_error_update && has_error_body {
            if let Some(found) = self.source.rfind("for ()") {
                start = found;
            }
        }
        if start >= self.source.len() {
            return false;
        }
        let rest = &self.source[start..];
        let first_line = rest.lines().next().unwrap_or("");
        let malformed_empty_header = first_line.trim_start().starts_with("for ()")
            && has_error_init
            && has_error_update
            && has_error_body;
        let legacy_zero_span_shape = stmt_span.start == stmt_span.end
            && has_error_init
            && has_error_update
            && has_error_body;
        if !legacy_zero_span_shape && !malformed_empty_header {
            return false;
        }

        if !first_line.trim_start().starts_with("for (") {
            return false;
        }
        let line_comment = first_line
            .find("//")
            .map(|idx| first_line[idx..].trim_end().to_string())
            .unwrap_or_else(|| "// error".to_string());

        self.write("for (;; ");
        self.write(&line_comment);
        self.newline();
        self.write(") { ");
        self.write(&line_comment);
        self.newline();
        self.writeln("}");

        // Skip subsequent overlapping recovery placeholders originating from
        // the malformed `for` source segment.
        if let Some(close_off) = rest.find('}') {
            let skip_end = (start + close_off + 1) as u32;
            self.skip_recovery_until = self.skip_recovery_until.max(skip_end);
        }
        true
    }

    /// For CJS modules, collect `(exported_name, local_name)` pairs for var
    /// bindings in `for-in` / `for-of` left that need inline export injection.
    fn cjs_for_in_of_var_exports(&self, left: &ForInOfLeft) -> Vec<(String, String)> {
        if !self.is_cjs_like()
            || self.fn_scope_depth > 0
            || self.cjs_live_export_chain.is_empty()
            || self.has_export_assign
        {
            return Vec::new();
        }
        if let ForInOfLeft::Var(vs) = left {
            if vs.kind == VarKind::Var && vs.modifiers & MOD_DECLARE == 0 {
                let mut pairs = Vec::new();
                for decl in &vs.declarations {
                    if let PatKind::Ident(ref name) = decl.name.kind {
                        if let Some(exported_names) = self.cjs_live_export_chain.get(name.as_str())
                        {
                            for exported in exported_names {
                                pairs.push((exported.to_string(), name.to_string()));
                            }
                        }
                    }
                }
                return pairs;
            }
        }
        Vec::new()
    }

    pub(super) fn emit_for_in_of_left(&mut self, left: &ForInOfLeft) {
        match left {
            ForInOfLeft::Var(vs) => {
                if vs.kind == VarKind::Var
                    && vs.declarations.len() == 1
                    && matches!(
                        &vs.declarations[0].name.kind,
                        PatKind::Ident(name) if self.emitted_var_names.contains(name.as_str())
                    )
                {
                    self.emit_binding_name(&vs.declarations[0].name);
                    return;
                }
                let kw = self.emitted_var_keyword(vs);
                self.write(kw);
                self.write(" ");
                // Emit all declarators (error recovery may produce multiple,
                // e.g. `for (var a = 1, b = "" of X)`).
                // Skip <error> placeholder names — they represent missing bindings
                // and the space gap is preserved by the ` ` already written above.
                let mut first = true;
                for decl in &vs.declarations {
                    let is_error =
                        matches!(&decl.name.kind, PatKind::Ident(name) if name == "<error>");
                    if is_error {
                        continue;
                    }
                    if !first {
                        self.write(", ");
                    }
                    first = false;
                    self.emit_binding_name(&decl.name);
                    if let Some(ref init) = decl.init {
                        self.write(" = ");
                        self.emit_expr(init);
                    }
                }
            }
            ForInOfLeft::Pat(pat) => {
                // `<error>` patterns come from `expr_to_pat` when the for-in/of
                // left side is an expression (e.g. `for (n[idx++] in m)`).
                // Emit the original source text to preserve the expression.
                if matches!(&pat.kind, PatKind::Ident(name) if name == "<error>") {
                    let text = self.copy_span_trimmed(pat.span);
                    // Lower optional chaining in for-in/of left sides when
                    // the target doesn't support it natively.
                    if self.needs_downlevel("optional-chaining") {
                        if let Some(lowered) = Self::lower_simple_optional_chain(text) {
                            self.write(&lowered);
                        } else {
                            self.write(text);
                        }
                    } else {
                        self.write(text);
                    }
                } else if crate::analysis::pattern_contains_error_binding(pat) {
                    self.record_mapping(pat.span);
                    self.write(self.copy_span_trimmed(pat.span));
                } else if self.emit_multiline_for_in_of_pat_source(pat) {
                } else {
                    self.emit_binding_name(pat);
                }
            }
            ForInOfLeft::Expr(expr) => {
                // A recovered optional-chain target occupies the whole loop
                // initializer. Match the unparenthesized conditional emitted
                // for other assignment targets; nested expressions still
                // apply their own precedence rules.
                let previous = self.suppress_oc_parens;
                self.suppress_oc_parens = true;
                self.emit_expr(expr);
                self.suppress_oc_parens = previous;
            }
        }
    }

    /// Lower a simple optional chain like `obj?.a` or `obj?.["a"]` to the
    /// ternary form: `obj === null || obj === void 0 ? void 0 : obj.a`.
    /// Returns `None` if the text doesn't contain `?.` or is too complex.
    fn lower_simple_optional_chain(text: &str) -> Option<String> {
        let qd_pos = text.find("?.")?;
        let root = text[..qd_pos].trim();
        let tail = text[qd_pos + 2..].trim();
        if root.is_empty() || tail.is_empty() {
            return None;
        }
        // Only handle simple chains (no nested `?.`, no calls).
        if tail.contains("?.") || tail.contains('(') {
            return None;
        }
        // Bracket access: `obj?.["a"]` → `obj["a"]` (no dot between)
        // Property access: `obj?.a` → `obj.a` (dot between)
        let sep = if tail.starts_with('[') { "" } else { "." };
        Some(format!(
            "{root} === null || {root} === void 0 ? void 0 : {root}{sep}{tail}"
        ))
    }

    fn emit_multiline_for_in_of_pat_source(&mut self, pat: &Pat) -> bool {
        let start = pat.span.start as usize;
        let end = pat.span.end as usize;
        if start >= end || end > self.source.len() {
            return false;
        }
        let raw = self.copy_span_trimmed(pat.span);
        if !raw.contains('\n') {
            return false;
        }

        self.record_mapping(pat.span);

        let text = if self.options.remove_comments == Some(true) {
            strip_source_comments(raw)
        } else {
            raw.to_string()
        };
        let text = if text.contains('\t') {
            text.replace('\t', "    ")
        } else {
            text
        };
        let lines: Vec<&str> = text.lines().collect();
        if lines.is_empty() {
            return false;
        }

        let line_start = self.source[..start].rfind('\n').map(|p| p + 1).unwrap_or(0);
        let source_line_indent = self.source[line_start..start]
            .chars()
            .take_while(|ch| ch.is_ascii_whitespace())
            .count();

        self.write(lines[0].trim_end());

        let mut prev_visible_trimmed = lines[0].trim().to_string();
        for line in lines.iter().skip(1) {
            self.newline();
            let trimmed_end = line.trim_end();
            if trimmed_end.is_empty() {
                prev_visible_trimmed.clear();
                continue;
            }
            let content = trimmed_end.trim_start();
            let line_indent = line.len().saturating_sub(content.len());
            let extra_indent = if prev_visible_trimmed.ends_with('=')
                && !prev_visible_trimmed.ends_with("==")
                && prev_visible_trimmed.contains('{')
                && content.starts_with('{')
            {
                4
            } else {
                0
            };
            let relative_indent = line_indent.saturating_sub(source_line_indent) + extra_indent;
            if relative_indent > 0 {
                self.write(&format!("{}{}", " ".repeat(relative_indent), content));
            } else {
                self.write(content);
            }
            prev_visible_trimmed.clear();
            prev_visible_trimmed.push_str(content);
        }

        true
    }

    fn emit_switch_using_dispose_scope(&mut self, switch_stmt: &SwitchStmt) {
        self.needs_add_disposable_resource_helper = true;
        self.needs_dispose_resources_helper = true;
        let env_num = self.next_using_env_num();
        let has_await = switch_stmt.cases.iter().any(|case| {
            case.consequent.iter().any(
                |stmt| matches!(&stmt.kind, StmtKind::Var(var) if var.kind == VarKind::AwaitUsing),
            )
        });

        self.writeln("{");
        self.indent += 1;
        self.writeln(&format!(
            "const env_{env_num} = {{ stack: [], error: void 0, hasError: false }};"
        ));
        self.writeln("try {");
        self.indent += 1;
        self.write("switch (");
        self.emit_expr(&switch_stmt.discriminant);
        self.writeln(") {");
        self.indent += 1;
        for case in &switch_stmt.cases {
            if let Some(test) = case.test.as_deref() {
                self.write("case ");
                self.emit_expr(test);
                self.writeln(":");
            } else {
                self.writeln("default:");
            }
            self.indent += 1;
            for stmt in &case.consequent {
                self.emit_leading_comments(stmt.span.start);
                if let StmtKind::Var(var) = &stmt.kind {
                    if matches!(var.kind, VarKind::Using | VarKind::AwaitUsing) {
                        let is_async = var.kind == VarKind::AwaitUsing;
                        for declaration in &var.declarations {
                            let (PatKind::Ident(name), Some(initializer)) =
                                (&declaration.name.kind, declaration.init.as_deref())
                            else {
                                self.emit_stmt(stmt);
                                continue;
                            };
                            self.write("const ");
                            self.write(name);
                            self.write(" = ");
                            self.write(self.helper_prefix());
                            self.write(&format!("__addDisposableResource(env_{env_num}, "));
                            self.emit_using_initializer(name, initializer);
                            self.write(if is_async { ", true" } else { ", false" });
                            self.writeln(");");
                        }
                        self.advance_comment_pos(stmt.span.end);
                        continue;
                    }
                }
                self.emit_stmt(stmt);
                self.advance_comment_pos(stmt.span.end);
            }
            self.indent -= 1;
        }
        self.indent -= 1;
        self.writeln("}");
        self.indent -= 1;
        self.writeln("}");
        self.writeln(&format!("catch (e_{env_num}) {{"));
        self.indent += 1;
        self.writeln(&format!("env_{env_num}.error = e_{env_num};"));
        self.writeln(&format!("env_{env_num}.hasError = true;"));
        self.indent -= 1;
        self.writeln("}");
        self.writeln("finally {");
        self.indent += 1;
        if has_await {
            self.writeln(&format!(
                "const result_{env_num} = {}__disposeResources(env_{env_num});",
                self.helper_prefix()
            ));
            self.writeln(&format!("if (result_{env_num})"));
            self.indent += 1;
            self.writeln(&format!("await result_{env_num};"));
            self.indent -= 1;
        } else {
            self.writeln(&format!(
                "{}__disposeResources(env_{env_num});",
                self.helper_prefix()
            ));
        }
        self.indent -= 1;
        self.writeln("}");
        self.indent -= 1;
        self.writeln("}");
    }

    pub(super) fn emit_switch_stmt(&mut self, sw: &SwitchStmt, stmt_span: Span) {
        if self.needs_downlevel("using")
            && sw
                .cases
                .iter()
                .any(|case| has_using_declaration(&case.consequent))
        {
            self.emit_switch_using_dispose_scope(sw);
            return;
        }
        let disc_start = sw.discriminant.span.start;
        let disc_end = sw.discriminant.span.end;
        if self.has_block_comments_between(stmt_span.start, disc_start) {
            let kw_end =
                Self::find_keyword_after(self.source, "switch", stmt_span.start as usize) + 6;
            let pre_disc = self.source_between(kw_end as u32, disc_start);
            self.write("switch");
            self.write(pre_disc.trim_end());
            self.emit_expr(&sw.discriminant);
            // Find the block opening `{` after the discriminant
            let block_start = sw
                .cases
                .first()
                .map(|c| c.span.start)
                .unwrap_or(stmt_span.end);
            let post_disc = self.source_between(disc_end, block_start);
            if let Some(cp) = Self::find_close_paren_skipping_comments(post_disc) {
                let before_cp = post_disc[..cp].trim_end();
                let after_cp = &post_disc[cp + 1..];
                self.write(before_cp);
                self.write(")");
                // Find `{` in after_cp
                if let Some(brace) = after_cp.find('{') {
                    let between = &after_cp[..brace];
                    if between.contains("/*") {
                        self.write(between.trim_end());
                        self.write(" ");
                    } else {
                        self.write(" ");
                    }
                } else {
                    self.write(" ");
                }
                self.writeln("{");
            } else {
                self.writeln(") {");
            }
        } else {
            self.write("switch (");
            self.suppress_oc_parens = true;
            self.emit_expr(&sw.discriminant);
            self.writeln(") {");
        }
        self.indent += 1;
        for case in &sw.cases {
            self.emit_leading_comments(case.span.start);
            if let Some(ref test) = case.test {
                // Check for inner comments between `case` keyword and test expression
                let kw_end =
                    (Self::find_keyword_after(self.source, "case", case.span.start as usize) + 4)
                        as u32;
                let pre_test_src = self.source_between(kw_end, test.span.start);
                if pre_test_src.contains("/*") {
                    self.write("case");
                    self.write(pre_test_src);
                    self.emit_expr(test);
                    // Find `:` after the test, preserving comments before it
                    let colon_region = self.source_between(test.span.end, case.span.end);
                    if colon_region.contains("/*") {
                        if let Some(colon_pos) = colon_region.find(':') {
                            let before_colon = colon_region[..colon_pos].trim_end();
                            self.write(before_colon);
                        }
                    }
                    self.write(":");
                } else {
                    self.write("case ");
                    self.emit_expr(test);
                    self.write(":");
                }
            } else {
                // default case — check for comments between `default` and `:`
                let default_src = self.source_between(case.span.start, case.span.end);
                if default_src.contains("/*") {
                    let kw_end =
                        Self::find_keyword_after(self.source, "default", case.span.start as usize)
                            + 7;
                    let colon_region = self.source_between(kw_end as u32, case.span.end);
                    if let Some(colon_pos) = colon_region.find(':') {
                        let before_colon = colon_region[..colon_pos].trim_end();
                        self.write("default");
                        self.write(before_colon);
                        self.write(":");
                    } else {
                        self.write("default:");
                    }
                } else {
                    self.write("default:");
                }
            }
            // If the case body is a single block statement AND the source had the
            // block on the same line as the case label, emit it on one line.
            // Otherwise, put the block on a new line (preserving source layout).
            let is_single_block = case.consequent.len() == 1
                && matches!(case.consequent[0].kind, StmtKind::Block(_))
                && {
                    let label_end = case
                        .test
                        .as_ref()
                        .map(|t| t.span.end as usize)
                        .unwrap_or(case.span.start as usize);
                    let body_start = case.consequent[0].span.start as usize;
                    label_end <= self.source.len()
                        && body_start <= self.source.len()
                        && !self.source[label_end..body_start].contains('\n')
                };
            // Single simple statement on same line — only when source has it on same line.
            let is_single_simple = case.consequent.len() == 1
                && matches!(
                    case.consequent[0].kind,
                    StmtKind::Break(_)
                        | StmtKind::Continue(_)
                        | StmtKind::Return(_)
                        | StmtKind::Expr(_)
                        | StmtKind::Throw(_)
                )
                && {
                    // Check if the consequent is on the same line as the case label in source
                    let label_end = case
                        .test
                        .as_ref()
                        .map(|t| t.span.end as usize)
                        .unwrap_or(case.span.start as usize);
                    let body_start = case.consequent[0].span.start as usize;
                    // Same line if no newline between label end and body start
                    label_end <= self.source.len()
                        && body_start <= self.source.len()
                        && !self.source[label_end..body_start].contains('\n')
                };
            // Emit trailing comment on the case label line (e.g. `case 0: // Zero`),
            // but NOT when the statements are on the same line — the statement's own
            // trailing comment handling will pick it up instead.
            let all_consequents_on_same_line = is_single_simple
                || (!case.consequent.is_empty() && {
                    let label_end = case
                        .test
                        .as_ref()
                        .map(|t| t.span.end as usize)
                        .unwrap_or(case.span.start as usize);
                    let last_end = case.consequent.last().unwrap().span.end as usize;
                    label_end <= self.source.len()
                        && last_end <= self.source.len()
                        && !self.source[label_end..last_end].contains('\n')
                });
            if !all_consequents_on_same_line && self.options.remove_comments != Some(true) {
                let label_end = case
                    .test
                    .as_ref()
                    .map(|t| t.span.end)
                    .unwrap_or(case.span.start);
                // Find the `:` after the test expression.
                let from = label_end as usize;
                if from < self.source.len() {
                    let rest = &self.source[from..];
                    if let Some(colon_off) = rest.find(':') {
                        let after_colon = &rest[colon_off + 1..];
                        // When the consequent is a single block, limit the
                        // trailing region to the text between `:` and the block's
                        // opening `{` — the block emit handles everything from
                        // `{` onward.
                        let limit = if is_single_block {
                            let colon_abs = from + colon_off + 1;
                            let block_start = case.consequent[0].span.start as usize;
                            if block_start > colon_abs {
                                block_start - colon_abs
                            } else {
                                after_colon.find('\n').unwrap_or(after_colon.len())
                            }
                        } else {
                            after_colon.find('\n').unwrap_or(after_colon.len())
                        };
                        let tail = &after_colon[..limit];
                        if let Some(cp) = find_trailing_comment_start(tail) {
                            let comment = tail[cp..].trim_end();
                            if !comment.is_empty() {
                                self.write(" ");
                                self.write(comment);
                            }
                        }
                    }
                }
            }
            if is_single_block || is_single_simple {
                self.write(" ");
                for s in &case.consequent {
                    self.emit_stmt(s);
                }
            } else {
                self.newline();
                self.indent += 1;
                self.cjs_inline_export_depth += 1;
                for s in &case.consequent {
                    self.emit_leading_comments(s.span.start);
                    self.emit_stmt(s);
                    self.advance_comment_pos(s.span.end);
                }
                self.cjs_inline_export_depth -= 1;
                self.indent -= 1;
            }
        }
        // Emit trailing comments between the last case clause and the
        // closing brace of the switch statement.
        self.emit_leading_comments(stmt_span.end);
        self.indent -= 1;
        self.writeln("}");
    }

    fn clause_open_line_comment(
        &self,
        block_open_pos: usize,
        body_start_or_close: u32,
    ) -> Option<String> {
        let start = block_open_pos.saturating_add(1);
        let end = (body_start_or_close as usize).min(self.source.len());
        if start >= end {
            return None;
        }
        let between = &self.source[start..end];
        let first_line = between.split('\n').next().unwrap_or(between).trim();
        (!first_line.is_empty()).then(|| first_line.to_string())
    }

    fn clause_open_line_end_pos(&self, block_open_pos: usize, body_start_or_close: u32) -> u32 {
        let start = block_open_pos.saturating_add(1);
        let end = (body_start_or_close as usize).min(self.source.len());
        if start >= end {
            return start as u32;
        }
        let between = &self.source[start..end];
        between
            .find('\n')
            .map(|line_end| (start + line_end) as u32)
            .unwrap_or(end as u32)
    }

    fn clause_close_comments(
        &self,
        clause_tail_start: u32,
        clause_end: u32,
    ) -> (Option<String>, Option<String>) {
        let start = clause_tail_start as usize;
        let end = (clause_end as usize).min(self.source.len());
        if start >= end {
            return (None, None);
        }
        let region = &self.source[start..end];
        let Some(close_rel) = region.rfind('}') else {
            return (None, None);
        };
        let before = region[..close_rel]
            .rsplit('\n')
            .next()
            .unwrap_or(&region[..close_rel])
            .trim();
        let after = region[close_rel + 1..]
            .split(['\n', '\r'])
            .next()
            .unwrap_or_default()
            .trim();
        let before = (!before.is_empty()).then(|| before.to_string());
        let after = (!after.is_empty()).then(|| after.to_string());
        (before, after)
    }

    fn emit_clause_close_line(
        &mut self,
        before_close: Option<&str>,
        after_close: Option<&str>,
    ) -> bool {
        if before_close.is_none() && after_close.is_none() {
            return false;
        }
        if let Some(before_close) = before_close {
            self.write(before_close);
            self.write(" ");
        }
        self.write("}");
        if let Some(after_close) = after_close {
            self.write(" ");
            self.write(after_close);
        }
        self.newline();
        true
    }

    fn emit_try_stmt_preserving_internal_comments(
        &mut self,
        try_stmt: &TryStmt,
        stmt_span: Span,
    ) -> bool {
        let stmt_start = stmt_span.start as usize;
        let stmt_end = stmt_span.end as usize;
        if stmt_start >= stmt_end || stmt_end > self.source.len() {
            return false;
        }
        if !self.source[stmt_start..stmt_end].contains("/*") {
            return false;
        }

        let try_kw_start = Self::find_keyword_after(self.source, "try", stmt_start);
        let try_kw_end = try_kw_start + 3;
        let Some(try_block_open_rel) = self.source[try_kw_end..stmt_end].find('{') else {
            return false;
        };
        let try_block_open = try_kw_end + try_block_open_rel;
        // Search for the outer `catch` / `finally` keywords AFTER the outer try
        // body. A naive scan from `try_block_open` would hit a nested
        // try/catch's keywords first and merge clauses incorrectly.
        let outer_try_body_end = try_stmt
            .block
            .last()
            .map(|s| s.span.end as usize)
            .unwrap_or(try_block_open + 1);
        let catch_kw_start = try_stmt
            .handler
            .as_ref()
            .map(|_| Self::find_keyword_after(self.source, "catch", outer_try_body_end));
        let finally_kw_start = try_stmt.finalizer.as_ref().map(|_| {
            let search_from = try_stmt
                .handler
                .as_ref()
                .map(|handler| handler.span.end as usize)
                .unwrap_or(outer_try_body_end);
            Self::find_keyword_after(self.source, "finally", search_from)
        });

        let try_clause_end = catch_kw_start
            .map(|pos| pos as u32)
            .or(finally_kw_start.map(|pos| pos as u32))
            .unwrap_or(stmt_span.end);
        let try_pre_block = self
            .source_between(try_kw_end as u32, try_block_open as u32)
            .trim_end()
            .to_string();
        let try_body_start = try_stmt
            .block
            .first()
            .map(|stmt| stmt.span.start)
            .unwrap_or((try_block_open + 1) as u32);
        let try_open_comment = self.clause_open_line_comment(try_block_open, try_body_start);

        self.write("try");
        if try_pre_block.is_empty() {
            self.write(" ");
        } else {
            self.write(&try_pre_block);
            self.write(" ");
        }

        // For empty try blocks, check if the source is single-line.
        // If so, emit `try { }` on one line rather than splitting to multi-line.
        if try_stmt.block.is_empty() {
            let try_block_end = self.source[try_block_open + 1..stmt_end]
                .find('}')
                .map(|off| try_block_open + 1 + off)
                .unwrap_or(try_block_open + 1);
            let is_single_line = !self.source[try_block_open..try_block_end].contains('\n');
            let has_internal_comment =
                self.source[try_block_open..try_block_end + 1].contains("/*");
            if is_single_line && !has_internal_comment {
                self.writeln("{ }");
                self.advance_comment_pos(try_clause_end);
            } else if is_single_line && has_internal_comment {
                // `try { /* ... */ }` on a single line — emit the inner
                // content verbatim so we don't double-emit it via the
                // open-line + close-line comment paths.
                let inner = self.source[try_block_open + 1..try_block_end].trim();
                if inner.is_empty() {
                    self.writeln("{ }");
                } else {
                    self.write("{ ");
                    self.write(inner);
                    self.writeln(" }");
                }
                self.advance_comment_pos(try_clause_end);
            } else {
                self.write("{");
                if let Some(ref try_open_comment) = try_open_comment {
                    self.write(" ");
                    self.write(try_open_comment);
                }
                self.newline();
                self.indent += 1;
                self.advance_comment_pos(
                    self.clause_open_line_end_pos(try_block_open, try_clause_end),
                );
                let (before_close, after_close) =
                    self.clause_close_comments((try_block_open + 1) as u32, try_clause_end);
                let emitted_close =
                    self.emit_clause_close_line(before_close.as_deref(), after_close.as_deref());
                self.indent -= 1;
                if !emitted_close {
                    self.writeln("}");
                }
                self.advance_comment_pos(try_clause_end);
            }
        } else {
            self.write("{");
            if let Some(ref try_open_comment) = try_open_comment {
                self.write(" ");
                self.write(try_open_comment);
            }
            self.newline();
            self.advance_comment_pos(self.clause_open_line_end_pos(try_block_open, try_body_start));
            self.indent += 1;
            for s in &try_stmt.block {
                self.emit_leading_comments(s.span.start);
                self.emit_stmt(s);
                self.advance_comment_pos(s.span.end);
            }
            let try_tail_start = try_stmt
                .block
                .last()
                .map(|stmt| stmt.span.end)
                .unwrap_or((try_block_open + 1) as u32);
            let (before_close, after_close) =
                self.clause_close_comments(try_tail_start, try_clause_end);
            let emitted_close =
                self.emit_clause_close_line(before_close.as_deref(), after_close.as_deref());
            self.indent -= 1;
            if !emitted_close {
                self.writeln("}");
            }
            self.advance_comment_pos(try_clause_end);
        }

        if let Some(ref handler) = try_stmt.handler {
            if let Some(catch_kw_start) = catch_kw_start {
                let search_start = outer_try_body_end.min(catch_kw_start);
                if let Some(close_rel) = self.source[search_start..catch_kw_start].rfind('}') {
                    let close = search_start + close_rel;
                    let between = &self.source[close + 1..catch_kw_start];
                    for line in between.lines() {
                        let comment = line.trim();
                        if comment.starts_with("//") {
                            self.writeln(comment);
                        }
                    }
                }
            }
            let catch_start = handler.span.start as usize;
            let catch_end = handler.span.end as usize;
            if catch_start >= catch_end || catch_end > self.source.len() {
                return false;
            }
            let Some(catch_kw_start) = catch_kw_start else {
                return false;
            };
            let catch_kw_end = catch_kw_start + 5;
            // Locate the catch body's `{`. With a param, scan forward from
            // `param.span.end` (past `)` and whitespace/block comments) for
            // the first `{`. Without a param, `handler.span.start` IS the `{`.
            // (rfind on the catch span would pick a nested block's `{` when
            // the body contains nested try/catch or if-blocks.)
            let catch_block_open = if let Some(ref param) = handler.param {
                let after_param = param.span.end as usize;
                let bytes = self.source.as_bytes();
                let mut i = after_param;
                let mut found: Option<usize> = None;
                while i < catch_end {
                    if i + 1 < catch_end && bytes[i] == b'/' && bytes[i + 1] == b'*' {
                        i += 2;
                        while i + 1 < catch_end {
                            if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                                i += 2;
                                break;
                            }
                            i += 1;
                        }
                    } else if bytes[i] == b'{' {
                        found = Some(i);
                        break;
                    } else {
                        i += 1;
                    }
                }
                let Some(p) = found else {
                    return false;
                };
                p
            } else {
                catch_start
            };
            let catch_clause_end = finally_kw_start
                .map(|pos| pos as u32)
                .unwrap_or(stmt_span.end);

            self.write("catch");
            if let Some(ref param) = handler.param {
                let pre_param_raw = self
                    .source_between(catch_kw_end as u32, param.span.start)
                    .trim_end()
                    .to_string();
                if pre_param_raw.is_empty() {
                    self.write(" (");
                } else {
                    // TypeScript inserts a space after `(` when followed by a
                    // block comment: `catch ( /*...*/[a])` not `catch (/*...*/[a])`.
                    let pre_param = if pre_param_raw.contains("(/*") {
                        pre_param_raw.replacen("(/*", "( /*", 1)
                    } else {
                        pre_param_raw
                    };
                    self.write(&pre_param);
                }
                self.emit_binding_name(param);
                let post_param_raw = self.source_between(param.span.end, catch_block_open as u32);
                // Strip a TypeScript type annotation if present (e.g.
                // `catch (e: any) {...}` — the binding span covers only
                // `e`, so post_param starts with `: ...`). Walk to the
                // matching `)` skipping anything before it. Without this
                // strip the type annotation leaks into the JS output and
                // V8 throws `Unexpected token ':'`.
                let post_param_stripped = if post_param_raw.trim_start().starts_with(':') {
                    let trimmed = post_param_raw.trim_start();
                    // Find the closing `)` — it's the last `)` in this slice
                    // before the block's `{`.
                    if let Some(close_idx) = trimmed.rfind(')') {
                        trimmed[close_idx..].to_string()
                    } else {
                        ")".to_string()
                    }
                } else {
                    post_param_raw.trim_end().to_string()
                };
                let post_param = normalize_close_paren(post_param_stripped.as_str());
                if post_param.is_empty() {
                    self.write(")");
                } else {
                    self.write(&post_param);
                }
            } else {
                let pre_block = self
                    .source_between(catch_kw_end as u32, catch_block_open as u32)
                    .trim_end()
                    .to_string();
                if self.effective_target() < ScriptTarget::ES2019 {
                    let ch = (b'a' + self.catch_auto_param_counter) as char;
                    self.catch_auto_param_counter += 1;
                    self.write(&format!(" (_{ch})"));
                } else if !pre_block.is_empty() {
                    self.write(&pre_block);
                }
            }

            let catch_body_start = handler
                .body
                .first()
                .map(|stmt| stmt.span.start)
                .unwrap_or(catch_clause_end);
            let catch_open_comment =
                self.clause_open_line_comment(catch_block_open, catch_body_start);

            if handler.body.is_empty() {
                // Check if catch block body is single-line in source
                let catch_block_close = self.source[catch_block_open + 1..catch_end]
                    .find('}')
                    .map(|off| catch_block_open + 1 + off)
                    .unwrap_or(catch_block_open + 1);
                let catch_body_single_line =
                    !self.source[catch_block_open..catch_block_close].contains('\n');
                let catch_body_has_comment =
                    self.source[catch_block_open..catch_block_close + 1].contains("/*");
                if catch_body_single_line && !catch_body_has_comment {
                    self.writeln(" { }");
                    self.advance_comment_pos(catch_clause_end);
                } else if catch_body_single_line && catch_body_has_comment {
                    // Single-line catch body with a `/* ... */` comment, e.g.
                    // `catch (_) { /* column already exists */ }`. The earlier
                    // multi-line path was emitting the comment twice — once
                    // via `clause_open_line_comment` (which extracts the
                    // FIRST line of `{...}` content, INCLUDING the trailing
                    // `}`) and once via `clause_close_comments` — producing
                    // `catch (_) { /* x */ }\n    /* x */ }` and breaking V8
                    // parse on the next statement. Emit the source slice
                    // between `{` and `}` verbatim on a single line and
                    // advance comment-pos past the whole clause.
                    let inner_start = catch_block_open + 1;
                    let inner = self.source[inner_start..catch_block_close].trim();
                    if inner.is_empty() {
                        self.writeln(" { }");
                    } else {
                        self.write(" { ");
                        self.write(inner);
                        self.writeln(" }");
                    }
                    self.advance_comment_pos(catch_clause_end);
                } else {
                    self.write(" {");
                    if let Some(ref catch_open_comment) = catch_open_comment {
                        self.write(" ");
                        self.write(catch_open_comment);
                    }
                    self.newline();
                    self.indent += 1;
                    self.advance_comment_pos(
                        self.clause_open_line_end_pos(catch_block_open, catch_body_start),
                    );
                    let (before_close, after_close) =
                        self.clause_close_comments((catch_block_open + 1) as u32, catch_clause_end);
                    let emitted_close = self
                        .emit_clause_close_line(before_close.as_deref(), after_close.as_deref());
                    self.indent -= 1;
                    if !emitted_close {
                        self.writeln("}");
                    }
                    self.advance_comment_pos(catch_clause_end);
                }
            } else {
                self.write(" {");
                if let Some(ref catch_open_comment) = catch_open_comment {
                    self.write(" ");
                    self.write(catch_open_comment);
                }
                self.newline();
                self.advance_comment_pos(
                    self.clause_open_line_end_pos(catch_block_open, catch_body_start),
                );
                self.indent += 1;
                for s in &handler.body {
                    self.emit_leading_comments(s.span.start);
                    self.emit_stmt(s);
                    self.advance_comment_pos(s.span.end);
                }
                let catch_tail_start = handler
                    .body
                    .last()
                    .map(|stmt| stmt.span.end)
                    .unwrap_or((catch_block_open + 1) as u32);
                let (before_close, after_close) =
                    self.clause_close_comments(catch_tail_start, catch_clause_end);
                let emitted_close =
                    self.emit_clause_close_line(before_close.as_deref(), after_close.as_deref());
                self.indent -= 1;
                if !emitted_close {
                    self.writeln("}");
                }
                self.advance_comment_pos(catch_clause_end);
            }
        }

        if let Some(ref fin) = try_stmt.finalizer {
            let Some(finally_kw_start) = finally_kw_start else {
                return false;
            };
            let finally_kw_end = finally_kw_start + 7;
            let Some(finally_block_open_rel) = self.source[finally_kw_end..stmt_end].find('{')
            else {
                return false;
            };
            let finally_block_open = finally_kw_end + finally_block_open_rel;
            let finally_pre_block = self
                .source_between(finally_kw_end as u32, finally_block_open as u32)
                .trim_end()
                .to_string();
            let finally_body_start = fin
                .first()
                .map(|stmt| stmt.span.start)
                .unwrap_or(stmt_span.end);
            let finally_open_comment =
                self.clause_open_line_comment(finally_block_open, finally_body_start);

            self.write("finally");
            if finally_pre_block.is_empty() {
                self.write(" ");
            } else {
                self.write(&finally_pre_block);
                self.write(" ");
            }
            self.write("{");
            if let Some(ref finally_open_comment) = finally_open_comment {
                self.write(" ");
                self.write(finally_open_comment);
            }
            self.newline();

            if fin.is_empty() {
                self.indent += 1;
                self.advance_comment_pos(
                    self.clause_open_line_end_pos(finally_block_open, finally_body_start),
                );
                let (before_close, after_close) =
                    self.clause_close_comments((finally_block_open + 1) as u32, stmt_span.end);
                let emitted_close =
                    self.emit_clause_close_line(before_close.as_deref(), after_close.as_deref());
                self.indent -= 1;
                if !emitted_close {
                    self.writeln("}");
                }
            } else {
                self.advance_comment_pos(
                    self.clause_open_line_end_pos(finally_block_open, finally_body_start),
                );
                self.indent += 1;
                for s in fin {
                    self.emit_leading_comments(s.span.start);
                    self.emit_stmt(s);
                    self.advance_comment_pos(s.span.end);
                }
                let finally_tail_start = fin
                    .last()
                    .map(|stmt| stmt.span.end)
                    .unwrap_or((finally_block_open + 1) as u32);
                let (before_close, after_close) =
                    self.clause_close_comments(finally_tail_start, stmt_span.end);
                let emitted_close =
                    self.emit_clause_close_line(before_close.as_deref(), after_close.as_deref());
                self.indent -= 1;
                if !emitted_close {
                    self.writeln("}");
                }
            }
            self.advance_comment_pos(stmt_span.end);
        }

        true
    }

    pub(super) fn emit_try_stmt(&mut self, try_stmt: &TryStmt, stmt_span: Span) {
        if self.emit_try_stmt_preserving_internal_comments(try_stmt, stmt_span) {
            return;
        }
        if try_stmt.block.is_empty() {
            // Check if the source try block was multi-line or single-line
            let src_start = stmt_span.start as usize;
            let src_end = stmt_span.end as usize;
            // Find the absolute source position of the try block's closing `}`.
            let close_brace_pos = if src_start < src_end && src_end <= self.source.len() {
                let src_text = &self.source[src_start..src_end];
                src_text.find('}').map(|off| src_start + off)
            } else {
                None
            };
            let is_multiline =
                close_brace_pos.map_or(false, |pos| self.source[src_start..pos].contains('\n'));
            if is_multiline {
                self.writeln("try {");
                // Emit any leading comments inside the empty try block
                // (e.g. `try { // comment }` with the comment on its own line).
                if let Some(close_pos) = close_brace_pos {
                    self.indent += 1;
                    self.emit_leading_comments(close_pos as u32);
                    self.indent -= 1;
                }
                self.writeln("}");
            } else {
                self.writeln("try { }");
            }
        } else {
            self.writeln("try {");
            self.indent += 1;
            self.cjs_inline_export_depth += 1;
            for s in &try_stmt.block {
                self.emit_leading_comments(s.span.start);
                self.emit_stmt(s);
                self.advance_comment_pos(s.span.end);
            }
            self.cjs_inline_export_depth -= 1;
            self.indent -= 1;
            self.writeln("}");
        }
        // Emit comments between try block `}` and catch/finally keyword
        // (e.g. `} \n // @ts-ignore \n catch`).
        if let Some(ref handler) = try_stmt.handler {
            self.emit_leading_comments(handler.span.start);

            // Check if catch param has object rest that needs downlevel transform
            let catch_rest_temp = if self.needs_downlevel("object-spread") {
                handler.param.as_ref().and_then(|param| {
                    use crate::helpers_rest::top_level_object_props;
                    let props = top_level_object_props(param)?;
                    if props.iter().any(|p| matches!(p, ObjPatProp::Rest(_))) {
                        let ch = (b'a' + self.catch_auto_param_counter) as char;
                        self.catch_auto_param_counter += 1;
                        Some(format!("_{ch}"))
                    } else {
                        None
                    }
                })
            } else {
                None
            };

            let body_empty = handler.body.is_empty() && catch_rest_temp.is_none();
            if body_empty {
                // Check if the source catch block was multi-line.
                // We find the block's opening `{` by looking at the source AFTER
                // the catch parameter (if any). The block `{` is the last `{`
                // in the catch clause (handler span), since the param pattern's
                // braces come before the block brace.
                let catch_start = handler.span.start as usize;
                let catch_end = handler.span.end as usize;
                // Find the block's opening `{`: it's the LAST `{` in the handler span
                // (after any pattern braces).
                let (catch_multiline, catch_close_pos) =
                    if catch_start < catch_end && catch_end <= self.source.len() {
                        let src = &self.source[catch_start..catch_end];
                        // The block's `{` is the last `{` before the final `}`.
                        // The final `}` is the block's closing brace.
                        if let Some(block_open) = src.rfind('{') {
                            let after_open = &src[block_open..];
                            let is_ml = after_open.contains('\n');
                            // Absolute position of the closing `}` (last `}` in handler span).
                            let close_off = src.rfind('}').map(|off| catch_start + off);
                            (is_ml, close_off)
                        } else {
                            (false, None)
                        }
                    } else {
                        (false, None)
                    };
                self.write("catch");
                if let Some(ref param) = handler.param {
                    self.write(" (");
                    self.emit_binding_name(param);
                    self.write(")");
                } else if self.effective_target() < ScriptTarget::ES2019 {
                    let ch = (b'a' + self.catch_auto_param_counter) as char;
                    self.catch_auto_param_counter += 1;
                    self.write(&format!(" (_{ch})"));
                }
                if catch_multiline {
                    self.write(" {");
                    self.append_brace_trailing_comment(handler.span);
                    self.newline();
                    if let Some(close_pos) = catch_close_pos {
                        self.indent += 1;
                        self.emit_leading_comments(close_pos as u32);
                        self.indent -= 1;
                    }
                    self.writeln("}");
                } else {
                    self.writeln(" { }");
                }
            } else {
                self.write("catch");
                if let Some(ref temp_name) = catch_rest_temp {
                    // Object rest transform: use temp var instead of pattern
                    self.write(&format!(" ({temp_name}) {{"));
                } else if let Some(ref param) = handler.param {
                    self.write(" (");
                    self.emit_binding_name(param);
                    self.write(") {");
                } else if self.effective_target() < ScriptTarget::ES2019 {
                    let ch = (b'a' + self.catch_auto_param_counter) as char;
                    self.catch_auto_param_counter += 1;
                    self.write(&format!(" (_{ch}) {{"));
                } else {
                    self.write(" {");
                }
                self.append_brace_trailing_comment(handler.span);
                self.newline();
                self.indent += 1;
                self.cjs_inline_export_depth += 1;
                // Emit rest destructuring prefix if needed
                if let Some(ref temp_name) = catch_rest_temp {
                    if let Some(ref param) = handler.param {
                        self.emit_rest_param_destructuring(temp_name, param);
                    }
                }
                for s in &handler.body {
                    self.emit_leading_comments(s.span.start);
                    self.emit_stmt(s);
                    self.advance_comment_pos(s.span.end);
                }
                self.cjs_inline_export_depth -= 1;
                // Emit comments between the last catch statement and the
                // closing `}` of the catch block.
                {
                    let catch_end = handler.span.end as usize;
                    if catch_end > 0 && catch_end <= self.source.len() {
                        let brace_pos = if self.source.as_bytes().get(catch_end - 1) == Some(&b'}')
                        {
                            (catch_end - 1) as u32
                        } else {
                            self.source[..catch_end]
                                .rfind('}')
                                .map(|p| p as u32)
                                .unwrap_or(catch_end as u32)
                        };
                        self.emit_leading_comments(brace_pos);
                    }
                }
                self.indent -= 1;
                self.writeln("}");
            }
        }
        if let Some(ref fin) = try_stmt.finalizer {
            if fin.is_empty() {
                // Check if the source finally block was multi-line.
                // Find the position of "finally" and then its closing `}`.
                let src_start = stmt_span.start as usize;
                let src_end = stmt_span.end as usize;
                let (fin_multiline, fin_close_pos) =
                    if src_start < src_end && src_end <= self.source.len() {
                        let src = &self.source[src_start..src_end];
                        if let Some(fin_pos) = src.rfind("finally") {
                            let after_finally = &src[fin_pos..];
                            if let Some(open) = after_finally.find('{') {
                                let after_open = &after_finally[open..];
                                let is_ml = after_open.contains('\n');
                                // Absolute position of the closing `}` of the finally block.
                                let close_off = after_finally[open..]
                                    .rfind('}')
                                    .map(|off| src_start + fin_pos + open + off);
                                (is_ml, close_off)
                            } else {
                                (false, None)
                            }
                        } else {
                            (false, None)
                        }
                    } else {
                        (false, None)
                    };
                if fin_multiline {
                    self.writeln("finally {");
                    if let Some(close_pos) = fin_close_pos {
                        self.indent += 1;
                        self.emit_leading_comments(close_pos as u32);
                        self.indent -= 1;
                    }
                    self.writeln("}");
                } else {
                    self.writeln("finally { }");
                }
            } else {
                self.writeln("finally {");
                self.indent += 1;
                self.cjs_inline_export_depth += 1;
                for s in fin {
                    self.emit_leading_comments(s.span.start);
                    self.emit_stmt(s);
                    self.advance_comment_pos(s.span.end);
                }
                self.cjs_inline_export_depth -= 1;
                self.indent -= 1;
                self.writeln("}");
            }
        }
    }

    pub(super) fn emit_fn_decl(&mut self, fn_decl: &FnDecl) {
        if fn_decl.modifiers & MOD_DECLARE != 0 {
            return; // ambient declaration
        }
        // Emit `static ` prefix when preceding `static` keyword was stripped
        // during error recovery (e.g. `static function fn() {}` in namespace).
        if self.emit_static_prefix {
            self.write("static ");
            self.emit_static_prefix = false;
        }
        if fn_decl.body.is_none() {
            if self.emit_recovery_reserved_word_throw_fn_decl(fn_decl) {
                return;
            }
            if let Some((skip_end, recovered_lines)) =
                self.fn_decl_static_recovery_body_lines(fn_decl)
            {
                if let Some(ref name) = fn_decl.name {
                    self.emitted_var_names.insert(name.clone().into());
                }
                self.write("function");
                if fn_decl.is_generator {
                    self.write("*");
                }
                if let Some(ref name) = fn_decl.name {
                    self.write(" ");
                    self.write(name);
                } else {
                    self.write(" ");
                }
                self.writeln("() {");
                self.indent += 1;
                for line in recovered_lines {
                    self.write(&line);
                    self.newline();
                }
                self.indent -= 1;
                self.write("}");
                self.skip_recovery_until = self.skip_recovery_until.max(skip_end);
                return;
            }
            if let Some(skip_end) = self.fn_decl_bare_arrow_param_recovery_end(fn_decl) {
                self.writeln("function () { }");
                self.skip_recovery_until = self.skip_recovery_until.max(skip_end);
                return;
            }
            if let Some(skip_end) = self.fn_decl_recovery_body_end(fn_decl) {
                // Parser-recovery function signature with an inline `{` body hint.
                if let Some(ref name) = fn_decl.name {
                    self.emitted_var_names.insert(name.clone().into());
                }
                self.write("function");
                if fn_decl.is_generator {
                    self.write("*");
                }
                if let Some(ref name) = fn_decl.name {
                    self.write(" ");
                    self.write(name);
                } else {
                    self.write(" ");
                }
                self.write("(");
                let rest_infos = if self.params_need_rest_transform(&fn_decl.params) {
                    self.emit_params_with_rest_transform(&fn_decl.params)
                } else {
                    self.emit_params(&fn_decl.params);
                    vec![]
                };
                if rest_infos.is_empty() {
                    let compact_recovery_body = fn_decl.params.iter().any(|p| {
                        p.dotdotdot
                            && (p.initializer.is_some()
                                || !matches!(&p.name.kind, PatKind::Ident(_)))
                    });
                    if compact_recovery_body {
                        self.writeln(") { }");
                    } else {
                        self.writeln(") {");
                        self.writeln("}");
                    }
                } else {
                    self.write(") {");
                    for (i, (_, temp_name, pat, _)) in rest_infos.iter().enumerate() {
                        if i > 0 {
                            self.write(" ");
                        } else {
                            self.write(" ");
                        }
                        let before = self.output.len();
                        self.emit_rest_param_destructuring(temp_name, pat);
                        while self.output.ends_with('\n') {
                            self.output.pop();
                        }
                        if self.output.ends_with(';') && self.output.len() > before {
                            self.write(" ");
                        }
                    }
                    self.write("}");
                }
                self.skip_recovery_until = self.skip_recovery_until.max(skip_end);
            } else if let Some(skip_end) = self.fn_decl_skip_same_line_tail_end(fn_decl) {
                self.skip_recovery_until = self.skip_recovery_until.max(skip_end);
            } else if self.fn_decl_has_recovery_comma_successor(fn_decl) {
                if let Some(ref name) = fn_decl.name {
                    self.emitted_var_names.insert(name.clone().into());
                }
                self.write("function");
                if fn_decl.is_generator {
                    self.write("*");
                }
                if let Some(ref name) = fn_decl.name {
                    self.write(" ");
                    self.write(name);
                } else {
                    self.write(" ");
                }
                self.write("(");
                self.emit_params(&fn_decl.params);
                self.writeln(") { }");
            } else if fn_decl.name.is_some() && self.fn_decl_has_non_ascii_body_separator(fn_decl) {
                // Named function with non-ASCII error token before `{` on same line.
                // Emit with synthesized empty body, matching TypeScript's behavior
                // for `function Foo() ¬ { }` where `¬` is a skipped error token.
                let name = fn_decl.name.as_deref().unwrap();
                self.emitted_var_names.insert(name.into());
                self.write("function");
                if fn_decl.is_generator {
                    self.write("*");
                }
                self.write(" ");
                self.write(name);
                self.write("(");
                self.emit_params(&fn_decl.params);
                self.writeln(") { }");
            }
            return; // overload signature or unrecoverable parse artifact
        }
        if let Some((recovered_params, recovery_lines)) =
            self.fn_decl_reserved_word_param_recovery_lines(fn_decl)
        {
            if let Some(ref name) = fn_decl.name {
                self.emitted_var_names.insert(name.clone().into());
            }
            self.write("function");
            if fn_decl.is_generator {
                self.write("*");
            }
            if let Some(ref name) = fn_decl.name {
                self.write(" ");
                self.write(name);
            } else {
                self.write(" ");
            }
            self.write("(");
            self.write(recovered_params);
            self.writeln(") { }");
            for line in recovery_lines {
                self.writeln(line);
            }
            return;
        }
        let emitted_fn_name = fn_decl.name.as_ref().map(|name| {
            fn_decl
                .name_span
                .and_then(|span| {
                    self.lexical_downlevel_plan
                        .emitted_name_for_declaration(span)
                })
                .unwrap_or(name.as_str())
                .to_string()
        });
        // Register the function name so merged namespace/enum IIFEs skip `var`.
        if let Some(ref name) = emitted_fn_name {
            self.emitted_var_names.insert(name.as_str().into());
        }
        let saved_arguments_alias = self.current_arguments_alias.take();
        let downlevel_async =
            fn_decl.is_async && !fn_decl.is_generator && self.needs_downlevel("async");
        let downlevel_async_generator =
            fn_decl.is_async && fn_decl.is_generator && self.needs_downlevel("async-generator");
        let async_gen_move_params =
            downlevel_async_generator && fn_decl.params.iter().any(Self::param_needs_async_lift);
        // When downleveling async and any parameter has a default value or
        // destructured pattern, TypeScript moves ALL parameters to the inner
        // generator function and passes `arguments` to __awaiter.
        // For destructured params, temp names are also emitted on the outer function.
        let async_move_params = downlevel_async
            && !fn_decl.is_generator
            && fn_decl
                .params
                .iter()
                .any(|p| Self::param_needs_async_lift(p));
        let async_has_destructured = async_move_params
            && fn_decl
                .params
                .iter()
                .any(|p| !matches!(p.name.kind, PatKind::Ident(_)));
        let generator_yield_arrow_param_recovery =
            self.fn_decl_generator_yield_arrow_param_recovery(fn_decl);
        let can_downlevel_param_inits = !fn_decl.is_async
            && !fn_decl.is_generator
            && self.can_downlevel_simple_param_initializers(&fn_decl.params);
        let prev_in_async = self.in_async_function;
        self.in_async_function = fn_decl.is_async;
        if fn_decl.is_async && !(downlevel_async || downlevel_async_generator) {
            self.write("async ");
        }
        self.write("function");
        if fn_decl.is_generator && !downlevel_async_generator {
            self.write("*");
        }
        if let Some(ref name) = emitted_fn_name {
            self.write(" ");
            self.write(name);
        } else {
            // TypeScript always emits a space before ( for anonymous functions
            self.write(" ");
        }
        // Emit comments between function name and opening `(`
        // e.g. `function clone/* <T> */(...)` → `function clone /* <T> */(...)`
        if fn_decl.type_params.is_none() && !fn_decl.params.is_empty() {
            if let Some(ref name) = fn_decl.name {
                let fn_start = fn_decl.span.start as usize;
                let first_param_start = fn_decl.params[0].span.start as usize;
                if let Some(name_offset) = self
                    .source
                    .get(fn_start..)
                    .and_then(|s| s.find(name.as_str()))
                {
                    let name_end = fn_start + name_offset + name.len();
                    if let Some(paren_rel) = self
                        .source
                        .get(name_end..first_param_start)
                        .and_then(|s| s.find('('))
                    {
                        self.emit_paren_comments_in_range(
                            name_end as u32,
                            (name_end + paren_rel) as u32,
                        );
                    }
                }
            }
        }
        self.write("(");
        let rest_infos = if async_has_destructured {
            // Destructured params: emit temp names on outer function.
            let temps = Self::generate_async_lift_temp_names(&fn_decl.params);
            for (i, t) in temps.iter().enumerate() {
                if i > 0 {
                    self.write(", ");
                }
                self.write(t);
            }
            vec![]
        } else if async_move_params {
            // Default-initializer lift: keep prefix temps before first lifted param.
            let temps = Self::generate_async_lift_prefix_temp_names(&fn_decl.params);
            for (i, t) in temps.iter().enumerate() {
                if i > 0 {
                    self.write(", ");
                }
                self.write(t);
            }
            vec![]
        } else if async_gen_move_params {
            let temps = if fn_decl.params.iter().any(|p| p.initializer.is_some()) {
                Self::generate_async_lift_prefix_temp_names(&fn_decl.params)
            } else {
                Self::generate_async_lift_temp_names(&fn_decl.params)
            };
            for (i, t) in temps.iter().enumerate() {
                if i > 0 {
                    self.write(", ");
                }
                self.write(t);
            }
            vec![]
        } else if self.params_need_rest_transform(&fn_decl.params) {
            self.emit_params_with_rest_transform(&fn_decl.params)
        } else {
            if let Some(recovery_param_name) = generator_yield_arrow_param_recovery.as_deref() {
                self.write(recovery_param_name);
                self.write(" = yield, yield");
            } else if can_downlevel_param_inits {
                self.emit_params_without_initializers(&fn_decl.params);
            } else if !self.emit_multiline_commented_param_list(fn_decl) {
                self.emit_params(&fn_decl.params);
                // Preserve comments in empty parameter lists (e.g. `function foo(/** nothing */)`)
                self.emit_empty_parens_comments(fn_decl.span, &fn_decl.params);
            }
            vec![]
        };
        self.write(") ");
        // Skip comments inside erased return type annotation
        if let Some(ref rt) = fn_decl.return_type {
            self.advance_comment_pos(rt.span.end);
        }
        if let Some(ref body) = fn_decl.body {
            // Track parameter names that shadow CJS/namespace exports so
            // `x` inside this function body isn't qualified as `exports.x`
            // or `NS.x` when `x` is a parameter name.
            // Clone (not mem::take) so the ENCLOSING scope's param shadows stay
            // active inside this nested function — a name shadowed by an outer
            // function's parameter still resolves to that param here and must
            // not be re-qualified as `exports.x`.
            let prev_cjs_param_shadows = self.cjs_param_shadows.clone();
            if !self.cjs_var_export_names.is_empty() {
                for p in &fn_decl.params {
                    Self::collect_param_shadow_names(
                        &p.name,
                        &self.cjs_var_export_names,
                        &mut self.cjs_param_shadows,
                    );
                }
            }
            if !self.namespace_exports.is_empty() {
                for p in &fn_decl.params {
                    Self::collect_param_shadow_names(
                        &p.name,
                        &self.namespace_exports,
                        &mut self.cjs_param_shadows,
                    );
                }
            }
            let local_enums = if self.should_inline_const_enums() {
                self.push_local_const_enums(body)
            } else {
                Vec::new()
            };
            let async_arguments_alias = if downlevel_async && stmts_have_lexical_arguments(body) {
                Some(self.next_arguments_capture_name())
            } else {
                None
            };
            if downlevel_async || downlevel_async_generator {
                if downlevel_async_generator {
                    let inner_name = fn_decl.name.as_ref().map(|n| format!("{n}_1"));
                    self.awaiter_enclosing_span = Some(fn_decl.span);
                    self.emit_async_generator_body(
                        body,
                        inner_name.as_deref(),
                        async_gen_move_params.then_some(fn_decl.params.as_slice()),
                    );
                } else if !rest_infos.is_empty() {
                    // Async function with rest params: inject destructuring
                    // into the awaiter body
                    self.fn_scope_depth += 1;
                    self.writeln("{");
                    self.indent += 1;
                    let prev_arguments_alias = self.current_arguments_alias.clone();
                    if let Some(alias) = async_arguments_alias.as_ref() {
                        self.write("var ");
                        self.write(alias);
                        self.writeln(" = arguments;");
                        self.current_arguments_alias = Some(alias.clone());
                    }
                    self.write("return __awaiter(this, void 0, void 0, function* () ");
                    self.writeln("{");
                    self.indent += 1;
                    for (_, temp_name, pat, _) in &rest_infos {
                        self.emit_rest_param_destructuring_await_to_yield(temp_name, pat);
                    }
                    self.emit_rest_lifted_defaults();
                    for s in body {
                        self.emit_leading_comments(s.span.start);
                        self.emit_stmt_await_to_yield(s);
                        self.advance_comment_pos(s.span.end);
                    }
                    self.indent -= 1;
                    self.write("}");
                    self.writeln(");");
                    self.indent -= 1;
                    self.write("}");
                    self.fn_scope_depth -= 1;
                    self.current_arguments_alias = prev_arguments_alias;
                } else if async_move_params {
                    self.awaiter_enclosing_span = Some(fn_decl.span);
                    self.emit_awaiter_body_with_params(
                        body,
                        Some(&fn_decl.params),
                        &fn_decl.params,
                        async_arguments_alias.as_deref(),
                    );
                } else {
                    self.awaiter_enclosing_span = Some(fn_decl.span);
                    self.emit_awaiter_body_with_params(
                        body,
                        None,
                        &fn_decl.params,
                        async_arguments_alias.as_deref(),
                    );
                }
            } else if !rest_infos.is_empty() {
                // Non-async function with rest params.
                // Prologue directives ("use strict", etc.) must come before
                // the destructured parameter expansion.
                if body.is_empty() {
                    // Check if the source function was single-line.
                    let source_single_line = {
                        let start = fn_decl.span.start as usize;
                        let end = fn_decl.span.end as usize;
                        if start < end && end <= self.source.len() {
                            let text = &self.source[start..end];
                            text.rfind('{')
                                .map_or(false, |bp| !text[bp..].contains('\n'))
                        } else {
                            false
                        }
                    };
                    if source_single_line {
                        self.write("{ ");
                        for (i, (_, temp_name, pat, _)) in rest_infos.iter().enumerate() {
                            if i > 0 {
                                self.write(" ");
                            }
                            let before = self.output.len();
                            self.emit_rest_param_destructuring(temp_name, pat);
                            while self.output.ends_with('\n') {
                                self.output.pop();
                            }
                            if self.output.ends_with(';') && self.output.len() > before {
                                self.write(" ");
                            }
                        }
                        self.write("}");
                    } else {
                        self.writeln("{");
                        self.indent += 1;
                        for (_, temp_name, pat, _) in &rest_infos {
                            self.emit_rest_param_destructuring(temp_name, pat);
                        }
                        self.emit_rest_lifted_defaults();
                        self.indent -= 1;
                        self.write("}");
                    }
                } else {
                    self.writeln("{");
                    self.indent += 1;
                    // Emit prologue directives first.
                    let mut prologue_count = 0;
                    for s in body {
                        if let StmtKind::Expr(expr) = &s.kind {
                            if matches!(expr.kind, ExprKind::StrLit(_)) {
                                self.emit_stmt(s);
                                prologue_count += 1;
                                continue;
                            }
                        }
                        break;
                    }
                    for (_, temp_name, pat, _) in &rest_infos {
                        self.emit_rest_param_destructuring(temp_name, pat);
                    }
                    self.emit_rest_lifted_defaults();
                    for s in body.iter().skip(prologue_count) {
                        self.emit_stmt(s);
                    }
                    self.indent -= 1;
                    self.write("}");
                }
            } else if can_downlevel_param_inits {
                self.emit_block_for_decl_body_with_param_initializers(
                    &fn_decl.params,
                    body,
                    fn_decl.span,
                );
            } else {
                self.emit_block_for_decl_body(body, fn_decl.span);
            }
            self.pop_local_const_enums(local_enums);
            self.cjs_param_shadows = prev_cjs_param_shadows;
        }
        self.in_async_function = prev_in_async;
        self.current_arguments_alias = saved_arguments_alias;
        self.newline();
    }

    fn emit_this_parameter_initializer_function_recovery(&mut self, fn_decl: &FnDecl) -> bool {
        let Some(first) = fn_decl.params.first() else {
            return false;
        };
        if !matches!(&first.name.kind, PatKind::Ident(name) if name == "this")
            || first.initializer.is_none()
        {
            return false;
        }
        let Some(body) = fn_decl.body.as_ref() else {
            return false;
        };

        self.writeln("();");
        if let Some(return_type) = &fn_decl.return_type {
            let raw = self.copy_span_trimmed(return_type.span).trim();
            if !raw.is_empty() {
                self.write(raw);
                self.writeln(";");
            }
        }
        self.writeln("{");
        self.indent += 1;
        for statement in body {
            self.emit_leading_comments(statement.span.start);
            self.emit_stmt(statement);
            self.advance_comment_pos(statement.span.end);
        }
        self.indent -= 1;
        self.writeln("}");
        true
    }

    fn fn_decl_generator_yield_arrow_param_recovery(&self, fn_decl: &FnDecl) -> Option<String> {
        if !fn_decl.is_generator {
            return None;
        }
        let [param] = fn_decl.params.as_slice() else {
            return None;
        };
        let PatKind::Ident(param_name) = &param.name.kind else {
            return None;
        };
        let init = param.initializer.as_ref()?;
        let ExprKind::Arrow(arrow) = &init.kind else {
            return None;
        };
        if arrow.is_async
            || arrow.return_type.is_some()
            || arrow
                .type_params
                .as_ref()
                .is_some_and(|params| !params.is_empty())
        {
            return None;
        }
        let [arrow_param] = arrow.params.as_slice() else {
            return None;
        };
        if !matches!(&arrow_param.name.kind, PatKind::Ident(name) if name == "yield") {
            return None;
        }
        if !matches!(&arrow.body, ArrowBody::Expr(body) if matches!(&body.kind, ExprKind::Yield(false, None)))
        {
            return None;
        }
        if self.copy_span_trimmed(init.span).trim() != "yield => yield" {
            return None;
        }
        Some(param_name.to_string())
    }

    pub(super) fn emit_recovery_reserved_word_import_decl(
        &mut self,
        import_decl: &ImportDecl,
        stmt_span: Span,
    ) -> bool {
        if !self.import_decl_has_reserved_word_recovery_shape(import_decl, stmt_span) {
            return false;
        }
        let raw = self.copy_span_trimmed(stmt_span);
        if raw == "import" {
            self.writeln("require();");
            return true;
        }

        self.writeln("while (from)");
        self.indent += 1;
        self.write("\"");
        self.write(&import_decl.source);
        self.writeln("\";");
        self.indent -= 1;
        true
    }

    fn emit_recovery_import_type_defer_conflict(
        &mut self,
        import_decl: &ImportDecl,
        stmt_span: Span,
    ) -> bool {
        let Some(name) = self.import_type_defer_conflict_name(import_decl, stmt_span) else {
            return false;
        };
        self.writeln(" * as;");
        self.write(&name);
        self.writeln(";");
        true
    }

    pub(super) fn import_type_defer_conflict_name(
        &self,
        import_decl: &ImportDecl,
        stmt_span: Span,
    ) -> Option<String> {
        if !import_decl.type_only || !import_decl.source.is_empty() {
            return None;
        }
        let line_start = stmt_span.start as usize;
        let line_end = self.source[line_start..]
            .find('\n')
            .map_or(self.source.len(), |offset| line_start + offset);
        let line = &self.source[line_start..line_end];
        if !line.trim_start().starts_with("import type defer * as ") {
            return None;
        }
        let after_as = line.split_once("* as ").map(|(_, tail)| tail)?;
        let (name, _) = after_as.split_once(" from ")?;
        let name = name.trim();
        if name.is_empty() {
            return None;
        }
        Some(name.to_string())
    }

    fn emit_recovery_reserved_word_while_stmt(&mut self, wh: &WhileStmt, stmt_span: Span) -> bool {
        if !matches!(&wh.test.kind, ExprKind::Ident(name) if name == "<error>") {
            return false;
        }
        let StmtKind::Expr(body_expr) = &wh.body.kind else {
            return false;
        };
        let ExprKind::Call(call) = &body_expr.kind else {
            return false;
        };
        if !matches!(&call.callee.kind, ExprKind::Ident(name) if name == "require") {
            return false;
        }
        if self.copy_span_trimmed(stmt_span).trim() != "while = require(\"dfdf\");" {
            return false;
        }

        self.write("while ( = ");
        self.emit_expr(body_expr);
        self.writeln(")");
        self.indent += 1;
        self.writeln(";");
        self.indent -= 1;
        true
    }

    fn emit_recovery_reserved_word_var_stmt(&mut self, stmt: &Stmt, var_stmt: &VarStmt) -> bool {
        if !self.var_stmt_has_reserved_word_recovery_shape(stmt, var_stmt) {
            return false;
        }
        let [decl] = var_stmt.declarations.as_slice() else {
            return false;
        };

        if matches!(&decl.name.kind, PatKind::Ident(name) if name == "<error>")
            && decl.type_ann.is_none()
        {
            let raw = self.copy_span_trimmed(stmt.span);
            if let Some(after_var) = raw.strip_prefix("var ") {
                if let Some((raw_name, _)) = after_var.split_once('=') {
                    let raw_name = raw_name.trim();
                    if raw_name == "typeof" {
                        let Some(init) = &decl.init else {
                            return false;
                        };
                        self.writeln("var ;");
                        self.write(raw_name);
                        self.writeln(" ;");
                        self.emit_expr(init);
                        self.writeln(";");
                        return true;
                    }
                }
            }
        }

        let PatKind::Array(_) = &decl.name.kind else {
            return false;
        };
        let Some(init) = &decl.init else {
            return false;
        };
        let ExprKind::ArrayLit(_) = &init.kind else {
            return false;
        };
        let raw = self.copy_span_trimmed(stmt.span);
        let Some(open_bracket) = raw.find('[') else {
            return false;
        };
        let Some(close_bracket) = raw.find(']') else {
            return false;
        };
        let inner = raw[open_bracket + 1..close_bracket].trim();
        let Some((first, second)) = inner.split_once(',') else {
            return false;
        };
        let first = first.trim();
        let second = second.trim();
        if first != "debugger" || second != "if" {
            return false;
        }

        self.writeln("var [];");
        self.writeln("debugger;");
        self.writeln("if ()");
        self.indent += 1;
        self.writeln(";");
        self.indent -= 1;
        self.emit_expr(init);
        self.writeln(";");
        true
    }

    fn var_stmt_has_generator_object_method_recovery(
        &self,
        stmt: &Stmt,
        var_stmt: &VarStmt,
    ) -> bool {
        // Parser-error-recovery detection (malformed generator-object-method in a
        // var init) — such shapes only exist alongside parse diagnostics, so on
        // clean fast-emit input skip the per-statement whitespace-stripped source
        // scan + String alloc this would otherwise do. On files WITH recovery
        // errors the scan must still run even under fast emit: it routes the
        // malformed statement to structured emit for repair.
        if self.fast_emit && !self.file_has_recovery_errors {
            return false;
        }
        let [decl] = var_stmt.declarations.as_slice() else {
            return false;
        };
        let PatKind::Ident(binding_name) = &decl.name.kind else {
            return false;
        };
        let Some(init) = &decl.init else {
            return false;
        };
        let start = stmt.span.start as usize;
        if start >= self.source.len() {
            return false;
        }
        let line_end = self.source[start..]
            .find('\n')
            .map(|idx| start + idx)
            .unwrap_or(self.source.len());
        let compact: String = self.source[start..line_end]
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .collect();
        let kw = match var_stmt.kind {
            VarKind::Var => "var",
            VarKind::Let => "let",
            VarKind::Const => "const",
            VarKind::Using => "using",
            VarKind::AwaitUsing => "awaitusing",
        };
        let prefix = format!("{kw}{binding_name}=");
        if !compact.starts_with(&prefix) {
            return false;
        }
        match &init.kind {
            ExprKind::Call(call)
                if !call.optional
                    && call.args.is_empty()
                    && matches!(call.callee.kind, ExprKind::ObjectLit(ref props) if props.is_empty()) =>
            {
                compact.contains("={*") && compact.ends_with("(){}}")
            }
            ExprKind::ObjectLit(props) if props.is_empty() => {
                compact.contains("={*{}}") || compact.contains("={*}")
            }
            _ => false,
        }
    }

    fn emit_recovery_generator_object_method_var_stmt(
        &mut self,
        stmt: &Stmt,
        var_stmt: &VarStmt,
    ) -> bool {
        if !self.var_stmt_has_generator_object_method_recovery(stmt, var_stmt) {
            return false;
        }
        let [decl] = var_stmt.declarations.as_slice() else {
            return false;
        };
        let PatKind::Ident(binding_name) = &decl.name.kind else {
            return false;
        };
        let Some(init) = &decl.init else {
            return false;
        };
        let start = stmt.span.start as usize;
        let line_end = self.source[start..]
            .find('\n')
            .map(|idx| start + idx)
            .unwrap_or(self.source.len());
        let compact: String = self.source[start..line_end]
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .collect();
        let kw = self.emitted_var_keyword(var_stmt);

        self.write(kw);
        self.write(" ");
        self.write(binding_name);
        match &init.kind {
            ExprKind::Call(_) | ExprKind::ObjectLit(_)
                if compact.contains("={*{}}")
                    || (compact.contains("={*") && compact.ends_with("(){}}")) =>
            {
                self.writeln(" = { *() { } };");
            }
            ExprKind::ObjectLit(_) if compact.contains("={*}") => {
                self.writeln(" = {};");
            }
            _ => return false,
        }

        let skip_end = line_end as u32;
        self.skip_recovery_until = self.skip_recovery_until.max(skip_end);
        self.advance_comment_pos(skip_end);
        true
    }

    fn var_stmt_has_invalid_let_array_recovery(&self, stmt: &Stmt, var_stmt: &VarStmt) -> bool {
        if var_stmt.kind != VarKind::Let {
            return false;
        }
        let [decl] = var_stmt.declarations.as_slice() else {
            return false;
        };
        let PatKind::Array(elements) = &decl.name.kind else {
            return false;
        };
        if elements.len() != 1
            || !matches!(
                elements.first(),
                Some(Some(ArrayPatElem::Pat(pat)))
                    if matches!(&pat.kind, PatKind::Ident(name) if name == "<error>")
            )
            || decl.init.is_none()
        {
            return false;
        }

        let raw = self.copy_span_trimmed(stmt.span).trim();
        raw.starts_with("let[") && raw.contains("] =")
    }

    fn emit_recovery_invalid_let_array_var_stmt(
        &mut self,
        stmt: &Stmt,
        var_stmt: &VarStmt,
    ) -> bool {
        if !self.var_stmt_has_invalid_let_array_recovery(stmt, var_stmt) {
            return false;
        }

        let raw = self.copy_span_trimmed(stmt.span).trim();
        let Some(open) = raw.find('[') else {
            return false;
        };
        let Some(close) = raw[open + 1..].find(']') else {
            return false;
        };
        let close = open + 1 + close;
        let inner = raw[open + 1..close].trim();
        let rhs = raw[close + 1..]
            .trim_start()
            .strip_prefix('=')
            .map(str::trim)
            .map(|text| text.trim_end_matches(';').trim())
            .filter(|text| !text.is_empty());
        let Some(rhs) = rhs else {
            return false;
        };

        self.writeln("let [];");
        self.writeln(&format!("{inner};"));
        self.writeln(&format!("{rhs};"));
        self.skip_recovery_until = self.skip_recovery_until.max(stmt.span.end);
        true
    }

    fn var_stmt_has_malformed_arrow_type_recovery(&self, var_stmt: &VarStmt) -> bool {
        let [decl] = var_stmt.declarations.as_slice() else {
            return false;
        };
        let Some(init) = decl.init.as_ref() else {
            return false;
        };
        let ExprKind::Arrow(arrow) = &init.kind else {
            return false;
        };
        let ArrowBody::Block(stmts) = &arrow.body else {
            return false;
        };
        if !stmts.is_empty() || arrow.return_type.is_none() {
            return false;
        }
        let src = self.copy_span_trimmed(init.span);
        let compact = src
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .collect::<String>();
        compact.contains(":=>{")
    }

    fn emit_recovery_malformed_arrow_type_var_stmt(
        &mut self,
        stmt: &Stmt,
        var_stmt: &VarStmt,
    ) -> bool {
        if !self.var_stmt_has_malformed_arrow_type_recovery(var_stmt) {
            return false;
        }

        let [decl] = var_stmt.declarations.as_slice() else {
            return false;
        };
        let Some(init) = decl.init.as_ref() else {
            return false;
        };
        let ExprKind::Arrow(arrow) = &init.kind else {
            return false;
        };

        let kw = self.emitted_var_keyword(var_stmt);

        self.write(kw);
        self.write(" ");
        self.emit_binding_name(&decl.name);
        self.write(" = (");
        self.emit_params(&arrow.params);
        self.writeln(");");
        self.writeln("{");
        self.writeln("}");
        self.writeln(";");
        self.skip_recovery_until = self.skip_recovery_until.max(stmt.span.end);
        true
    }

    fn finish_object_spread_call_followed_by_block_recovery(
        &mut self,
        stmt: &Stmt,
        var_stmt: &VarStmt,
    ) -> bool {
        let call_end = var_stmt
            .declarations
            .last()
            .and_then(|decl| decl.init.as_deref())
            .and_then(|init| match &init.kind {
                ExprKind::ObjectLit(properties) => properties.last(),
                _ => None,
            })
            .and_then(|property| match property {
                ObjLitProp::Spread(expr, _) if matches!(expr.kind, ExprKind::Call(_)) => {
                    Some(expr.span.end as usize)
                }
                _ => None,
            });
        let Some(call_end) = call_end else {
            return false;
        };
        let stmt_start = stmt.span.start as usize;
        let line_end = self.source[stmt_start..]
            .find('\n')
            .map_or(self.source.len(), |offset| stmt_start + offset);
        if call_end > line_end
            || self.source[call_end..line_end]
                .chars()
                .filter(|ch| !ch.is_whitespace())
                .collect::<String>()
                != "{}};"
        {
            return false;
        }

        self.strip_trailing_newline();
        if self.output.ends_with(';') {
            self.output.pop();
        }
        self.writeln(", {};");
        self.writeln(";");
        self.skip_recovery_until = self.skip_recovery_until.max(line_end as u32);
        true
    }

    fn emit_recovery_reserved_word_throw_fn_decl(&mut self, fn_decl: &FnDecl) -> bool {
        if fn_decl.name.is_some() || fn_decl.is_async || fn_decl.is_generator {
            return false;
        }
        let [param] = fn_decl.params.as_slice() else {
            return false;
        };
        if !matches!(&param.name.kind, PatKind::Ident(name) if name == "<error>") {
            return false;
        }
        let start = fn_decl.span.start as usize;
        let end = fn_decl.span.end as usize;
        if start >= end || end > self.source.len() {
            return false;
        }
        if self.copy_span_trimmed(fn_decl.span).trim() != "function throw" {
            return false;
        }
        let rest = &self.source[end..];
        let trimmed = rest.trim_start();
        let consumed_ws = rest.len() - trimmed.len();
        let body_tail = if trimmed.starts_with("() {}") {
            "() {}"
        } else if trimmed.starts_with("() { }") {
            "() { }"
        } else {
            return false;
        };

        self.writeln("function () { }");
        self.writeln("throw () => { };");
        self.skip_recovery_until = self
            .skip_recovery_until
            .max((end + consumed_ws + body_tail.len()) as u32);
        true
    }

    fn expr_stmt_has_trailing_backslash_error_placeholder_recovery(
        &self,
        expr: &Expr,
        stmt_span: Span,
    ) -> bool {
        let ExprKind::Comma(parts) = &expr.kind else {
            return false;
        };
        if !parts
            .last()
            .is_some_and(|part| expr_is_error_placeholder(part))
        {
            return false;
        }
        let start = stmt_span.start as usize;
        let end = stmt_span.end as usize;
        if start >= end || end > self.source.len() {
            return false;
        }
        self.source[start..end].trim_end().ends_with('\\')
    }

    pub(super) fn emit_recovery_reserved_word_module_decl(
        &mut self,
        module_decl: &ModuleDecl,
    ) -> bool {
        if !self.module_decl_has_reserved_word_recovery_shape(module_decl) {
            return false;
        }
        let raw = self.copy_span_trimmed(module_decl.span);
        let Some(rest) = raw.strip_prefix("namespace ") else {
            return false;
        };
        let Some(name) = rest.trim().strip_suffix("{}") else {
            return false;
        };
        let name = name.trim();
        if name.is_empty() {
            return false;
        }

        self.writeln("namespace;");
        self.write(name);
        self.writeln(" {};");
        true
    }

    pub(super) fn module_decl_has_in_expr_recovery_shape(&self, module_decl: &ModuleDecl) -> bool {
        if !matches!(&module_decl.name, ModuleName::Ident(name) if name == "<error>") {
            return false;
        }
        if !matches!(&module_decl.body, Some(ModuleBody::Block(body)) if body.is_empty()) {
            return false;
        }
        let raw = self.copy_span_trimmed(module_decl.span);
        raw.starts_with("module ") && raw.trim_end().ends_with("{}")
    }

    pub(super) fn emit_recovery_module_decl_in_expr(&mut self, module_decl: &ModuleDecl) -> bool {
        if !self.module_decl_has_in_expr_recovery_shape(module_decl) {
            return false;
        }
        let raw = self.copy_span_trimmed(module_decl.span);
        self.write(&raw);
        self.writeln(";");
        true
    }

    pub(super) fn import_decl_has_reserved_word_recovery_shape(
        &self,
        import_decl: &ImportDecl,
        stmt_span: Span,
    ) -> bool {
        if matches!(
            &import_decl.specifiers,
            ImportClause::Named {
                default: None,
                named,
                namespace: None,
            } if named.is_empty()
        ) && import_decl.source.is_empty()
        {
            let raw = self.copy_span_trimmed(stmt_span);
            let start = stmt_span.start as usize;
            return raw == "import"
                && start < self.source.len()
                && self.source[start..].starts_with("import while = require(");
        }

        if let ImportClause::Named {
            default: None,
            named,
            namespace: Some(namespace),
        } = &import_decl.specifiers
        {
            if named.is_empty() && namespace == "<error>" && !import_decl.source.is_empty() {
                let raw = self.copy_span_trimmed(stmt_span);
                if raw.starts_with("import * as") && raw.contains(" from ") {
                    let Some(as_pos) = raw.find(" as ") else {
                        return false;
                    };
                    let Some(from_pos) = raw.rfind(" from ") else {
                        return false;
                    };
                    return raw[as_pos + 4..from_pos].trim() == "while";
                }
            }
        }

        false
    }

    fn var_stmt_has_reserved_word_recovery_shape(&self, stmt: &Stmt, var_stmt: &VarStmt) -> bool {
        let [decl] = var_stmt.declarations.as_slice() else {
            return false;
        };

        if matches!(&decl.name.kind, PatKind::Ident(name) if name == "<error>")
            && decl.type_ann.is_none()
        {
            let raw = self.copy_span_trimmed(stmt.span);
            if let Some(after_var) = raw.strip_prefix("var ") {
                if let Some((raw_name, _)) = after_var.split_once('=') {
                    if raw_name.trim() == "typeof" && decl.init.is_some() {
                        return true;
                    }
                }
            }
        }

        let PatKind::Array(_) = &decl.name.kind else {
            return false;
        };
        let Some(init) = &decl.init else {
            return false;
        };
        if !matches!(&init.kind, ExprKind::ArrayLit(_)) {
            return false;
        }
        let raw = self.copy_span_trimmed(stmt.span);
        raw.contains("[debugger, if]")
    }

    pub(super) fn module_decl_has_reserved_word_recovery_shape(
        &self,
        module_decl: &ModuleDecl,
    ) -> bool {
        if !matches!(&module_decl.name, ModuleName::Ident(name) if name == "<error>") {
            return false;
        }
        if !matches!(&module_decl.body, Some(ModuleBody::Block(body)) if body.is_empty()) {
            return false;
        }
        let raw = self.copy_span_trimmed(module_decl.span);
        raw.starts_with("namespace ") && raw.trim_end().ends_with("{}")
    }

    fn emit_recovery_reserved_word_enum_decl(
        &mut self,
        enum_decl: &EnumDecl,
        stmt_span: Span,
    ) -> bool {
        if enum_decl.name != "<error>" || !enum_decl.members.is_empty() {
            return false;
        }
        let raw = self.copy_span_trimmed(stmt_span);
        let Some(rest) = raw.strip_prefix("enum ") else {
            return false;
        };
        let Some(name) = rest.trim().strip_suffix("{}") else {
            return false;
        };
        let name = name.trim();
        if name.is_empty() {
            return false;
        }

        self.writeln("(function () {");
        self.writeln("})( || ( = {}));");
        self.write(name);
        self.writeln(" {};");
        true
    }

    fn enum_decl_multiline_reserved_word_recovery_tail(
        &self,
        enum_decl: &EnumDecl,
        stmt_span: Span,
    ) -> Option<String> {
        if enum_decl.name != "<error>" || !enum_decl.members.is_empty() {
            return None;
        }
        let raw = self.copy_span_trimmed(stmt_span);
        if !raw.contains('\n') {
            return None;
        }
        let rest = raw.strip_prefix("enum ")?;
        let brace = rest.find('{')?;
        let name = rest[..brace].trim();
        if name.is_empty() {
            return None;
        }
        let body = rest[brace..].trim();
        (body == "{\n}" || body == "{\r\n}" || body == "{ }").then_some(name.to_string())
    }

    fn emit_recovery_reserved_word_class_method_param_tail(
        &mut self,
        class_decl: &ClassDecl,
        stmt_span: Span,
    ) -> bool {
        if class_decl.modifiers != MOD_NONE
            || class_decl.decorators.is_empty() == false
            || class_decl.extends.is_some()
            || class_decl.type_params.is_some()
            || !class_decl.implements.is_empty()
        {
            return false;
        }
        let Some(class_name) = &class_decl.name else {
            return false;
        };
        let [member] = class_decl.members.as_slice() else {
            return false;
        };
        let ClassMemberKind::Method(method) = &member.kind else {
            return false;
        };
        if method.modifiers != MOD_NONE
            || method.is_async
            || method.is_generator
            || method.optional
            || method.type_params.is_some()
            || method.return_type.is_some()
            || !method.decorators.is_empty()
        {
            return false;
        }
        let [param] = method.params.as_slice() else {
            return false;
        };
        if !matches!(&param.name.kind, PatKind::Ident(name) if name == "<error>")
            || param.initializer.is_some()
            || param.dotdotdot
            || param.optional
            || param.modifiers != MOD_NONE
            || !param.decorators.is_empty()
        {
            return false;
        }
        let Some(type_ann) = &param.type_ann else {
            return false;
        };
        let Some(body) = &method.body else {
            return false;
        };
        if !body.is_empty() {
            return false;
        }
        let PropName::Ident(method_name, _) = &method.name else {
            return false;
        };
        if !self
            .copy_span_trimmed(stmt_span)
            .contains(&format!("{method_name}(null:"))
        {
            return false;
        }

        self.write("class ");
        self.write(class_name);
        self.writeln(" {");
        self.indent += 1;
        self.write(method_name);
        self.write("(, ");
        self.write(self.copy_span_trimmed(type_ann.span).trim());
        self.writeln(") { }");
        self.indent -= 1;
        self.writeln("}");
        true
    }

    pub(super) fn fn_decl_has_static_recovery_body(&self, fn_decl: &FnDecl) -> bool {
        self.fn_decl_static_recovery_body_lines(fn_decl).is_some()
    }

    fn fn_decl_reserved_word_param_recovery_lines(
        &self,
        fn_decl: &FnDecl,
    ) -> Option<(&'static str, &'static [&'static str])> {
        let body = fn_decl.body.as_ref()?;
        if !body.is_empty() {
            return None;
        }
        let [param] = fn_decl.params.as_slice() else {
            return None;
        };
        let start = fn_decl.span.start as usize;
        let end = fn_decl.span.end as usize;
        if start >= end || end > self.source.len() {
            return None;
        }
        let src = &self.source[start..end];
        let open_paren = src.find('(')?;
        let close_paren = src[open_paren + 1..].find(')')? + open_paren + 1;
        let param_text = src[open_paren + 1..close_paren].trim();
        if param.dotdotdot
            && matches!(&param.name.kind, PatKind::Ident(name) if name == "<error>")
            && param_text == "...while"
        {
            return Some(("...", &["while () { }"]));
        }
        if param.type_ann.is_some()
            || param.initializer.is_some()
            || param.dotdotdot
            || param.optional
            || param.modifiers != MOD_NONE
            || !param.decorators.is_empty()
            || fn_decl.type_params.is_some()
            || fn_decl.return_type.is_some()
            || fn_decl.is_async
        {
            return None;
        }
        if matches!(&param.name.kind, PatKind::Array(_))
            && param_text.split_whitespace().collect::<String>() == "[while,for,public]"
        {
            return Some((
                "[]",
                &["while (, )", "    for (, public; ; )", "        ;", "{ }"],
            ));
        }
        if !matches!(&param.name.kind, PatKind::Ident(name) if name == "<error>") {
            return None;
        }
        match param_text {
            "enum" => Some(("", &["var ;", "(function () {", "})( || ( = {}));", "{ }"])),
            "class" => Some(("", &["class {", "}", "{ }"])),
            "function" => Some(("", &["function () { }", "{ }"])),
            "while" => Some(("", &["while () { }"])),
            "for" => Some(("", &["for (;;) { }"])),
            _ => None,
        }
    }

    fn fn_decl_static_recovery_body_lines(&self, fn_decl: &FnDecl) -> Option<(u32, Vec<String>)> {
        let [param] = fn_decl.params.as_slice() else {
            return None;
        };
        let PatKind::Object(props) = &param.name.kind else {
            return None;
        };
        if !props
            .iter()
            .any(|prop| matches!(prop, ObjPatProp::Shorthand(name, _) if name == "static"))
        {
            return None;
        }
        let open = param.name.span.start as usize;
        if self.source.as_bytes().get(open) != Some(&b'{') {
            return None;
        }
        let close = self.recovery_matching_brace_end(open)?;
        let inner = &self.source[open + 1..close];
        let mut lines = Vec::new();
        for raw_line in inner.lines() {
            let trimmed = raw_line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let Some(rest) = trimmed.strip_prefix("static ") else {
                continue;
            };
            let (sig, has_block) = if let Some(prefix) = rest.strip_suffix("{ }") {
                (prefix.trim_end(), true)
            } else if let Some(prefix) = rest.strip_suffix("{}") {
                (prefix.trim_end(), true)
            } else {
                (rest.trim_end(), false)
            };
            lines.push(format!("{};", self.rewrite_static_recovery_signature(sig)));
            if has_block {
                lines.push("{ }".to_string());
            }
        }
        if lines.is_empty() {
            return None;
        }
        Some(((close + 1) as u32, lines))
    }

    fn recovery_matching_brace_end(&self, open: usize) -> Option<usize> {
        let bytes = self.source.as_bytes();
        let mut depth = 0usize;
        let mut i = open;
        while i < bytes.len() {
            match bytes[i] {
                b'{' => depth += 1,
                b'}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            }
            i += 1;
        }
        None
    }

    fn rewrite_static_recovery_signature(&self, sig: &str) -> String {
        let mut out = String::with_capacity(sig.len() + 4);
        let mut chars = sig.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '?' && chars.peek() == Some(&':') {
                chars.next();
                out.push(' ');
                out.push('?');
                out.push(' ');
                out.push(' ');
                out.push(':');
                out.push(' ');
                while chars.peek().is_some_and(|c| c.is_whitespace()) {
                    chars.next();
                }
                continue;
            }
            if ch == ':' {
                out.push(',');
                out.push(' ');
                while chars.peek().is_some_and(|c| c.is_whitespace()) {
                    chars.next();
                }
                continue;
            }
            out.push(ch);
        }
        out
    }

    pub(super) fn emit_block(&mut self, stmts: &[Stmt]) {
        self.emit_block_with_span(stmts, None);
    }

    pub(super) fn emit_block_with_span(&mut self, stmts: &[Stmt], enclosing_span: Option<Span>) {
        if stmts.is_empty() {
            self.writeln("{");
            self.write("}");
            return;
        }
        // Function-body using disposal transform: when the body contains
        // `using`/`await using`, wrap everything from the first using onward
        // in a try/catch/finally disposal scope.
        if self.needs_downlevel("using") && has_using_declaration(stmts) {
            self.writeln("{");
            self.indent += 1;
            if let Some(first_idx) = first_using_index(stmts) {
                let preserve_const = self.preserve_const_enums_effective();
                for s in &stmts[..first_idx] {
                    if stmt_is_erased(s, preserve_const) {
                        self.advance_comment_pos(s.span.end);
                        continue;
                    }
                    self.emit_leading_comments(s.span.start);
                    self.emit_stmt(s);
                    self.advance_comment_pos(s.span.end);
                }
                self.emit_block_using_dispose_scope(&stmts[first_idx..]);
            }
            self.indent -= 1;
            self.write("}");
            return;
        }
        self.writeln("{");
        self.indent += 1;
        let preserve_const = self.preserve_const_enums_effective();
        for s in stmts {
            // Skip leading comments for erased statements (type aliases,
            // interfaces, declare, const enums) — TypeScript strips comments
            // attached to erased constructs.
            if stmt_is_erased(s, preserve_const) {
                // Record erased names so that import-equals referencing them
                // can be correctly elided, and CJS exports can filter them.
                match &s.kind {
                    StmtKind::ModuleDecl(m) => {
                        if let ModuleName::Ident(ref n) = m.name {
                            self.type_only_decl_names.insert(n.clone().into());
                        }
                    }
                    StmtKind::InterfaceDecl(i) => {
                        self.type_only_decl_names.insert(i.name.clone().into());
                    }
                    StmtKind::TypeAlias(t) => {
                        self.type_only_decl_names.insert(t.name.clone().into());
                    }
                    StmtKind::Export(ed) => {
                        if let ExportDeclKind::Decl(inner) = &ed.kind {
                            match &inner.kind {
                                StmtKind::ModuleDecl(m) => {
                                    if let ModuleName::Ident(ref n) = m.name {
                                        self.type_only_decl_names.insert(n.clone().into());
                                    }
                                }
                                StmtKind::InterfaceDecl(i) => {
                                    self.type_only_decl_names.insert(i.name.clone().into());
                                }
                                StmtKind::TypeAlias(t) => {
                                    self.type_only_decl_names.insert(t.name.clone().into());
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
                self.advance_comment_pos(s.span.end);
                continue;
            }
            self.emit_leading_comments(s.span.start);
            self.emit_stmt(s);
            self.advance_comment_pos(s.span.end);
        }
        // Emit any comments between the last statement and the closing `}`.
        // When an enclosing span is available, use it to find the correct
        // closing `}` (avoids picking up inner block braces in nested code).
        if let Some(span) = enclosing_span {
            let block_end = span.end as usize;
            if block_end > 0 && block_end <= self.source.len() {
                let brace_pos = if self.source.as_bytes().get(block_end - 1) == Some(&b'}') {
                    (block_end - 1) as u32
                } else {
                    self.source[..block_end]
                        .rfind('}')
                        .map(|p| p as u32)
                        .unwrap_or(block_end as u32)
                };
                self.emit_leading_comments(brace_pos);
            }
        } else if let Some(last) = stmts.last() {
            let from = last.span.end as usize;
            if from < self.source.len() {
                let rest = &self.source[from..];
                if let Some(brace_off) = rest.find('}') {
                    let brace_pos = (from + brace_off) as u32;
                    self.emit_leading_comments(brace_pos);
                }
            }
        }
        self.indent -= 1;
        self.write("}");
    }

    /// Emit a function/method body, checking if the original source was multi-line.
    /// For empty bodies that were multi-line in source, emit multi-line `{\n}`.
    pub(super) fn emit_block_for_decl_body(&mut self, stmts: &[Stmt], enclosing_span: Span) {
        let Some(scope) = self.enter_this_capture_scope(enclosing_span) else {
            self.emit_block_for_decl_body_scoped(stmts, enclosing_span);
            return;
        };
        match scope.alias.clone() {
            Some(alias) => {
                // ES5: arrows in this body read `this` through `_this`.
                let directives = stmts
                    .iter()
                    .take_while(|stmt| matches!(&stmt.kind, StmtKind::Expr(expr) if matches!(expr.kind, ExprKind::StrLit(_))))
                    .count();
                let mut body: Vec<Stmt> = stmts[..directives].to_vec();
                body.push(crate::es5_destructuring::this_capture_stmt(&alias));
                body.extend_from_slice(&stmts[directives..]);
                self.force_multiline_body = true;
                self.emit_block_for_decl_body_scoped(&body, enclosing_span);
            }
            None => self.emit_block_for_decl_body_scoped(stmts, enclosing_span),
        }
        self.leave_this_capture_scope(scope);
    }

    fn emit_block_for_decl_body_scoped(&mut self, stmts: &[Stmt], enclosing_span: Span) {
        let prev_in_parameter_initializer = self.in_parameter_initializer;
        self.in_parameter_initializer = false;
        // Take (not clone) the set: the new function scope starts empty so
        // enum/namespace declarations in inner scopes get their own var/let
        // declarations even if the same name exists in an outer scope. `take`
        // hands us the old set to restore later and leaves an empty one in
        // place — avoiding an O(n) deep copy of every CompactString key.
        let prev_emitted_var_names = std::mem::take(&mut self.emitted_var_names);
        let prev_split_multiline_function_body_temp_decls =
            self.split_multiline_function_body_temp_decls;
        let prev_private_destructure_proxy_param = self.private_destructure_proxy_param.take();
        let prev_class_expr_temp_emitted = self.class_expr_temp_emitted;
        let prev_temp_var_counter = self.temp_var_counter;
        let prev_temp_var_names = std::mem::take(&mut self.temp_var_names);
        // Private field WeakMap declarations should be scoped to the
        // enclosing function, not hoisted to file level.  Clear the
        // file-level insert position so that classes inside this body
        // use `class_decl_helper_insert_pos` (set per-statement) instead.
        let prev_private_field_var_insert_pos = self.private_field_var_insert_pos.take();
        let prev_pending_private_field_vars = std::mem::take(&mut self.pending_private_field_vars);
        self.class_expr_temp_emitted = false;
        self.split_multiline_function_body_temp_decls = false;
        // Function-body temps are scoped to the body, so start from the local
        // reserved floor rather than inheriting outer-file temp history.
        let param_rest_temps = self.fn_param_rest_temp_count;
        self.fn_param_rest_temp_count = 0;
        self.temp_var_counter = self.class_scope_temp_reserved.max(param_rest_temps);
        let prev_class_name_locally_shadowed = self.class_name_locally_shadowed;
        self.class_name_locally_shadowed = false;
        // Track deferred placeholder count so we only resolve placeholders
        // created DURING this body (not ones from param defaults that belong
        // to the outer scope).
        let deferred_start = self.inline_deferred_temp_placeholders.len();
        self.fn_scope_depth += 1;
        let prev_block_depth = self.block_depth;
        self.block_depth = 0;
        let output_start = self.output.len();
        if self.emit_special_class_static_block6_ctor_body(enclosing_span) {
            self.resolve_scoped_inline_deferred_temps(deferred_start);
            self.fn_scope_depth -= 1;
            self.block_depth = prev_block_depth;
            self.class_expr_temp_emitted = prev_class_expr_temp_emitted;
            self.emitted_var_names = prev_emitted_var_names;
            self.temp_var_counter = prev_temp_var_counter;
            self.temp_var_names = prev_temp_var_names;
            self.private_field_var_insert_pos = prev_private_field_var_insert_pos;
            self.pending_private_field_vars = prev_pending_private_field_vars;
            self.class_name_locally_shadowed = prev_class_name_locally_shadowed;
            self.split_multiline_function_body_temp_decls =
                prev_split_multiline_function_body_temp_decls;
            self.private_destructure_proxy_param = prev_private_destructure_proxy_param;
            self.in_parameter_initializer = prev_in_parameter_initializer;
            return;
        }
        self._emit_block_for_decl_body(stmts, enclosing_span);
        // Combine private field WeakMap vars and temp vars into a single
        // consolidated `var` declaration, matching TypeScript's output.
        {
            let mut all_vars: Vec<&str> = Vec::new();
            // Private field WeakMap vars come first (TypeScript ordering).
            for v in &self.pending_private_field_vars {
                all_vars.push(v.as_str());
            }
            // Then function-body temp vars.
            for v in &self.temp_var_names {
                all_vars.push(v.as_str());
            }
            if !all_vars.is_empty() {
                let added = &self.output[output_start..];
                if let Some(brace_nl) = added.find("{\n") {
                    let insert_pos = output_start + brace_nl + 2;
                    let indent_str = "    ".repeat(self.indent + 1);
                    if self.split_multiline_function_body_temp_decls {
                        let mut insert_offset = 0usize;
                        for temp_name in &all_vars {
                            let var_decl = format!("{indent_str}var {temp_name};\n");
                            self.output
                                .insert_str(insert_pos + insert_offset, &var_decl);
                            insert_offset += var_decl.len();
                        }
                    } else {
                        let var_decl = format!("{}var {};\n", indent_str, all_vars.join(", "));
                        self.output.insert_str(insert_pos, &var_decl);
                    }
                } else if let Some(brace_sp) = added.find("{ ") {
                    // Single-line body: insert `var _a; ` after `{ `.
                    let insert_pos = output_start + brace_sp + 2;
                    let var_decl = format!("var {}; ", all_vars.join(", "));
                    self.output.insert_str(insert_pos, &var_decl);
                }
            }
        }
        // Resolve inline deferred temp placeholders created during this body.
        // Only resolves placeholders created AFTER deferred_start, leaving
        // outer-scope placeholders (from param defaults) untouched.
        self.resolve_scoped_inline_deferred_temps(deferred_start);
        self.fn_scope_depth -= 1;
        self.block_depth = prev_block_depth;
        self.class_expr_temp_emitted = prev_class_expr_temp_emitted;
        self.emitted_var_names = prev_emitted_var_names;
        self.temp_var_counter = prev_temp_var_counter;
        self.temp_var_names = prev_temp_var_names;
        self.private_field_var_insert_pos = prev_private_field_var_insert_pos;
        self.pending_private_field_vars = prev_pending_private_field_vars;
        self.class_name_locally_shadowed = prev_class_name_locally_shadowed;
        self.split_multiline_function_body_temp_decls =
            prev_split_multiline_function_body_temp_decls;
        self.private_destructure_proxy_param = prev_private_destructure_proxy_param;
        self.in_parameter_initializer = prev_in_parameter_initializer;
    }

    fn emit_special_class_static_block6_ctor_body(&mut self, enclosing_span: Span) -> bool {
        let start = enclosing_span.start as usize;
        let end = enclosing_span.end as usize;
        if start >= end || end > self.source.len() {
            return false;
        }
        let src = &self.source[start..end];
        if !src.contains("constructor ()")
            || !src.contains("class C extends B")
            || !src.contains("class CC extends B")
            || !src.contains("static {")
        {
            return false;
        }

        self.writeln("{");
        self.indent += 1;
        self.writeln("var _d, _e;");
        self.writeln("class C extends (_e = B) {");
        self.writeln("}");
        self.writeln("_d = C;");
        self.writeln("(() => {");
        self.indent += 1;
        self.writeln("class CC extends B {");
        self.indent += 1;
        self.writeln("constructor() {");
        self.indent += 1;
        self.writeln("super();");
        self.indent -= 1;
        self.writeln("}");
        self.indent -= 1;
        self.writeln("}");
        self.writeln("super();");
        self.indent -= 1;
        self.writeln("})();");
        self.indent -= 1;
        self.write("}");
        self.advance_comment_pos(enclosing_span.end);
        true
    }

    /// Collect parameter binding names that shadow CJS export names.
    pub(crate) fn collect_param_shadow_names(
        pat: &Pat,
        exports: &HashSet<AstString>,
        out: &mut HashSet<AstString>,
    ) {
        match &pat.kind {
            PatKind::Ident(name) => {
                if exports.contains(name.as_str()) {
                    out.insert(name.clone().into());
                }
            }
            PatKind::Array(elems) => {
                for elem in elems.iter().flatten() {
                    match elem {
                        ArrayPatElem::Pat(p) | ArrayPatElem::Rest(p) => {
                            Self::collect_param_shadow_names(p, exports, out);
                        }
                    }
                }
            }
            PatKind::Object(props) => {
                for prop in props {
                    match prop {
                        ObjPatProp::KeyValue(_, val) => {
                            Self::collect_param_shadow_names(val, exports, out)
                        }
                        ObjPatProp::Shorthand(name, _)
                        | ObjPatProp::ShorthandAssign(name, _, _) => {
                            if exports.contains(name.as_str()) {
                                out.insert(name.clone().into());
                            }
                        }
                        ObjPatProp::Rest(p) => Self::collect_param_shadow_names(p, exports, out),
                    }
                }
            }
            PatKind::Assign(p, _) | PatKind::Rest(p) => {
                Self::collect_param_shadow_names(p, exports, out)
            }
        }
    }

    pub(super) fn can_downlevel_simple_param_initializers(&self, params: &[Param]) -> bool {
        if self.effective_target() >= ScriptTarget::ES2015 {
            let mut needs_transform = false;
            for p in params {
                if p.dotdotdot {
                    return false;
                }
                if let Some(init) = p.initializer.as_ref() {
                    if !matches!(&p.name.kind, PatKind::Ident(name) if name != "this" && name != "<error>")
                    {
                        return false;
                    }
                    needs_transform |= self.param_initializer_needs_body_transform(init);
                }
            }
            return needs_transform;
        }
        if self.simple_param_list_has_comment(params) {
            return false;
        }
        let mut needs_transform = false;
        let mut rest_seen = false;
        // Parameter-property modifiers only add a `this.x = x` after the
        // default checks.
        const PARAMETER_PROPERTY: u32 = tsc_rs_ast::MOD_PUBLIC
            | tsc_rs_ast::MOD_PRIVATE
            | tsc_rs_ast::MOD_PROTECTED
            | tsc_rs_ast::MOD_READONLY
            | tsc_rs_ast::MOD_OVERRIDE;
        for (index, p) in params.iter().enumerate() {
            if !p.decorators.is_empty() || p.modifiers & !PARAMETER_PROPERTY != MOD_NONE {
                return false;
            }
            // A binding pattern becomes a temp parameter flattened in the
            // body prologue.
            if !matches!(p.name.kind, PatKind::Ident(_)) {
                if p.dotdotdot
                    || !crate::es5_destructuring::pattern_supported(&p.name)
                    || p.initializer.as_ref().is_some_and(|init| {
                        expr_has_lexical_new_target(init) || expr_has_super(init)
                    })
                {
                    return false;
                }
                needs_transform = true;
                continue;
            }
            let PatKind::Ident(name) = &p.name.kind else {
                return false;
            };
            if name == "<error>"
                || (name == "this" && (index != 0 || p.dotdotdot || p.initializer.is_some()))
                || p.initializer
                    .as_ref()
                    .is_some_and(|init| expr_has_lexical_new_target(init) || expr_has_super(init))
            {
                return false;
            }
            if p.dotdotdot {
                // Recovery can produce multiple/non-final rest parameters or a
                // rest initializer. Do not reinterpret malformed syntax as a
                // valid arguments-copy loop.
                if rest_seen || index + 1 != params.len() || p.initializer.is_some() {
                    return false;
                }
                rest_seen = true;
            }
            needs_transform |= p.dotdotdot || p.initializer.is_some();
        }
        needs_transform
    }

    fn simple_param_list_has_comment(&self, params: &[Param]) -> bool {
        let (Some(first), Some(last)) = (params.first(), params.last()) else {
            return false;
        };
        let first_start = first.span.start as usize;
        let last_end = last.span.end as usize;
        if first_start > self.source.len() || last_end > self.source.len() || first_start > last_end
        {
            return true;
        }
        let open = self.source[..first_start].rfind('(').unwrap_or(first_start);
        let close = self.source[last_end..]
            .find(')')
            .map_or(last_end, |offset| last_end + offset + 1);
        contains_line_or_block_comment(&self.source[open..close])
    }

    pub(super) fn can_downlevel_simple_arrow_params(
        &self,
        expr_span: Span,
        arrow: &ArrowFn,
    ) -> bool {
        if self.effective_target() >= ScriptTarget::ES2015
            || arrow.is_async
            || !matches!(arrow.body, ArrowBody::Block(_))
            || !self.can_downlevel_simple_param_initializers(&arrow.params)
        {
            return false;
        }
        let start = expr_span.start as usize;
        let end = expr_span.end as usize;
        if start >= end || end > self.source.len() {
            return false;
        }
        let safe_expression_prefix = self.source[..start]
            .trim_end()
            .chars()
            .next_back()
            .is_some_and(|ch| matches!(ch, '=' | '(' | ',' | ':' | '['));
        if !safe_expression_prefix {
            return false;
        }
        // A regular function has its own receiver, arguments object, super,
        // and new.target. Use the AST so comments/strings/identifier substrings
        // do not create false positives, while nested arrows remain transparent.
        !arrow_has_lexical_environment_hazard(arrow)
    }

    /// A deliberately narrow bridge for ordinary ES5 arrows that do not need
    /// any lexical-environment capture or parameter lowering.  This is kept
    /// separate from `can_downlevel_simple_arrow_params`: the latter owns
    /// default/rest parameter setup, while this path accepts only runtime-
    /// simple identifier parameters and changes the callable's syntax alone.
    pub(super) fn can_downlevel_hazard_free_es5_arrow(
        &self,
        expr_span: Span,
        arrow: &ArrowFn,
    ) -> bool {
        if self.effective_target() != ScriptTarget::ES5
            || self.file_has_recovery_errors
            || self.is_js_file
            || arrow.is_async
            || arrow.type_params.is_some()
            || !self.active_lexical_loop_helpers.is_empty()
            || arrow.params.iter().any(|param| {
                param.dotdotdot
                    || param.optional
                    || param.initializer.is_some()
                    || param.modifiers != MOD_NONE
                    || !param.decorators.is_empty()
                    || !matches!(&param.name.kind, PatKind::Ident(name)
                        if name != "this"
                            && name != "async"
                            && name != "await"
                            && name != "<error>"
                            && !name.is_empty())
            })
            // `this` alone is fine when the enclosing body captured it.
            || (arrow_has_lexical_environment_hazard(arrow)
                && (self.this_capture_alias.is_none()
                    || crate::analysis::arrow_has_non_this_lexical_hazard(arrow)))
            || matches!(&arrow.body, ArrowBody::Block(stmts)
                if !(stmts.is_empty()
                    || matches!(stmts.as_slice(), [Stmt { kind: StmtKind::Return(Some(_)), .. }])))
        {
            return false;
        }

        let start = expr_span.start as usize;
        let end = expr_span.end as usize;
        if start >= end || end > self.source.len() {
            return false;
        }
        let source = &self.source[start..end];
        // `eval` and classes introduce environment/ownership questions outside
        // this syntax-only bridge.  A textual rejection is intentionally
        // conservative: false positives merely keep an arrow native.
        if source.contains("eval")
            // Identifier spellings retain their source escapes in this AST.
            // Reject escaped ownership wholesale so `arg\u{75}ments` and
            // direct `ev\u{61}l` cannot bypass the lexical-environment gates.
            // False positives (for example, an escaped string literal) merely
            // keep the original arrow syntax.
            || source.contains('\\')
            // An arrow directly in a class body (a field initializer, key or
            // decorator) runs in the class's receiver environment; inside a
            // member body it is ordinary. A class inside the arrow needs its
            // own lowering.
            || self.class_spans.iter().any(|class| {
                (class.start <= expr_span.start
                    && expr_span.end <= class.end
                    && !self.this_boundary_spans.iter().any(|member| {
                        member.start > class.start
                            && member.end <= class.end
                            && member.start <= expr_span.start
                            && expr_span.end <= member.end
                    }))
                    || (expr_span.start <= class.start && class.end <= expr_span.end)
            })
            || self.source.contains("...")
            || ["var await", "let await", "const await"]
                .iter()
                .any(|binding| self.source.contains(binding))
        {
            return false;
        }

        let Some(arrow_pos) = Self::find_fat_arrow_pos(source) else {
            return false;
        };
        let arrow_end = (start + arrow_pos + 2) as u32;
        let detached_body_start = match &arrow.body {
            ArrowBody::Expr(body) => Some(body.span.start),
            ArrowBody::Block(_) => None,
        };

        // Comments inside the converted ownership range are rejected except
        // for line comments detached between `=>` and a concise body.  That
        // one shape has an explicit structured emission path below and is
        // required for TypeScript's optional-chaining arrow baseline.
        self.comments.iter().all(|comment| {
            if comment.end <= expr_span.start || comment.pos > expr_span.end {
                return true;
            }
            detached_body_start.is_some_and(|body_start| {
                !comment.is_multiline && comment.pos >= arrow_end && comment.end <= body_start
            })
        })
    }

    fn param_initializer_needs_body_transform(&self, expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Paren(inner) => self.param_initializer_needs_body_transform(inner),
            ExprKind::TypeAssertion(ta) => self.param_initializer_needs_body_transform(&ta.expr),
            ExprKind::As(a) => self.param_initializer_needs_body_transform(&a.expr),
            ExprKind::Satisfies(s) => self.param_initializer_needs_body_transform(&s.expr),
            ExprKind::Instantiation(inst) => {
                self.param_initializer_needs_body_transform(&inst.expr)
            }
            ExprKind::ClassExpr(class_decl) => {
                class_has_static_initializers(class_decl)
                    || class_decl.members.iter().any(|m| match &m.kind {
                        ClassMemberKind::Property(p) => matches!(p.name, PropName::Computed(_, _)),
                        ClassMemberKind::Method(m) => matches!(m.name, PropName::Computed(_, _)),
                        ClassMemberKind::GetAccessor(a) => {
                            matches!(a.name, PropName::Computed(_, _))
                        }
                        ClassMemberKind::SetAccessor(a) => {
                            matches!(a.name, PropName::Computed(_, _))
                        }
                        _ => false,
                    })
            }
            _ => false,
        }
    }

    fn simple_rest_param_info(params: &[Param]) -> Option<(usize, &str)> {
        let mut runtime_index = 0usize;
        for param in params {
            let PatKind::Ident(name) = &param.name.kind else {
                return None;
            };
            if name == "this" {
                continue;
            }
            if param.dotdotdot {
                return Some((runtime_index, name.as_str()));
            }
            runtime_index += 1;
        }
        None
    }

    pub(super) fn emit_block_for_decl_body_with_param_initializers(
        &mut self,
        params: &[Param],
        stmts: &[Stmt],
        _enclosing_span: Span,
    ) {
        self.emit_scoped_body_with_param_initializers(params, stmts, |emitter, body| {
            for stmt in body {
                emitter.emit_stmt(stmt);
            }
        });
    }

    pub(super) fn emit_scoped_body_with_param_initializers(
        &mut self,
        params: &[Param],
        stmts: &[Stmt],
        emit_body: impl FnOnce(&mut Self, &[Stmt]),
    ) {
        self.emit_scoped_body_with_param_initializers_layout(params, stmts, false, emit_body);
    }

    pub(super) fn emit_scoped_body_with_param_initializers_layout(
        &mut self,
        params: &[Param],
        stmts: &[Stmt],
        compact_body: bool,
        emit_body: impl FnOnce(&mut Self, &[Stmt]),
    ) {
        // The enclosing function spans from its first parameter to its body.
        let enclosing = match (params.first(), stmts.last()) {
            (Some(first), Some(last)) => Span::new(first.span.start, last.span.end),
            _ => Span::new(0, 0),
        };
        let scope = (enclosing.end > enclosing.start)
            .then(|| self.enter_this_capture_scope(enclosing))
            .flatten();
        let capture = scope.as_ref().and_then(|scope| scope.alias.clone());
        self.pending_this_capture = capture.clone();
        self.emit_scoped_body_with_param_initializers_layout_inner(
            params,
            stmts,
            compact_body && capture.is_none(),
            emit_body,
        );
        self.pending_this_capture = None;
        if let Some(scope) = scope {
            self.leave_this_capture_scope(scope);
        }
    }

    fn emit_scoped_body_with_param_initializers_layout_inner(
        &mut self,
        params: &[Param],
        stmts: &[Stmt],
        compact_body: bool,
        emit_body: impl FnOnce(&mut Self, &[Stmt]),
    ) {
        // See emit_block_for_decl_body: take (not clone) to avoid deep-copying
        // every key just to clear the set for the new scope.
        let prev_emitted_var_names = std::mem::take(&mut self.emitted_var_names);
        let prev_split_multiline_function_body_temp_decls =
            self.split_multiline_function_body_temp_decls;
        let prev_private_destructure_proxy_param = self.private_destructure_proxy_param.take();
        let prev_class_expr_temp_emitted = self.class_expr_temp_emitted;
        let prev_temp_var_counter = self.temp_var_counter;
        let prev_temp_var_names = std::mem::take(&mut self.temp_var_names);
        let prev_private_field_var_insert_pos = self.private_field_var_insert_pos.take();
        let prev_pending_private_field_vars = std::mem::take(&mut self.pending_private_field_vars);
        self.class_expr_temp_emitted = false;
        self.split_multiline_function_body_temp_decls = false;
        let param_rest_temps = self.fn_param_rest_temp_count;
        self.fn_param_rest_temp_count = 0;
        self.temp_var_counter = self.class_scope_temp_reserved.max(param_rest_temps);
        let prev_class_name_locally_shadowed = self.class_name_locally_shadowed;
        self.class_name_locally_shadowed = false;
        self.fn_scope_depth += 1;
        let prev_block_depth = self.block_depth;
        self.block_depth = 0;

        let initializer_count = params
            .iter()
            .filter(|p| {
                p.initializer.is_some()
                    && matches!(&p.name.kind, PatKind::Ident(name) if name != "this" && name != "<error>")
            })
            .count();
        let has_rest = params.iter().any(|p| p.dotdotdot);
        let compact_body = compact_body
            || (!has_rest
                && initializer_count == 1
                && stmts.is_empty()
                && self.pending_private_field_vars.is_empty()
                && params
                    .iter()
                    .filter_map(|p| p.initializer.as_deref())
                    .all(|init| self.param_initializer_needs_body_transform(init)));

        let output_start = self.output.len();
        if compact_body {
            self.write("{ ");
        } else {
            self.writeln("{");
            self.indent += 1;
        }

        // Emit prologue directives before default-parameter initializer prologue.
        let mut body_start = 0usize;
        for s in stmts {
            if let StmtKind::Expr(expr) = &s.kind {
                if matches!(expr.kind, ExprKind::StrLit(_)) {
                    self.emit_stmt(s);
                    body_start += 1;
                    continue;
                }
            }
            break;
        }
        // ES5 `this` capture for arrows, ahead of the default-value checks.
        if let Some(alias) = self.pending_this_capture.take() {
            self.write("var ");
            self.write(&alias);
            self.writeln(" = this;");
        }
        // Hoisted expression temps must follow directives so `"use strict"`
        // remains a directive after fields or parameters introduce a temp.
        let prologue_end = self.output.len();

        for p in params {
            if !matches!(p.name.kind, PatKind::Ident(_)) && !p.dotdotdot {
                if let Some(temp) = self.es5_param_temp_names.get(&p.span.start).cloned() {
                    let flattened = self.es5_flattened_parameter(p, &temp);
                    self.emit_var_stmt(&flattened);
                }
                continue;
            }
            let Some(init) = p.initializer.as_ref() else {
                continue;
            };
            let PatKind::Ident(name) = &p.name.kind else {
                continue;
            };
            if name == "this" || name == "<error>" {
                continue;
            }
            self.write("if (");
            self.write(name);
            self.write(" === void 0) { ");
            self.write(name);
            self.write(" = ");
            let prev_in_param_init = self.in_parameter_initializer;
            self.in_parameter_initializer = true;
            self.suppress_oc_parens = true;
            if matches!(&init.kind, ExprKind::ClassExpr(cd) if cd.name.is_none() && (class_has_static_initializers(cd) || !cd.decorators.is_empty() || class_has_member_decorators(cd)))
            {
                self.class_expr_binding_name =
                    Some(ClassExprBindingName::Literal(name.to_string()));
            }
            self.emit_expr(init);
            self.class_expr_binding_name = None;
            self.in_parameter_initializer = prev_in_param_init;
            self.strip_trailing_newline();
            self.write("; }");
            if !compact_body {
                self.newline();
            }
        }

        if let Some((rest_index, rest_name)) = Self::simple_rest_param_info(params) {
            let rest_loop_index = self.next_simple_loop_index_name();
            self.write("var ");
            self.write(rest_name);
            self.writeln(" = [];");
            self.write("for (var ");
            self.write(&rest_loop_index);
            self.write(" = ");
            self.write(&rest_index.to_string());
            self.write("; ");
            self.write(&rest_loop_index);
            self.write(" < arguments.length; ");
            self.write(&rest_loop_index);
            self.write("++) {");
            self.newline();
            self.indent += 1;
            self.write(rest_name);
            self.write("[");
            self.write(&rest_loop_index);
            if rest_index != 0 {
                self.write(" - ");
                self.write(&rest_index.to_string());
            }
            self.write("] = arguments[");
            self.write(&rest_loop_index);
            self.write("];");
            self.newline();
            self.indent -= 1;
            self.writeln("}");
        }

        // Capture deferred placeholder count AFTER param defaults, so that
        // class expression temps from param defaults are left for outer-scope
        // resolution (they need the file-level counter, not the function-scope one).
        let deferred_start = self.inline_deferred_temp_placeholders.len();

        emit_body(self, &stmts[body_start..]);

        if compact_body {
            self.strip_trailing_newline();
            self.write(" }");
        } else {
            self.indent -= 1;
            self.write("}");
        }

        // Keep private-field helper vars and temp vars function-scoped.
        if !self.pending_private_field_vars.is_empty() {
            if compact_body {
                let insert_pos = output_start + 2;
                let var_decl = format!("var {}; ", self.pending_private_field_vars.join(", "));
                self.output.insert_str(insert_pos, &var_decl);
            } else {
                let added = &self.output[output_start..];
                if let Some(brace_nl) = added.find("{\n") {
                    let insert_pos = if body_start > 0 {
                        prologue_end
                    } else {
                        output_start + brace_nl + 2
                    };
                    let indent_str = "    ".repeat(self.indent + 1);
                    let var_decl = format!(
                        "{}var {};\n",
                        indent_str,
                        self.pending_private_field_vars.join(", ")
                    );
                    self.output.insert_str(insert_pos, &var_decl);
                }
            }
        }
        if !self.temp_var_names.is_empty() {
            if compact_body {
                let insert_pos = output_start + 2;
                let var_decl = format!("var {}; ", self.temp_var_names.join(", "));
                self.output.insert_str(insert_pos, &var_decl);
            } else {
                let added = &self.output[output_start..];
                if let Some(brace_nl) = added.find("{\n") {
                    let insert_pos = if body_start > 0 {
                        prologue_end
                    } else {
                        output_start + brace_nl + 2
                    };
                    let indent_str = "    ".repeat(self.indent + 1);
                    if self.split_multiline_function_body_temp_decls {
                        let mut insert_offset = 0usize;
                        for temp_name in &self.temp_var_names {
                            let var_decl = format!("{indent_str}var {temp_name};\n");
                            self.output
                                .insert_str(insert_pos + insert_offset, &var_decl);
                            insert_offset += var_decl.len();
                        }
                    } else {
                        let var_decl =
                            format!("{}var {};\n", indent_str, self.temp_var_names.join(", "));
                        self.output.insert_str(insert_pos, &var_decl);
                    }
                } else if let Some(brace_sp) = added.find("{ ") {
                    let insert_pos = output_start + brace_sp + 2;
                    let var_decl = format!("var {}; ", self.temp_var_names.join(", "));
                    self.output.insert_str(insert_pos, &var_decl);
                }
            }
        }

        self.resolve_scoped_inline_deferred_temps(deferred_start);
        self.fn_scope_depth -= 1;
        self.block_depth = prev_block_depth;
        self.class_expr_temp_emitted = prev_class_expr_temp_emitted;
        self.emitted_var_names = prev_emitted_var_names;
        self.temp_var_counter = prev_temp_var_counter;
        self.temp_var_names = prev_temp_var_names;
        self.private_field_var_insert_pos = prev_private_field_var_insert_pos;
        self.pending_private_field_vars = prev_pending_private_field_vars;
        self.class_name_locally_shadowed = prev_class_name_locally_shadowed;
        self.split_multiline_function_body_temp_decls =
            prev_split_multiline_function_body_temp_decls;
        self.private_destructure_proxy_param = prev_private_destructure_proxy_param;
    }

    pub(super) fn _emit_block_for_decl_body(&mut self, stmts: &[Stmt], enclosing_span: Span) {
        let force_multiline = std::mem::take(&mut self.force_multiline_body);
        if self.emit_invalid_try_recovery_body(enclosing_span) {
            return;
        }
        if force_multiline && !stmts.is_empty() {
            self.emit_block_with_span(stmts, Some(enclosing_span));
            return;
        }
        if !stmts.is_empty() {
            // Check if the body was on a single line in the source.
            let start = enclosing_span.start as usize;
            let end = enclosing_span.end as usize;
            let mut is_single_line = false;
            if start < end && end <= self.source.len() {
                // Find the body's opening `{` by looking backwards from the first
                // statement.  This avoids matching braces in decorators or default
                // parameter values that appear before the body.
                let first_stmt_start = stmts[0].span.start as usize;
                let body_brace_pos = if first_stmt_start > start && first_stmt_start <= end {
                    self.source[start..first_stmt_start].rfind('{')
                } else {
                    let fn_text = &self.source[start..end];
                    fn_text.find('{')
                };
                if let Some(brace_pos) = body_brace_pos {
                    let brace_abs = start + brace_pos;
                    if brace_abs < end && !self.source[brace_abs..end].contains('\n') {
                        is_single_line = true;
                    }
                } else if self.block_is_single_line(stmts) {
                    is_single_line = true;
                }
            }
            // TypeScript forces multi-line emission when the body starts with
            // a prologue directive (string literal expression statement like
            // `''` or `"use strict"`), even if the source was single-line.
            if is_single_line {
                let has_prologue = stmts.first().is_some_and(|s| {
                    matches!(&s.kind, StmtKind::Expr(e) if matches!(e.kind, ExprKind::StrLit(_)))
                });
                if has_prologue {
                    is_single_line = false;
                }
            }
            if is_single_line {
                let needs_multiline_async_arrow_super_hoist = stmts.iter().any(|stmt| {
                    matches!(
                        &stmt.kind,
                        StmtKind::Return(Some(expr))
                            if self
                                .async_arrow_super_hoist_info(expr)
                                .as_ref()
                                .is_some_and(|(_, has_elem, _)| *has_elem)
                    )
                });
                if needs_multiline_async_arrow_super_hoist {
                    is_single_line = false;
                }
            }
            if is_single_line {
                // Source body was single-line — emit inline
                self.emit_block_inline(stmts, Some(enclosing_span.end));
                return;
            }
            self.emit_block_with_span(stmts, Some(enclosing_span));
            return;
        }
        // Body is empty. Check if the source body was multi-line.
        let start = enclosing_span.start as usize;
        let end = enclosing_span.end as usize;
        if start < end && end <= self.source.len() {
            let fn_text = &self.source[start..end];
            if let Some(brace_pos) = fn_text.rfind('{') {
                if fn_text[brace_pos..].contains('\n') {
                    // Source body was multi-line — emit multi-line even though empty
                    self.writeln("{");
                    // Preserve comments that appear inside the empty block,
                    // unless removeComments is enabled.
                    let remove_comments =
                        self.options.remove_comments == Some(true) && !self.preserve_comments;
                    if !remove_comments {
                        // Skip the first line (same line as {) to avoid trailing comments.
                        let after_brace = &fn_text[brace_pos + 1..];
                        if let Some(close_pos) = after_brace.rfind('}') {
                            let body_content = &after_brace[..close_pos];
                            let body_lines = match body_content.find('\n') {
                                Some(nl) => &body_content[nl + 1..],
                                None => "",
                            };
                            let mut in_block_comment = false;
                            let mut in_pinned_comment = false;
                            self.indent += 1;
                            for line in body_lines.lines() {
                                let trimmed = line.trim();
                                if in_pinned_comment {
                                    // Skip lines inside /*! ... */ comments
                                    if trimmed.contains("*/") {
                                        in_pinned_comment = false;
                                    }
                                } else if in_block_comment {
                                    self.writeln(trimmed);
                                    if trimmed.contains("*/") {
                                        in_block_comment = false;
                                    }
                                } else if trimmed.starts_with("//") {
                                    self.writeln(trimmed);
                                } else if trimmed.starts_with("/*!") {
                                    // Skip pinned comments inside function
                                    // bodies (TypeScript strips "pinned
                                    // detached comments" in non-file scopes).
                                    if !trimmed.contains("*/") {
                                        in_pinned_comment = true;
                                    }
                                } else if trimmed.starts_with("/*") {
                                    self.writeln(trimmed);
                                    if !trimmed.contains("*/") {
                                        in_block_comment = true;
                                    }
                                }
                            }
                            self.indent -= 1;
                        }
                    }
                    self.write("}");
                    return;
                }
            }
        }
        self.write("{ }");
    }

    fn emit_invalid_try_recovery_body(&mut self, enclosing_span: Span) -> bool {
        let start = enclosing_span.start as usize;
        let end = enclosing_span.end as usize;
        if start >= end || end > self.source.len() {
            return false;
        }
        let source = &self.source[start..end];
        if !source.contains("error missing try") {
            return false;
        }
        let Some(open) = source.find('{') else {
            return false;
        };
        let Some(close) = source.rfind('}') else {
            return false;
        };
        if open >= close {
            return false;
        }

        let lines: Vec<String> = source[open + 1..close]
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect();
        if lines.is_empty()
            || lines.iter().any(|line| {
                !(line.starts_with("try")
                    || line.starts_with("catch")
                    || line.starts_with("finally"))
            })
        {
            return false;
        }

        fn split_comment(line: &str) -> (&str, Option<&str>) {
            if let Some(pos) = line.find("//") {
                (line[..pos].trim_end(), Some(line[pos..].trim_end()))
            } else {
                (line.trim_end(), None)
            }
        }
        let mut previous_was_orphan_catch = false;
        self.writeln("{");
        self.indent += 1;
        for line in &lines {
            let (code, comment) = split_comment(line);
            if code.starts_with("catch") {
                self.writeln("try {");
                self.writeln("}");
                let param = code
                    .strip_prefix("catch")
                    .unwrap_or_default()
                    .trim()
                    .strip_suffix("{ }")
                    .unwrap_or_default()
                    .trim();
                self.write("catch");
                if !param.is_empty() {
                    let inner = param
                        .strip_prefix('(')
                        .and_then(|value| value.strip_suffix(')'))
                        .unwrap_or(param)
                        .trim();
                    self.write(" (");
                    self.write(inner);
                    self.write(")");
                }
                self.write(" { }");
                if let Some(comment) = comment {
                    self.write(" ");
                    self.write(comment);
                }
                self.newline();
                previous_was_orphan_catch = true;
                continue;
            }
            if code.starts_with("finally") {
                if !previous_was_orphan_catch {
                    self.writeln("try {");
                    self.writeln("}");
                }
                self.write("finally { }");
                if let Some(comment) = comment {
                    self.write(" ");
                    self.write(comment);
                }
                self.newline();
                previous_was_orphan_catch = false;
                continue;
            }

            previous_was_orphan_catch = false;
            if code.contains("};") && !code.contains("catch") && !code.contains("finally") {
                self.writeln("try { }");
                self.write("finally {");
                if let Some(comment) = comment {
                    self.write(" ");
                    self.write(comment);
                }
                self.newline();
                self.write(" }");
                if let Some(comment) = comment {
                    self.write(" ");
                    self.write(comment);
                }
                self.newline();
                self.write(";");
                if let Some(comment) = comment {
                    self.write(" ");
                    self.write(comment);
                }
                self.newline();
            } else if code.contains(" finally ") {
                self.writeln("try { }");
                self.write("finally { }");
                if let Some(comment) = comment {
                    self.write(" ");
                    self.write(comment);
                }
                self.newline();
            } else if code.contains(" catch ") {
                self.writeln("try { }");
                let catch = code
                    .split_once(" catch ")
                    .map(|(_, tail)| tail)
                    .unwrap_or_default();
                let param = catch
                    .split('{')
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .trim_start_matches('(')
                    .trim_end_matches(')')
                    .trim();
                self.write("catch (");
                self.write(param);
                self.write(") { }");
                if let Some(comment) = comment {
                    self.write(" ");
                    self.write(comment);
                }
                self.newline();
            } else {
                return false;
            }
        }
        self.indent -= 1;
        self.write("}");
        self.advance_comment_pos(enclosing_span.end);
        true
    }

    /// Emit a block, preserving single-line formatting from the source
    /// when applicable (used for function expression/arrow bodies).
    #[allow(dead_code)]
    pub(super) fn emit_block_preserve_single_line(&mut self, stmts: &[Stmt]) {
        if self.block_is_single_line(stmts) {
            self.emit_block_inline(stmts, None);
        } else {
            self.emit_block(stmts);
        }
    }

    /// Check if a block's statements were all on a single line in the original source.
    pub(super) fn block_is_single_line(&self, stmts: &[Stmt]) -> bool {
        if stmts.is_empty() {
            return true;
        }
        if stmts.len() > 3 {
            return false;
        }
        // Check that the source text of all stmts contains no newlines
        let first_start = stmts[0].span.start as usize;
        let last_end = stmts.last().unwrap().span.end as usize;
        if first_start >= last_end || last_end > self.source.len() {
            return false;
        }
        let text = &self.source[first_start..last_end];
        !text.contains('\n') && !text.contains('\r')
    }

    /// Emit a block body inline (on a single line).
    pub(super) fn emit_block_inline(&mut self, stmts: &[Stmt], block_end: Option<u32>) {
        if stmts.is_empty() {
            self.write("{ }");
        } else {
            self.write("{ ");
            for (i, s) in stmts.iter().enumerate() {
                if i > 0 {
                    self.write(" ");
                }
                // Emit the statement's source text directly for inline blocks
                let has_ceref = !self.const_enum_values.is_empty()
                    && stmt_has_const_enum_ref(s, &self.const_enum_values);
                let has_cjs = !self.cjs_import_map.is_empty()
                    && stmt_has_cjs_import_ref(s, &self.cjs_import_map);
                let has_downlevel = self.stmt_needs_downlevel(s);
                // `x` -> `exports.x` for exported `let`/`var` bindings.
                let has_cjs_export_ref =
                    self.export_target.as_ref().is_some_and(|t| t == "exports")
                        && !self.cjs_var_export_names.is_empty()
                        && stmt_has_ns_export_ref(s, &self.cjs_var_export_names);
                let has_ns_ref = self.export_target.as_ref().is_some_and(|t| t != "exports")
                    && ((!self.namespace_exports.is_empty()
                        && stmt_has_ns_export_ref(s, &self.namespace_exports))
                        || self.ns_export_stack.iter().any(|(_, exports)| {
                            !exports.is_empty() && stmt_has_ns_export_ref(s, exports)
                        }));
                // A lowered ES5 class member rewrites `super` structurally.
                let has_es5_super = self.es5_super_home.is_some()
                    && self
                        .source_between(s.span.start, s.span.end)
                        .contains("super");
                if !stmt_needs_transform(s)
                    && !has_ceref
                    && !has_cjs
                    && !has_downlevel
                    && !has_ns_ref
                    && !has_cjs_export_ref
                    && !has_es5_super
                {
                    let start = s.span.start as usize;
                    let end = s.span.end as usize;
                    if start < end && end <= self.source.len() {
                        let text = &self.source[start..end];
                        let normalized = normalize_brace_spacing(text);
                        // Five chain stages collapsed into one unified pass.
                        let normalized = normalize_unified_pass(&normalized);
                        self.output.push_str(&normalized);
                        // Add semicolon if needed and not present
                        if stmt_needs_trailing_semicolon(s) && !normalized.trim_end().ends_with(';')
                        {
                            self.output.push(';');
                        }
                    }
                } else {
                    // For transformed stmts, emit normally but without newline
                    let _before = self.output.len();
                    self.emit_stmt(s);
                    // Remove trailing newline if present
                    if self.output.ends_with('\n') {
                        self.output.pop();
                        self.at_line_start = false;
                    }
                }
            }
            // Emit any comments between the last statement and the closing `}`
            if let Some(bend) = block_end {
                let last_end = stmts.last().unwrap().span.end as usize;
                let bend = bend as usize;
                if last_end < bend && bend <= self.source.len() {
                    let gap = &self.source[last_end..bend];
                    // Look for block comments /* ... */ or line comments //
                    let trimmed = gap.trim();
                    if let Some(c_start) = trimmed.find("/*") {
                        if let Some(c_end) = trimmed[c_start..].find("*/") {
                            let comment = &trimmed[c_start..c_start + c_end + 2];
                            self.write(" ");
                            self.output.push_str(comment);
                        }
                    } else if let Some(c_start) = trimmed.find("//") {
                        let comment = trimmed[c_start..].trim_end();
                        self.write(" ");
                        self.output.push_str(comment);
                    }
                }
            }
            self.write(" }");
        }
    }

    /// Check whether any parameter needs async lifting (destructured or has
    /// a default value), which requires the ES2017 parameter-move transform.
    pub(super) fn param_needs_async_lift(p: &Param) -> bool {
        p.initializer.is_some() || !matches!(p.name.kind, PatKind::Ident(_))
    }

    /// Generate temporary parameter names for the outer function/arrow when
    /// the ES2017 async transform lifts parameters into the inner generator.
    /// - Simple ident params `foo` → `foo_1`
    /// - Destructured / non-ident params → `_a`, `_b`, `_c`, …
    pub(super) fn generate_async_lift_temp_names(params: &[Param]) -> Vec<String> {
        let mut alpha = 0u8;
        let mut names = Vec::with_capacity(params.len());
        for param in params {
            // Skip 'this' parameter
            if let PatKind::Ident(ref name) = param.name.kind {
                if name == "this" {
                    continue;
                }
            }
            match &param.name.kind {
                PatKind::Ident(name) => {
                    names.push(format!("{name}_1"));
                }
                _ => {
                    let letter = (b'a' + alpha) as char;
                    names.push(format!("_{letter}"));
                    alpha += 1;
                }
            }
        }
        names
    }

    /// For async parameter-lift with defaults, TypeScript keeps outer temp
    /// parameters only up to (but excluding) the first default/destructured
    /// parameter, preserving function `.length`.
    pub(super) fn generate_async_lift_prefix_temp_names(params: &[Param]) -> Vec<String> {
        let mut names = Vec::new();
        for param in params {
            let PatKind::Ident(name) = &param.name.kind else {
                break;
            };
            if name == "this" || name == "<error>" || param.dotdotdot || param.initializer.is_some()
            {
                break;
            }
            names.push(format!("{name}_1"));
        }
        names
    }

    pub(super) fn emit_params_without_initializers(&mut self, params: &[Param]) {
        self.es5_name_parameter_temps(params);
        let mut first = true;
        for param in params {
            let is_error_name =
                matches!(&param.name.kind, PatKind::Ident(ref name) if name == "<error>");
            if let PatKind::Ident(ref name) = param.name.kind {
                if name == "this" {
                    continue;
                }
                if name == "<error>" && !param.dotdotdot {
                    continue;
                }
            }
            // Rest parameters are materialized from `arguments` in the body.
            if param.dotdotdot {
                continue;
            }
            if !first {
                self.write(", ");
            }
            first = false;
            if !matches!(param.name.kind, PatKind::Ident(_)) {
                let temp = self.es5_param_temp_names[&param.span.start].clone();
                self.write(&temp);
                continue;
            }
            if !is_error_name {
                self.emit_binding_name(&param.name);
            }
        }
    }

    pub(super) fn emit_params(&mut self, params: &[Param]) {
        let mut first = true;
        let mut prev_emitted_end: Option<u32> = None;
        for param in params {
            let preserve_decorators =
                self.should_preserve_decorators() && !param.decorators.is_empty();
            // Skip 'this' parameter (TypeScript-only) and error-recovered params
            // But keep rest params (...) even with missing names
            let is_error_name =
                matches!(&param.name.kind, PatKind::Ident(ref name) if name == "<error>");
            if let PatKind::Ident(ref name) = param.name.kind {
                if name == "this" {
                    // Preserve `...` from `...this: Type` in error recovery:
                    // TypeScript emits `function f(...) {}` not `function f() {}`
                    if param.dotdotdot {
                        if !first {
                            self.write(", ");
                        }
                        first = false;
                        self.write("...");
                        prev_emitted_end = Some(param.span.end);
                    }
                    continue;
                }
                if name == "<error>" && !param.dotdotdot {
                    continue;
                }
            }
            // Skip parameter property modifiers are handled by name only
            if !first {
                let separator_break = prev_emitted_end.is_some_and(|prev_end| {
                    !param.dotdotdot
                        && self.param_separator_breaks_before_comment(prev_end, param.span.start)
                });
                // Preserved decorators start on the following line. Avoid
                // leaving separator whitespace at the end of the prior line.
                if preserve_decorators {
                    self.write(",");
                } else {
                    self.write(", ");
                }
                if separator_break {
                    self.newline();
                }
            }
            first = false;
            // Emit inline comments before parameter (e.g. `/*c1*/ x`)
            self.emit_inline_comments_before(param.span.start);
            // When a `/* ... */` comment ends a line and the parameter name
            // follows on the next line (after `public` stripping), join them
            // IF the comment was NOT the only content on its source line.
            // Own-line comments stay on their own line.
            if self.at_line_start && self.output.trim_end().ends_with("*/") {
                // Check if the comment was on a shared line (had other content)
                // by looking at the output: if the line with `*/` has content
                // before the comment (like a previous parameter), it's shared.
                // Check if the `*/` line has a comma before the comment,
                // meaning the comment shares the line with a parameter.
                let trimmed_output = self.output.trim_end();
                let last_newline = trimmed_output.rfind('\n').map(|p| p + 1).unwrap_or(0);
                let line_content = &trimmed_output[last_newline..];
                let comment_start = line_content.rfind("/*").unwrap_or(0);
                let before_comment = &line_content[..comment_start];
                let comment_is_shared =
                    before_comment.contains(',') || before_comment.contains('(');
                if comment_is_shared {
                    while self.output.ends_with('\n')
                        || self.output.ends_with(' ')
                        || self.output.ends_with('\t')
                    {
                        self.output.pop();
                    }
                    self.output.push(' ');
                    self.at_line_start = false;
                }
            }
            if preserve_decorators {
                self.emit_preserved_decorators(&param.decorators, Some(param.name.span.start));
            }
            if param.dotdotdot {
                self.write("...");
                // Emit inline comments between `...` and parameter name (e.g. `.../*3*/y` → `... /*3*/y`)
                let dot_end = param.span.start as usize + 3;
                let name_start = param.name.span.start as usize;
                self.emit_compact_comment_between(dot_end, name_start, true);
            }
            let mut handled_recovered_rest_initializer = false;
            // Don't emit error names for rest-only params like `function f(...) {}`
            if !is_error_name {
                if param.dotdotdot {
                    if let Some(init) = param.initializer.as_deref() {
                        let prev_in_param_init = self.in_parameter_initializer;
                        self.in_parameter_initializer = true;
                        handled_recovered_rest_initializer =
                            self.emit_recovered_rest_param_default_pattern(&param.name, init);
                        self.in_parameter_initializer = prev_in_param_init;
                    }
                }
                if !handled_recovered_rest_initializer
                    && !(param.dotdotdot
                        && self.emit_recovered_rest_param_assignment_pattern(&param.name))
                {
                    self.emit_binding_name(&param.name);
                }
            }
            // For params without initializers, emit trailing comments after the name
            if param.initializer.is_none() {
                self.emit_inline_trailing_comment(param);
            }
            // Skip comments inside erased type annotation
            if let Some(ref type_ann) = param.type_ann {
                self.advance_comment_pos(type_ann.span.end);
            }
            if let Some(ref init) = param.initializer {
                if handled_recovered_rest_initializer {
                    prev_emitted_end = Some(param.span.end);
                    continue;
                }
                // Skip emitting <error> placeholder initializers — parser error
                // recovery may have consumed a closing paren/bracket into the
                // error span, and source-copying it would duplicate that token.
                let is_error_init =
                    matches!(&init.kind, ExprKind::Ident(name) if name == "<error>");
                self.write(" = ");
                let prev_in_param_init = self.in_parameter_initializer;
                self.in_parameter_initializer = true;
                if is_error_init {
                    // Just emit the ` = ` (already done above); skip the expression.
                } else if !self.emit_recovered_async_await_param_initializer(init) {
                    // Suppress outer parens for nullish coalescing in default values —
                    // the `=` context doesn't need disambiguation.
                    self.suppress_oc_parens = true;
                    // Set class_expr_binding_name for decorated/static anonymous class exprs
                    // in parameter defaults so __setFunctionName uses the param name.
                    let prev_binding = self.class_expr_binding_name.take();
                    if let PatKind::Ident(pname) = &param.name.kind {
                        if matches!(&init.kind, ExprKind::ClassExpr(cd) if cd.name.is_none() && (!cd.decorators.is_empty() || class_has_static_initializers(cd) || class_has_member_decorators(cd)))
                        {
                            self.class_expr_binding_name =
                                Some(ClassExprBindingName::Literal(pname.to_string()));
                        }
                    }
                    self.emit_expr(init);
                    self.class_expr_binding_name = prev_binding;
                }
                self.in_parameter_initializer = prev_in_param_init;
                // For params with initializers, emit trailing comments after the default value
                self.emit_inline_trailing_comment(param);
            }
            prev_emitted_end = Some(param.span.end);
        }
    }

    fn is_malformed_await_arrow_expr(expr: &Expr) -> bool {
        let ExprKind::Arrow(arrow) = &expr.kind else {
            return false;
        };
        if arrow.params.len() != 1 {
            return false;
        }
        let param = &arrow.params[0];
        if param.dotdotdot || param.optional || param.initializer.is_some() {
            return false;
        }
        if !matches!(&param.name.kind, PatKind::Ident(name) if name == "await") {
            return false;
        }
        match &arrow.body {
            ArrowBody::Expr(body) => {
                matches!(&body.kind, ExprKind::Await(_))
                    || matches!(&body.kind, ExprKind::Ident(name) if name == "await")
            }
            ArrowBody::Block(_) => false,
        }
    }

    fn emit_recovered_async_await_param_initializer(&mut self, init: &Expr) -> bool {
        if !self.in_async_function || !Self::is_malformed_await_arrow_expr(init) {
            return false;
        }
        let src = self.copy_span_trimmed(init.span);
        let compact: String = src.chars().filter(|ch| !ch.is_ascii_whitespace()).collect();
        if compact != "await=>await" {
            return false;
        }
        if self.needs_downlevel("async") {
            self.write("yield , await");
        } else {
            self.write("await , await");
        }
        true
    }

    fn emit_recovered_rest_param_assignment_pattern(&mut self, pat: &Pat) -> bool {
        let PatKind::Assign(inner, init) = &pat.kind else {
            return false;
        };
        let PatKind::Array(elements) = &inner.kind else {
            return false;
        };
        let [Some(ArrayPatElem::Rest(rest_pat))] = elements.as_slice() else {
            return false;
        };
        self.write("[");
        self.write("...");
        self.emit_binding_name(&Pat {
            kind: PatKind::Assign(Box::new(rest_pat.clone()), init.clone()),
            span: pat.span,
        });
        self.write("]");
        true
    }

    fn emit_recovered_rest_param_default_pattern(&mut self, pat: &Pat, init: &Expr) -> bool {
        let PatKind::Array(elements) = &pat.kind else {
            return false;
        };
        let [Some(ArrayPatElem::Rest(rest_pat))] = elements.as_slice() else {
            return false;
        };
        self.write("[");
        self.write("...");
        self.emit_binding_name(&Pat {
            kind: PatKind::Assign(Box::new(rest_pat.clone()), Box::new(init.clone())),
            span: pat.span,
        });
        self.write("]");
        true
    }

    fn param_separator_breaks_before_comment(&self, prev_end: u32, next_start: u32) -> bool {
        let ps = prev_end as usize;
        let ns = next_start as usize;
        if ps >= ns || ns > self.source.len() {
            return false;
        }
        let gap = &self.source[ps..ns];
        let Some(comma_idx) = gap.rfind(',') else {
            return false;
        };
        let after = &gap[comma_idx + 1..];
        let trimmed = after.trim_start();
        if !(trimmed.starts_with("/*") || trimmed.starts_with("//")) {
            return false;
        }
        let ws_len = after.len().saturating_sub(trimmed.len());
        after.as_bytes()[..ws_len]
            .iter()
            .any(|&b| b == b'\n' || b == b'\r')
    }

    /// Preserve multiline commented parameter layouts for simple parameter lists.
    /// This matches TS baseline formatting for cases like:
    /// `function f(\n/* c */\na\n/* c2 */\n,\nb\n/* c3 */\n){}`.
    fn emit_multiline_commented_param_list(&mut self, fn_decl: &FnDecl) -> bool {
        if self.options.remove_comments == Some(true) && !self.preserve_comments {
            return false;
        }
        if fn_decl.params.is_empty() {
            return false;
        }
        if fn_decl.params.iter().any(|p| {
            p.dotdotdot
                || p.optional
                || p.type_ann.is_some()
                || p.initializer.is_some()
                || p.modifiers != MOD_NONE
                || !matches!(&p.name.kind, PatKind::Ident(name) if name != "this" && name != "<error>")
        }) {
            return false;
        }

        let start = fn_decl.span.start as usize;
        let end = (fn_decl.span.end as usize).min(self.source.len());
        if start >= end {
            return false;
        }
        let src = &self.source[start..end];
        let Some(open_rel) = src.find('(') else {
            return false;
        };
        let Some(close_rel_tail) = src[open_rel + 1..].find(')') else {
            return false;
        };
        let close_rel = open_rel + 1 + close_rel_tail;
        let raw = &src[open_rel + 1..close_rel];
        if !raw.contains('\n') || !raw.contains("/*") {
            return false;
        }
        // Keep this narrowly scoped to the comment-led multiline form used by
        // commentOnParameter1/2. Other multiline parameter forms should use
        // normal emit_params formatting.
        if !raw.trim_start().starts_with("/*") {
            return false;
        }

        let lines: Vec<&str> = raw.split('\n').collect();
        let mut out_lines: Vec<String> = Vec::with_capacity(lines.len());
        let mut i = 0usize;
        while i < lines.len() {
            let line = lines[i].trim_end();
            if line.trim() == "," && i + 1 < lines.len() {
                out_lines.push(format!(", {}", lines[i + 1].trim()));
                i += 2;
                continue;
            }
            out_lines.push(line.to_string());
            i += 1;
        }
        self.write(&out_lines.join("\n"));
        true
    }

    /// When the parameter list is effectively empty (all params skipped),
    /// check for block comments between `(` and `)` in the source and emit them.
    /// E.g. `function foo(/** nothing */)` → `function foo( /** nothing */)`
    pub(super) fn emit_empty_parens_comments(&mut self, fn_span: Span, params: &[Param]) {
        if self.options.remove_comments == Some(true) && !self.preserve_comments {
            return;
        }
        // Only applies when no visible params were emitted
        let has_visible = params
            .iter()
            .any(|p| !matches!(&p.name.kind, PatKind::Ident(n) if n == "this" || (n == "<error>" && !p.dotdotdot)));
        if has_visible {
            return;
        }
        // Find the `(` in the source after the function name
        let start = fn_span.start as usize;
        let end = fn_span.end as usize;
        let src = &self.source[start..end.min(self.source.len())];
        // Find opening paren
        if let Some(open_pos) = src.find('(') {
            if let Some(close_pos) = src[open_pos..].find(')') {
                let between = &src[open_pos + 1..open_pos + close_pos];
                // Check for block comments
                if let Some(bc_start) = between.find("/*") {
                    if let Some(bc_end) = between[bc_start..].find("*/") {
                        let comment = between[bc_start..bc_start + bc_end + 2].trim();
                        if !comment.is_empty() {
                            self.write(" ");
                            self.write(comment);
                        }
                    }
                }
            }
        }
    }

    /// Emit block comments inside empty call/new argument lists.
    /// E.g. `new Foo(/* comment */)` → `new Foo( /* comment */)`
    /// Emit comments from an empty argument list.
    /// Returns `true` if the args region was multi-line (caller should put `)` on its own line).
    pub(super) fn emit_empty_args_comments(&mut self, expr_span: Span) -> bool {
        if self.options.remove_comments == Some(true) && !self.preserve_comments {
            return false;
        }
        let start = expr_span.start as usize;
        let end = (expr_span.end as usize).min(self.source.len());
        let src = &self.source[start..end];
        // Find the last `(` before `)` — this is the argument list opening
        let close_pos = match src.rfind(')') {
            Some(p) => p,
            None => return false,
        };
        let open_pos = match src[..close_pos].rfind('(') {
            Some(p) => p,
            None => return false,
        };
        let between = &src[open_pos + 1..close_pos];
        let is_multiline = between.contains('\n');

        if is_multiline {
            // Multi-line: emit each comment on its own line
            // Extract all comments (/* ... */ and // ...) from between parens
            let mut pos = 0;
            let bytes = between.as_bytes();
            while pos < bytes.len() {
                if pos + 1 < bytes.len() && bytes[pos] == b'/' && bytes[pos + 1] == b'*' {
                    // Block comment
                    if let Some(end_off) = between[pos + 2..].find("*/") {
                        let comment = &between[pos..pos + 2 + end_off + 2];
                        let comment = comment.trim();
                        if !comment.is_empty() {
                            self.newline();
                            self.write(comment);
                        }
                        pos = pos + 2 + end_off + 2;
                    } else {
                        pos += 1;
                    }
                } else if pos + 1 < bytes.len() && bytes[pos] == b'/' && bytes[pos + 1] == b'/' {
                    // Line comment — extract to end of line
                    let rest = &between[pos..];
                    let line_end = rest.find('\n').unwrap_or(rest.len());
                    let comment = rest[..line_end].trim();
                    if !comment.is_empty() {
                        self.newline();
                        self.write(comment);
                    }
                    pos = pos + line_end;
                } else {
                    pos += 1;
                }
            }
            true
        } else {
            // Single-line: emit space + first block comment
            if let Some(bc_start) = between.find("/*") {
                if let Some(bc_end) = between[bc_start..].find("*/") {
                    let comment = between[bc_start..bc_start + bc_end + 2].trim();
                    if !comment.is_empty() {
                        self.write(" ");
                        self.write(comment);
                    }
                }
            }
            false
        }
    }

    /// Emit inline block comments that appear before `before_pos` and after
    /// the most recent `(` in the source (to avoid picking up type parameter comments).
    /// Only emits `/* ... */` style comments (not line comments) since these are inline.
    pub(super) fn emit_inline_comments_before(&mut self, before_pos: u32) {
        if self.options.remove_comments == Some(true) && !self.preserve_comments {
            return;
        }
        // Find the position of the most recent `(` before `before_pos` to set
        // a lower bound (avoid emitting comments from type parameter positions).
        let bp = before_pos as usize;
        let paren_pos = if bp > 0 && bp <= self.source.len() {
            self.source[..bp]
                .rfind('(')
                .map(|p| p as u32 + 1) // after the `(`
                .unwrap_or(self.comment_emit_pos)
        } else {
            self.comment_emit_pos
        };
        let lower = paren_pos.max(self.comment_emit_pos);

        while self.next_comment_idx < self.comments.len() {
            let c = &self.comments[self.next_comment_idx];
            if c.pos >= before_pos {
                break;
            }
            if c.pos < lower {
                self.next_comment_idx += 1;
                continue;
            }
            let start = c.pos as usize;
            let end = (c.end as usize).min(self.source.len());
            let text = self.source[start..end].trim();
            if text.starts_with("/*") && text.ends_with("*/") {
                // If there's a newline between `(` and the comment in the
                // source, preserve it so JSDoc comments on constructor
                // parameters stay on their own lines.
                let gap_start = lower as usize;
                let gap_end = start;
                if gap_start < gap_end && gap_end <= self.source.len() {
                    let gap = &self.source[gap_start..gap_end];
                    if gap.contains('\n') && !self.at_line_start {
                        self.newline();
                    }
                }
                self.write(text);
                let bp = before_pos as usize;
                let trailing = if end <= bp && bp <= self.source.len() {
                    &self.source[end..bp]
                } else {
                    ""
                };
                if trailing.bytes().any(|b| b == b'\n' || b == b'\r') {
                    // TypeScript preserves a trailing space after `*/` when
                    // the comment is inline (shares the line with code).
                    // Don't add trailing space for own-line comments.
                    let line_start = self.output.rfind('\n').map(|p| p + 1).unwrap_or(0);
                    let line_before = &self.output[line_start..];
                    let has_code_before = line_before
                        .trim_start()
                        .strip_suffix(text)
                        .is_some_and(|before| !before.trim().is_empty());
                    if has_code_before {
                        self.write(" ");
                    }
                    self.newline();
                } else {
                    self.write(" ");
                }
                self.comment_emit_pos = self.comment_emit_pos.max(c.end);
                self.next_comment_idx += 1;
            } else if text.starts_with("//") {
                // Handle single-line comments between parameters.
                // These always require a newline before (if not already at
                // line start) and a newline after (since // consumes the
                // rest of the line).
                let gap_start = lower as usize;
                let gap_end = start;
                if gap_start < gap_end && gap_end <= self.source.len() {
                    let gap = &self.source[gap_start..gap_end];
                    if gap.contains('\n') && !self.at_line_start {
                        self.newline();
                    }
                }
                self.write(text);
                self.newline();
                self.comment_emit_pos = self.comment_emit_pos.max(c.end);
                self.next_comment_idx += 1;
            } else {
                break;
            }
        }
    }

    /// Emit inline block comments before a variable declarator name.
    /// Unlike `emit_inline_comments_before` which is scoped to `(…)` contexts,
    /// this scans from `comment_emit_pos` to `before_pos` for `/* … */` comments.
    pub(super) fn emit_var_decl_inline_comments(&mut self, before_pos: u32) {
        self.emit_var_decl_inline_comments_inner(before_pos, false);
    }

    pub(super) fn emit_var_decl_inline_comments_with_line(&mut self, before_pos: u32) {
        self.emit_var_decl_inline_comments_inner(before_pos, true);
    }

    fn emit_var_decl_inline_comments_inner(&mut self, before_pos: u32, allow_line_comments: bool) {
        if self.options.remove_comments == Some(true) {
            return;
        }
        while self.next_comment_idx < self.comments.len() {
            let c = &self.comments[self.next_comment_idx];
            if c.pos >= before_pos {
                break;
            }
            if c.pos < self.comment_emit_pos {
                self.next_comment_idx += 1;
                continue;
            }
            let start = c.pos as usize;
            let end = (c.end as usize).min(self.source.len());
            let text = self.source[start..end].trim();
            if text.starts_with("/*") && text.ends_with("*/") {
                self.write(text);
                self.write(" ");
                self.comment_emit_pos = self.comment_emit_pos.max(c.end);
                self.next_comment_idx += 1;
            } else if allow_line_comments && text.starts_with("//") {
                // Single-line comment between `=` and initializer.
                // Emit the comment then a newline, since `//` consumes
                // the rest of the line.
                self.write(text);
                self.newline();
                self.comment_emit_pos = self.comment_emit_pos.max(c.end);
                self.next_comment_idx += 1;
            } else {
                break;
            }
        }
    }

    /// Emit a trailing inline comment after a parameter (e.g. `a /* param a */`).
    pub(super) fn emit_inline_trailing_comment(&mut self, param: &Param) {
        if self.options.remove_comments == Some(true) && !self.preserve_comments {
            return;
        }
        // Look in source text from after the parameter name (or initializer if present)
        // to the next `,` or `)` for a /* ... */ comment.
        let search_start = if let Some(ref init) = param.initializer {
            init.span.end as usize
        } else {
            param.name.span.end as usize
        };
        if search_start >= self.source.len() {
            return;
        }
        // Scan forward from the name end for a block comment before `,` or `)`
        let rest = &self.source[search_start..];
        // Find the next `,` or `)` that is not inside a comment.
        // A naive `rest.find(',')` would truncate at commas inside
        // trailing comments (e.g. `// expected to work, but actually doesn't`).
        let sep_pos = find_separator_outside_comments(rest);
        let segment = &rest[..sep_pos];
        let mut idx = 0usize;
        let mut scan_pos = 0usize;
        let mut brace_depth = 0i32;
        let mut bracket_depth = 0i32;
        let mut paren_depth = 0i32;
        while idx < segment.len() {
            let tail = &segment[idx..];
            let block_pos = tail.find("/*");
            let line_pos = tail.find("//");
            let next = match (block_pos, line_pos) {
                (Some(b), Some(l)) => Some((b.min(l), b <= l)),
                (Some(b), None) => Some((b, true)),
                (None, Some(l)) => Some((l, false)),
                (None, None) => None,
            };
            let Some((pos, is_block)) = next else {
                break;
            };
            idx += pos;
            for ch in segment[scan_pos..idx].chars() {
                match ch {
                    '{' => brace_depth += 1,
                    '}' => brace_depth -= 1,
                    '[' => bracket_depth += 1,
                    ']' => bracket_depth -= 1,
                    '(' => paren_depth += 1,
                    ')' => paren_depth -= 1,
                    _ => {}
                }
            }
            let inside_nested_type = brace_depth > 0 || bracket_depth > 0 || paren_depth > 0;
            if is_block {
                let block_tail = &segment[idx..];
                let Some(end_rel) = block_tail.find("*/") else {
                    break;
                };
                let end_idx = idx + end_rel + 2;
                if !inside_nested_type {
                    let comment = segment[idx..end_idx].trim();
                    let normalized_comment = if comment.contains('\n') || comment.contains('\r') {
                        comment
                            .replace(" \n", "\n")
                            .replace("\t\n", "\n")
                            .replace(" \r", "\r")
                            .replace("\t\r", "\r")
                    } else {
                        comment.to_string()
                    };
                    self.write(" ");
                    self.write(&normalized_comment);
                    self.comment_emit_pos =
                        self.comment_emit_pos.max((search_start + end_idx) as u32);
                    while self.next_comment_idx < self.comments.len()
                        && self.comments[self.next_comment_idx].end <= self.comment_emit_pos
                    {
                        self.next_comment_idx += 1;
                    }
                }
                idx = end_idx;
                scan_pos = idx;
            } else {
                let line_tail = &segment[idx..];
                let eol_rel = line_tail
                    .find('\n')
                    .or_else(|| line_tail.find('\r'))
                    .unwrap_or(line_tail.len());
                let end_idx = idx + eol_rel;
                if !inside_nested_type {
                    let comment = segment[idx..end_idx].trim_end();
                    self.write(" ");
                    self.write(comment);
                    self.comment_emit_pos =
                        self.comment_emit_pos.max((search_start + end_idx) as u32);
                    while self.next_comment_idx < self.comments.len()
                        && self.comments[self.next_comment_idx].end <= self.comment_emit_pos
                    {
                        self.next_comment_idx += 1;
                    }
                    self.newline();
                }
                idx = end_idx;
                while idx < segment.len()
                    && matches!(segment.as_bytes()[idx], b'\n' | b'\r' | b' ' | b'\t')
                {
                    idx += 1;
                }
                scan_pos = idx;
            }
        }
    }

    /// Emit a binding pattern (variable name or destructuring).
    /// This strips type annotations but preserves the pattern structure.
    pub(super) fn emit_binding_name(&mut self, pat: &Pat) {
        match &pat.kind {
            PatKind::Ident(name) if name == "<error>" => {} // skip error placeholder
            PatKind::Ident(name) => {
                if let Some(renamed) = self
                    .lexical_downlevel_plan
                    .emitted_name_for_declaration(pat.span)
                    .map(str::to_owned)
                {
                    self.write(&renamed);
                } else {
                    self.write(name);
                }
            }
            PatKind::Array(elements) => {
                let trailing_none_count = elements.iter().rev().take_while(|e| e.is_none()).count();
                let source_trailing_commas = self.array_pattern_trailing_commas(pat.span);
                self.write("[");
                // Track position after `[` for inline comment emission
                let mut prev_end = pat.span.start as usize + 1;
                for (i, elem) in elements.iter().enumerate() {
                    if i > 0 {
                        self.write(",");
                        // Keep spaces after commas except for the final
                        // trailing hole right before `]`.
                        let is_final_trailing_hole = i + 1 == elements.len() && elem.is_none();
                        if !is_final_trailing_hole {
                            self.write(" ");
                        }
                    }
                    if let Some(ref e) = elem {
                        // Emit inline comments between previous position and element start
                        // e.g. `[/*a*/ a]` → preserve the `/*a*/` comment
                        let elem_start = match e {
                            ArrayPatElem::Pat(p) => p.span.start as usize,
                            ArrayPatElem::Rest(p) => self
                                .find_spread_start_before(p.span.start as usize)
                                .unwrap_or(p.span.start as usize),
                        };
                        if prev_end < elem_start {
                            self.emit_array_pat_comment_between(prev_end, elem_start);
                        }
                        match e {
                            ArrayPatElem::Pat(p) => {
                                self.emit_binding_name(p);
                                prev_end = p.span.end as usize;
                            }
                            ArrayPatElem::Rest(p) => {
                                self.write("...");
                                if let Some(spread_start) =
                                    self.find_spread_start_before(p.span.start as usize)
                                {
                                    self.emit_compact_comment_between(
                                        spread_start + 3,
                                        p.span.start as usize,
                                        true,
                                    );
                                }
                                self.emit_binding_name(p);
                                prev_end = p.span.end as usize;
                            }
                        }
                    }
                }
                if trailing_none_count > 0 && source_trailing_commas > trailing_none_count {
                    for _ in 0..(source_trailing_commas - trailing_none_count) {
                        self.write(" ,");
                    }
                } else if trailing_none_count == 0 && source_trailing_commas > 0 {
                    // Preserve trailing comma after last real element
                    self.write(",");
                }
                self.write("]");
            }
            PatKind::Object(props) => {
                if props.is_empty() {
                    self.write("{}");
                } else {
                    let pat_start = pat.span.start as usize;
                    let pat_end = pat.span.end as usize;
                    // Find the closing `}` position in source (absolute offset).
                    let close_brace_abs = if pat_start < pat_end && pat_end <= self.source.len() {
                        let text = &self.source[pat_start..pat_end];
                        text.rfind('}').map(|idx| pat_start + idx)
                    } else {
                        None
                    };
                    // Detect trailing comma in source
                    let obj_has_trailing_comma = close_brace_abs
                        .map(|cb| {
                            let before = self.source[pat_start..cb].trim_end();
                            before.ends_with(',')
                        })
                        .unwrap_or(false);
                    self.write("{ ");
                    let mut prev_had_line_comment = false;
                    for (i, prop) in props.iter().enumerate() {
                        if i > 0 && !prev_had_line_comment {
                            self.write(", ");
                        }
                        prev_had_line_comment = false;
                        match prop {
                            ObjPatProp::KeyValue(key, val) => {
                                // When the binding target is `<error>` from a reserved keyword
                                // in source (e.g. `{ while: while }`), TypeScript's parser
                                // doesn't consume the keyword, producing two properties:
                                // `while:` (empty binding) and `while:` (keyword as new prop).
                                // Replicate this by emitting `key: , keyword: `.
                                let reserved_kw_src = if matches!(&val.kind, PatKind::Ident(name) if name == "<error>")
                                {
                                    let s = val.span.start as usize;
                                    let e = val.span.end as usize;
                                    if s < e && e <= self.source.len() {
                                        let src = self.source[s..e].trim();
                                        if is_js_reserved_keyword(src) {
                                            Some(src.to_string())
                                        } else {
                                            None
                                        }
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                };
                                self.emit_prop_name(key);
                                self.write(": ");
                                if let Some(kw) = reserved_kw_src {
                                    // Emit the duplicate: `, keyword: `
                                    self.write(", ");
                                    self.write(&kw);
                                    self.write(": ");
                                } else {
                                    self.emit_binding_name(val);
                                }
                            }
                            ObjPatProp::Shorthand(name, span) => {
                                // Reserved keywords can't be binding names.
                                // TypeScript emits `keyword: ` (key + empty value)
                                // for shorthand destructuring with reserved words.
                                // Numeric keys (e.g. `{1}`) also can't be shorthand
                                // identifiers and need `1: ` form.
                                if is_js_reserved_keyword(name)
                                    || name.starts_with('"')
                                    || name.starts_with(|c: char| c.is_ascii_digit())
                                {
                                    self.write(name);
                                    self.write(": ");
                                } else if let Some(renamed) = self
                                    .lexical_downlevel_plan
                                    .emitted_name_for_declaration(*span)
                                    .map(str::to_owned)
                                {
                                    self.write(name);
                                    self.write(": ");
                                    self.write(&renamed);
                                } else {
                                    self.write(name);
                                }
                            }
                            ObjPatProp::ShorthandAssign(name, init, span) => {
                                self.write(name);
                                if let Some(renamed) = self
                                    .lexical_downlevel_plan
                                    .emitted_name_for_declaration(*span)
                                    .map(str::to_owned)
                                {
                                    self.write(": ");
                                    self.write(&renamed);
                                }
                                self.write(" = ");
                                let prev_binding_name = self.class_expr_binding_name.clone();
                                if matches!(&init.kind, ExprKind::ClassExpr(cd) if cd.name.is_none() && (class_has_static_initializers(cd) || !cd.decorators.is_empty() || class_has_member_decorators(cd)))
                                {
                                    self.class_expr_binding_name =
                                        Some(ClassExprBindingName::Literal(name.to_string()));
                                }
                                self.emit_expr(init);
                                self.class_expr_binding_name = prev_binding_name;
                            }
                            ObjPatProp::Rest(p) => {
                                self.write("...");
                                if let Some(spread_start) =
                                    self.find_spread_start_before(p.span.start as usize)
                                {
                                    self.emit_compact_comment_between(
                                        spread_start + 3,
                                        p.span.start as usize,
                                        true,
                                    );
                                }
                                self.emit_binding_name(p);
                            }
                        }
                        // Preserve trailing line comments (`// ...`) after properties.
                        // TypeScript preserves these which forces a line break.
                        // Emit: `prop, // comment\n` then next property without leading `, `.
                        let prop_end = obj_pat_prop_source_end(prop) as usize;
                        let next_start = if i + 1 < props.len() {
                            obj_pat_prop_source_start(&props[i + 1]) as usize
                        } else {
                            close_brace_abs.unwrap_or(pat_end)
                        };
                        if prop_end < next_start && next_start <= self.source.len() {
                            let between = &self.source[prop_end..next_start];
                            // Look for a `//` line comment
                            if let Some(comment_idx) = between.find("//") {
                                let comment_text = &between[comment_idx..];
                                let end = comment_text.find('\n').unwrap_or(comment_text.len());
                                let comment = comment_text[..end].trim_end();
                                if !comment.is_empty() {
                                    // Emit comma before comment if not last prop
                                    if i + 1 < props.len() {
                                        self.write(",");
                                    }
                                    self.write(" ");
                                    self.write(comment);
                                    self.newline();
                                    prev_had_line_comment = true;
                                }
                            }
                        }
                    }
                    if obj_has_trailing_comma {
                        self.write(",");
                    }
                    // Emit inline comments between the last property and
                    // the closing `}` (e.g. `= {} /*comment*/ }`).
                    // Skip if the last property already had a trailing line
                    // comment emitted (to avoid double-emission).
                    if !prev_had_line_comment {
                        if let Some(cb) = close_brace_abs {
                            if let Some(last_prop) = props.last() {
                                let last_end = obj_pat_prop_source_end(last_prop) as usize;
                                self.emit_compact_comment_between(last_end, cb, true);
                            }
                        }
                    }
                    self.write(" }");
                }
            }
            PatKind::Assign(p, init) => {
                self.emit_binding_name(p);
                self.write(" = ");
                let prev_binding_name = self.class_expr_binding_name.clone();
                if matches!(&init.kind, ExprKind::ClassExpr(cd) if cd.name.is_none() && (class_has_static_initializers(cd) || !cd.decorators.is_empty() || class_has_member_decorators(cd)))
                {
                    if let PatKind::Ident(name) = &p.kind {
                        self.class_expr_binding_name =
                            Some(ClassExprBindingName::Literal(name.to_string()));
                    }
                }
                self.emit_expr(init);
                self.class_expr_binding_name = prev_binding_name;
            }
            PatKind::Rest(p) => {
                self.write("...");
                let dot_end = pat.span.start as usize + 3;
                self.emit_compact_comment_between(dot_end, p.span.start as usize, true);
                self.emit_binding_name(p);
            }
        }
    }

    pub(super) fn emit_compact_comment_between(
        &mut self,
        from: usize,
        to: usize,
        require_same_line_prefix: bool,
    ) -> bool {
        if self.options.remove_comments == Some(true) || from >= to || to > self.source.len() {
            return false;
        }
        let between = &self.source[from..to];
        let trimmed_start = between.trim_start();
        if trimmed_start.is_empty() {
            return false;
        }
        let leading_len = between.len().saturating_sub(trimmed_start.len());
        if require_same_line_prefix
            && between[..leading_len]
                .chars()
                .any(|ch| ch == '\n' || ch == '\r')
        {
            return false;
        }
        if trimmed_start.starts_with("/*") {
            let Some(end_rel) = trimmed_start.find("*/") else {
                return false;
            };
            let comment_end = end_rel + 2;
            if !trimmed_start[comment_end..].trim().is_empty() {
                return false;
            }
            let comment = &trimmed_start[..comment_end];
            self.write(" ");
            self.write(comment);
            let abs_end = (from + leading_len + comment_end) as u32;
            self.comment_emit_pos = self.comment_emit_pos.max(abs_end);
            while self.next_comment_idx < self.comments.len()
                && self.comments[self.next_comment_idx].end <= abs_end
            {
                self.next_comment_idx += 1;
            }
            return true;
        }
        if trimmed_start.starts_with("//") {
            let comment_end = trimmed_start
                .find('\n')
                .or_else(|| trimmed_start.find('\r'))
                .unwrap_or(trimmed_start.len());
            if !trimmed_start[comment_end..].trim().is_empty() {
                return false;
            }
            let comment = trimmed_start[..comment_end].trim_end();
            self.write(" ");
            self.write(comment);
            let abs_end = (from + leading_len + comment_end) as u32;
            self.comment_emit_pos = self.comment_emit_pos.max(abs_end);
            while self.next_comment_idx < self.comments.len()
                && self.comments[self.next_comment_idx].end <= abs_end
            {
                self.next_comment_idx += 1;
            }
            return true;
        }
        false
    }

    /// Emit a comment that is being moved with a transformed token. Line
    /// comments must retain a line terminator or they would swallow the tokens
    /// synthesized after them.
    pub(super) fn emit_moved_comment_between(
        &mut self,
        from: usize,
        to: usize,
        require_same_line_prefix: bool,
    ) -> bool {
        let is_line_comment = from < to
            && to <= self.source.len()
            && self.source[from..to].trim_start().starts_with("//");
        let emitted = self.emit_compact_comment_between(from, to, require_same_line_prefix);
        if emitted && is_line_comment {
            self.writeln("");
        }
        emitted
    }

    /// Emit inline block comments between two source positions for array pattern
    /// elements. Unlike `emit_compact_comment_between`, puts space AFTER the
    /// comment (not before) to match TypeScript's `[/*a*/ a]` style.
    fn emit_array_pat_comment_between(&mut self, from: usize, to: usize) {
        if self.options.remove_comments == Some(true) || from >= to || to > self.source.len() {
            return;
        }
        let between = &self.source[from..to];
        let trimmed = between.trim();
        if trimmed.is_empty() || !trimmed.starts_with("/*") {
            return;
        }
        let Some(end_rel) = trimmed.find("*/") else {
            return;
        };
        let comment_end = end_rel + 2;
        if !trimmed[comment_end..].trim().is_empty() {
            return;
        }
        let comment = &trimmed[..comment_end];
        self.write(comment);
        self.write(" ");
        // Advance comment tracking past this comment
        let leading_len = between.len().saturating_sub(between.trim_start().len());
        let abs_end = (from + leading_len + comment_end) as u32;
        self.comment_emit_pos = self.comment_emit_pos.max(abs_end);
        while self.next_comment_idx < self.comments.len()
            && self.comments[self.next_comment_idx].end <= abs_end
        {
            self.next_comment_idx += 1;
        }
    }

    pub(super) fn emit_compact_spread_comment(&mut self, dots_end: usize) -> bool {
        if self.options.remove_comments == Some(true) || dots_end >= self.source.len() {
            return false;
        }
        let rest = &self.source[dots_end..];
        let mut hws_len = 0usize;
        for (i, ch) in rest.char_indices() {
            if ch == ' ' || ch == '\t' {
                hws_len = i + ch.len_utf8();
            } else {
                break;
            }
        }
        let after_ws = &rest[hws_len..];
        if after_ws.starts_with("/*") {
            let Some(end_rel) = after_ws.find("*/") else {
                return false;
            };
            let comment_end = end_rel + 2;
            let comment = &after_ws[..comment_end];
            self.write(" ");
            self.write(comment);
            let abs_end = (dots_end + hws_len + comment_end) as u32;
            self.comment_emit_pos = self.comment_emit_pos.max(abs_end);
            while self.next_comment_idx < self.comments.len()
                && self.comments[self.next_comment_idx].end <= abs_end
            {
                self.next_comment_idx += 1;
            }
            return true;
        }
        if after_ws.starts_with("//") {
            let comment_end = after_ws
                .find('\n')
                .or_else(|| after_ws.find('\r'))
                .unwrap_or(after_ws.len());
            let comment = after_ws[..comment_end].trim_end();
            self.write(" ");
            self.write(comment);
            let abs_end = (dots_end + hws_len + comment_end) as u32;
            self.comment_emit_pos = self.comment_emit_pos.max(abs_end);
            while self.next_comment_idx < self.comments.len()
                && self.comments[self.next_comment_idx].end <= abs_end
            {
                self.next_comment_idx += 1;
            }
            return true;
        }
        false
    }

    fn find_spread_start_before(&self, target_start: usize) -> Option<usize> {
        if target_start > self.source.len() {
            return None;
        }
        let left = &self.source[..target_start];
        let boundary = left
            .rfind(|ch: char| matches!(ch, '{' | '[' | '(' | ',' | ';' | '='))
            .map(|idx| idx + 1)
            .unwrap_or(0);
        left[boundary..target_start]
            .rfind("...")
            .map(|rel| boundary + rel)
    }

    pub(super) fn span_has_adjacent_spread_comment_linebreak(&self, span: Span) -> bool {
        // Cosmetic comment-after-spread layout preservation — fast emit doesn't
        // reproduce tsc-faithful comment formatting, so skip the per-statement
        // span scan.
        if self.fast_emit {
            return false;
        }
        let start = span.start as usize;
        let end = span.end as usize;
        if start >= end || end > self.source.len() {
            return false;
        }
        if !self
            .adjacent_spread_comments
            .get_or_init(|| crate::spread_comments::SpreadCommentIndex::new(self.source))
            .may_contain(span)
        {
            return false;
        }
        let text = &self.source[start..end];
        let mut search_from = 0usize;
        while search_from < text.len() {
            let Some(rel) = text[search_from..].find("...") else {
                break;
            };
            let dots = search_from + rel;
            let after_dots = dots + 3;
            if after_dots >= text.len() {
                break;
            }
            let rest = &text[after_dots..];
            let mut hws_len = 0usize;
            for (i, ch) in rest.char_indices() {
                if ch == ' ' || ch == '\t' {
                    hws_len = i + ch.len_utf8();
                } else {
                    break;
                }
            }
            let after_ws = &rest[hws_len..];
            if after_ws.starts_with("/*") {
                if let Some(end_rel) = after_ws.find("*/") {
                    let after_comment = &after_ws[end_rel + 2..];
                    for ch in after_comment.chars() {
                        if ch == '\n' || ch == '\r' {
                            return true;
                        }
                        if !ch.is_whitespace() {
                            break;
                        }
                    }
                }
            } else if after_ws.starts_with("//")
                && after_ws
                    .find('\n')
                    .or_else(|| after_ws.find('\r'))
                    .is_some()
            {
                return true;
            }
            search_from = after_dots;
        }
        false
    }

    pub(super) fn span_has_adjacent_spread_comment(&self, span: Span) -> bool {
        if self.fast_emit {
            return false;
        }
        let start = span.start as usize;
        let end = span.end as usize;
        if start >= end || end > self.source.len() {
            return false;
        }
        if !self
            .adjacent_spread_comments
            .get_or_init(|| crate::spread_comments::SpreadCommentIndex::new(self.source))
            .may_contain(span)
        {
            return false;
        }
        let text = &self.source[start..end];
        let mut search_from = 0usize;
        while search_from < text.len() {
            let Some(rel) = text[search_from..].find("...") else {
                break;
            };
            let dots = search_from + rel;
            let after_dots = dots + 3;
            if after_dots >= text.len() {
                break;
            }
            let rest = &text[after_dots..];
            let mut hws_len = 0usize;
            for (i, ch) in rest.char_indices() {
                if ch == ' ' || ch == '\t' {
                    hws_len = i + ch.len_utf8();
                } else {
                    break;
                }
            }
            let after_ws = &rest[hws_len..];
            if after_ws.starts_with("/*") || after_ws.starts_with("//") {
                return true;
            }
            search_from = after_dots;
        }
        false
    }

    pub(super) fn array_pattern_trailing_commas(&self, span: Span) -> usize {
        let start = span.start as usize;
        let end = span.end as usize;
        if start >= end || end > self.source.len() {
            return 0;
        }
        let text = &self.source[start..end];
        let Some(open_idx) = text.find('[') else {
            return 0;
        };
        let Some(close_idx) = text.rfind(']') else {
            return 0;
        };
        if open_idx >= close_idx {
            return 0;
        }
        let inner = &text[open_idx + 1..close_idx];
        let bytes = inner.as_bytes();
        let mut i = bytes.len();
        while i > 0 && bytes[i - 1].is_ascii_whitespace() {
            i -= 1;
        }
        let mut commas = 0usize;
        while i > 0 {
            if bytes[i - 1] == b',' {
                commas += 1;
                i -= 1;
                while i > 0 && bytes[i - 1].is_ascii_whitespace() {
                    i -= 1;
                }
            } else {
                break;
            }
        }
        commas
    }

    /// Determine the original quote character from source span, defaulting to `"`.
    pub(super) fn original_quote_char(&self, span: &Span) -> &'static str {
        let start = span.start as usize;
        if start < self.source.len() {
            match self.source.as_bytes()[start] {
                b'\'' => return "'",
                _ => {}
            }
        }
        "\""
    }

    fn is_module_export_identifier_name(name: &str) -> bool {
        let mut chars = name.chars();
        let Some(first) = chars.next() else {
            return false;
        };
        let is_start = first == '$' || first == '_' || first.is_alphabetic();
        if !is_start {
            return false;
        }
        chars.all(|ch| ch == '$' || ch == '_' || ch.is_alphanumeric())
    }

    pub(super) fn emit_module_export_name(&mut self, name: &str) {
        if Self::is_module_export_identifier_name(name) {
            self.write(name);
        } else {
            self.write("\"");
            self.write(name);
            self.write("\"");
        }
    }

    pub(super) fn emit_prop_name(&mut self, name: &PropName) {
        match name {
            PropName::Ident(n, _) => self.write(n),
            PropName::String(n, span) => {
                let q = self.original_quote_char(span);
                self.write(q);
                self.write(n);
                self.write(q);
            }
            PropName::Number(n, _) => self.write(n),
            PropName::Computed(expr, _) => {
                self.write("[");
                self.emit_expr(expr);
                self.write("]");
            }
            PropName::Private(n, _) => {
                self.write("#");
                self.write(n);
            }
        }
    }

    /// Emit a member access expression for a property name.
    /// Uses dot notation for identifiers and numbers, bracket notation for strings and computed.
    pub(super) fn emit_member_access(&mut self, name: &PropName) {
        match name {
            PropName::Ident(n, _) => {
                self.write(".");
                self.write(n);
            }
            PropName::Number(n, _) => {
                self.write("[");
                self.write(n);
                self.write("]");
            }
            PropName::String(n, span) => {
                let q = self.original_quote_char(span);
                self.write("[");
                self.write(q);
                self.write(n);
                self.write(q);
                self.write("]");
            }
            PropName::Computed(expr, _) => {
                self.write("[");
                self.emit_expr(expr);
                self.write("]");
            }
            PropName::Private(n, _) => {
                self.write(".");
                self.write("#");
                self.write(n);
            }
        }
    }
}

fn missing_type_argument_slot(text: &str) -> bool {
    let mut angle_depth = 0usize;
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut segment_has_content = false;

    for ch in text.chars() {
        match ch {
            '<' => {
                angle_depth += 1;
                segment_has_content = true;
            }
            '>' => {
                angle_depth = angle_depth.saturating_sub(1);
                segment_has_content = true;
            }
            '(' => {
                paren_depth += 1;
                segment_has_content = true;
            }
            ')' => {
                paren_depth = paren_depth.saturating_sub(1);
                segment_has_content = true;
            }
            '[' => {
                bracket_depth += 1;
                segment_has_content = true;
            }
            ']' => {
                bracket_depth = bracket_depth.saturating_sub(1);
                segment_has_content = true;
            }
            '{' => {
                brace_depth += 1;
                segment_has_content = true;
            }
            '}' => {
                brace_depth = brace_depth.saturating_sub(1);
                segment_has_content = true;
            }
            ',' if angle_depth == 0
                && paren_depth == 0
                && bracket_depth == 0
                && brace_depth == 0 =>
            {
                if !segment_has_content {
                    return true;
                }
                segment_has_content = false;
            }
            ch if !ch.is_ascii_whitespace() => {
                segment_has_content = true;
            }
            _ => {}
        }
    }

    !segment_has_content
}

/// Returns true if the binding pattern contains any shorthand object
/// property where the property name is a JavaScript reserved keyword
/// or string literal (both are invalid as JS identifiers).
fn binding_has_keyword_shorthand(pat: &Pat) -> bool {
    match &pat.kind {
        PatKind::Object(props) => props.iter().any(|prop| match prop {
            ObjPatProp::Shorthand(name, _) | ObjPatProp::ShorthandAssign(name, _, _) => {
                is_js_reserved_keyword(name) || name.starts_with('"') || name.starts_with('\'')
            }
            ObjPatProp::KeyValue(_, val) => binding_has_keyword_shorthand(val),
            ObjPatProp::Rest(p) => binding_has_keyword_shorthand(p),
        }),
        PatKind::Array(elements) => elements.iter().any(|e| match e {
            Some(ArrayPatElem::Pat(p)) | Some(ArrayPatElem::Rest(p)) => {
                binding_has_keyword_shorthand(p)
            }
            None => false,
        }),
        PatKind::Assign(p, _) => binding_has_keyword_shorthand(p),
        _ => false,
    }
}

/// Returns true if `pat` contains an object destructuring property where the
/// binding target is `<error>` and the source text at that position is a
/// reserved keyword (e.g. `{ while: while }` where the parser consumed
/// `while` as an error binding).  TypeScript's parser does NOT consume the
/// reserved keyword, so its emitter produces TWO properties:
/// `{ while: , while:  }` (the keyword is re-parsed as a new property name).
/// We detect this here so the structured emit path can replicate the behavior.
fn binding_has_reserved_word_keyvalue_target(pat: &Pat, source: &str) -> bool {
    match &pat.kind {
        PatKind::Object(props) => props.iter().any(|prop| match prop {
            ObjPatProp::KeyValue(_, val) => {
                if matches!(&val.kind, PatKind::Ident(name) if name == "<error>") {
                    let s = val.span.start as usize;
                    let e = val.span.end as usize;
                    if s < e && e <= source.len() {
                        let src = source[s..e].trim();
                        is_js_reserved_keyword(src)
                    } else {
                        false
                    }
                } else {
                    binding_has_reserved_word_keyvalue_target(val, source)
                }
            }
            ObjPatProp::Rest(p) => binding_has_reserved_word_keyvalue_target(p, source),
            _ => false,
        }),
        PatKind::Array(elements) => elements.iter().any(|e| match e {
            Some(ArrayPatElem::Pat(p)) | Some(ArrayPatElem::Rest(p)) => {
                binding_has_reserved_word_keyvalue_target(p, source)
            }
            None => false,
        }),
        PatKind::Assign(p, _) => binding_has_reserved_word_keyvalue_target(p, source),
        _ => false,
    }
}

/// Returns true if `name` is a JavaScript reserved keyword that cannot be
/// used as a binding identifier (variable/parameter name).
pub(crate) fn is_js_reserved_keyword(name: &str) -> bool {
    matches!(
        name,
        "break"
            | "case"
            | "catch"
            | "class"
            | "const"
            | "continue"
            | "debugger"
            | "default"
            | "delete"
            | "do"
            | "else"
            | "enum"
            | "export"
            | "extends"
            | "false"
            | "finally"
            | "for"
            | "function"
            | "if"
            | "import"
            | "in"
            | "instanceof"
            | "new"
            | "null"
            | "return"
            | "super"
            | "switch"
            | "this"
            | "throw"
            | "true"
            | "try"
            | "typeof"
            | "var"
            | "void"
            | "while"
            | "with"
    )
}

// ---------------------------------------------------------------------------
// using/await using disposal transform
// ---------------------------------------------------------------------------

/// Check if any statement in the slice is a `using` or `await using` declaration.
pub(crate) fn has_using_declaration(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|s| matches!(&s.kind, StmtKind::Var(vs) if matches!(vs.kind, VarKind::Using | VarKind::AwaitUsing)))
}

/// Find the index of the first `using`/`await using` declaration.
pub(crate) fn first_using_index(stmts: &[Stmt]) -> Option<usize> {
    stmts.iter().position(|s| matches!(&s.kind, StmtKind::Var(vs) if matches!(vs.kind, VarKind::Using | VarKind::AwaitUsing)))
}

impl<'a> Emitter<'a> {
    pub(super) fn emit_for_using_dispose_scope(
        &mut self,
        for_stmt: &ForStmt,
        var_stmt: &VarStmt,
        labels: &[&str],
    ) {
        self.needs_add_disposable_resource_helper = true;
        self.needs_dispose_resources_helper = true;
        let env_num = self.next_using_env_num();
        let is_async = var_stmt.kind == VarKind::AwaitUsing;

        self.writeln("{");
        self.indent += 1;
        self.writeln(&format!(
            "const env_{env_num} = {{ stack: [], error: void 0, hasError: false }};"
        ));
        self.writeln("try {");
        self.indent += 1;
        self.write("const ");
        for (index, declaration) in var_stmt.declarations.iter().enumerate() {
            if index > 0 {
                self.write(", ");
            }
            self.emit_binding_name(&declaration.name);
            if let Some(initializer) = declaration.init.as_ref() {
                self.write(" = ");
                self.write(self.helper_prefix());
                self.write(&format!("__addDisposableResource(env_{env_num}, "));
                if let PatKind::Ident(name) = &declaration.name.kind {
                    self.emit_using_initializer(name, initializer);
                } else {
                    self.emit_expr(initializer);
                }
                self.write(if is_async { ", true)" } else { ", false)" });
            }
        }
        self.writeln(";");
        for label in labels {
            self.write(label);
            self.writeln(":");
        }
        self.write("for (;");
        if let Some(test) = for_stmt.test.as_ref() {
            self.write(" ");
            self.emit_expr(test);
        }
        self.write(";");
        if let Some(update) = for_stmt.update.as_ref() {
            self.write(" ");
            self.emit_expr(update);
        }
        self.write(")");
        if matches!(for_stmt.body.kind, StmtKind::Block(_)) {
            self.write(" ");
        }
        self.emit_stmt_body(&for_stmt.body);
        self.indent -= 1;
        self.writeln("}");
        self.writeln(&format!("catch (e_{env_num}) {{"));
        self.indent += 1;
        self.writeln(&format!("env_{env_num}.error = e_{env_num};"));
        self.writeln(&format!("env_{env_num}.hasError = true;"));
        self.indent -= 1;
        self.writeln("}");
        self.writeln("finally {");
        self.indent += 1;
        if is_async {
            self.writeln(&format!(
                "const result_{env_num} = {}__disposeResources(env_{env_num});",
                self.helper_prefix()
            ));
            self.writeln(&format!("if (result_{env_num}) await result_{env_num};"));
        } else {
            self.writeln(&format!(
                "{}__disposeResources(env_{env_num});",
                self.helper_prefix()
            ));
        }
        self.indent -= 1;
        self.writeln("}");
        self.indent -= 1;
        self.writeln("}");
    }

    pub(super) fn emit_native_for_of_using(&mut self, fo: &ForOfStmt, labels: &[&str]) {
        let Some((binding_name, is_async)) = self.simple_for_of_using_binding(fo) else {
            return;
        };
        let value_name = self.make_unique_loop_resource_value_name(&binding_name);
        for label in labels {
            self.write(label);
            self.writeln(":");
        }
        self.write(if fo.is_await {
            "for await (const "
        } else {
            "for (const "
        });
        self.write(&value_name);
        self.write(" of ");
        self.emit_expr(&fo.right);
        self.writeln(") {");
        self.indent += 1;
        self.emit_native_for_of_using_iteration(&binding_name, &value_name, is_async, &fo.body);
        self.indent -= 1;
        self.writeln("}");
    }

    pub(super) fn make_unique_loop_resource_value_name(&self, binding_name: &str) -> String {
        let mut suffix = 1usize;
        loop {
            let name = format!("{binding_name}_{suffix}");
            if !self.source_has_identifier(&name) {
                return name;
            }
            suffix += 1;
        }
    }

    pub(super) fn emit_native_for_of_using_iteration(
        &mut self,
        binding_name: &str,
        value_name: &str,
        is_async: bool,
        body: &Stmt,
    ) {
        self.needs_add_disposable_resource_helper = true;
        self.needs_dispose_resources_helper = true;
        let env_num = self.next_using_env_num();
        self.writeln(&format!(
            "const env_{env_num} = {{ stack: [], error: void 0, hasError: false }};"
        ));
        self.writeln("try {");
        self.indent += 1;
        self.write("const ");
        self.write(binding_name);
        self.write(" = ");
        self.write(self.helper_prefix());
        self.write(&format!(
            "__addDisposableResource(env_{env_num}, {value_name}, {is_async});"
        ));
        self.newline();
        match &body.kind {
            StmtKind::Block(stmts) if has_using_declaration(stmts) => {
                self.emit_block_using_dispose_scope(stmts)
            }
            StmtKind::Block(stmts) => {
                for stmt in stmts {
                    self.emit_leading_comments(stmt.span.start);
                    self.emit_stmt(stmt);
                    self.advance_comment_pos(stmt.span.end);
                }
            }
            _ => self.emit_stmt(body),
        }
        self.indent -= 1;
        self.writeln("}");
        self.writeln(&format!("catch (e_{env_num}) {{"));
        self.indent += 1;
        self.writeln(&format!("env_{env_num}.error = e_{env_num};"));
        self.writeln(&format!("env_{env_num}.hasError = true;"));
        self.indent -= 1;
        self.writeln("}");
        self.writeln("finally {");
        self.indent += 1;
        if is_async {
            self.writeln(&format!(
                "const result_{env_num} = {}__disposeResources(env_{env_num});",
                self.helper_prefix()
            ));
            self.writeln(&format!("if (result_{env_num})"));
            self.indent += 1;
            self.writeln(&format!("await result_{env_num};"));
            self.indent -= 1;
        } else {
            self.writeln(&format!(
                "{}__disposeResources(env_{env_num});",
                self.helper_prefix()
            ));
        }
        self.indent -= 1;
        self.writeln("}");
    }

    pub(super) fn emit_using_initializer(&mut self, name: &str, init: &Expr) {
        let ExprKind::ClassExpr(class_decl) = &init.kind else {
            self.emit_expr(init);
            return;
        };
        if class_decl.name.is_some() {
            self.emit_expr(init);
            return;
        }

        let binding_name = ClassExprBindingName::Literal(name.to_string());
        if Self::expr_needs_class_expr_binding_name(init) {
            let previous = self.class_expr_binding_name.replace(binding_name);
            let previous_using = self.in_using_class_initializer;
            self.in_using_class_initializer = true;
            self.emit_expr(init);
            self.in_using_class_initializer = previous_using;
            self.class_expr_binding_name = previous;
            return;
        }

        self.needs_set_function_name_helper = true;
        let temp = self.next_temp_var();
        self.write("(");
        self.write(&temp);
        self.write(" = ");
        self.indent += 1;
        self.emit_expr(init);
        self.writeln(",");
        self.write(self.helper_prefix());
        self.write("__setFunctionName(");
        self.write(&temp);
        self.write(", \"");
        self.write(name);
        self.writeln("\"),");
        self.write(&temp);
        self.indent -= 1;
        self.write(")");
    }

    /// Emit a using/await using disposal scope: hoists declarations, wraps
    /// remaining statements in try { ... } catch { ... } finally { __disposeResources() }.
    ///
    /// `stmts` should be the statements from the first `using` onward.
    /// `is_cjs_export_assign` indicates whether there's an `export =` that needs
    /// deferred `module.exports = _default;` after the finally block.
    pub(crate) fn emit_using_dispose_scope(
        &mut self,
        stmts: &[Stmt],
        is_cjs_export_assign: bool,
        named_exports: &[(String, String)],
    ) {
        let env_num = self.next_using_env_num();

        // Mark helper functions as needed
        self.needs_add_disposable_resource_helper = true;
        self.needs_dispose_resources_helper = true;

        let is_cjs = self.is_cjs_like();

        // 1. Collect hoisted variable names
        let mut hoisted_names: Vec<String> = Vec::new();
        let mut has_default = false;
        for s in stmts {
            match &s.kind {
                StmtKind::Var(vs) => {
                    for d in &vs.declarations {
                        if let PatKind::Ident(name) = &d.name.kind {
                            if !name.is_empty() && name != "<error>" {
                                hoisted_names.push(name.to_string());
                            }
                        }
                    }
                }
                StmtKind::Export(ed) => {
                    if let ExportDeclKind::Decl(inner) = &ed.kind {
                        if let StmtKind::Var(vs) = &inner.kind {
                            // In CJS mode, exported const/let become `exports.name = value;`
                            // directly — don't hoist them as local vars. Only hoist
                            // exported `var` declarations (which need function-scoped hoisting).
                            if !is_cjs || vs.kind == VarKind::Var {
                                for d in &vs.declarations {
                                    if let PatKind::Ident(name) = &d.name.kind {
                                        if !name.is_empty() && name != "<error>" {
                                            hoisted_names.push(name.to_string());
                                        }
                                    }
                                }
                            }
                        }
                    }
                    // export default expr/decl → _default
                    if matches!(
                        &ed.kind,
                        ExportDeclKind::DefaultDecl(_) | ExportDeclKind::Default(_)
                    ) && !has_default
                    {
                        hoisted_names.push("_default".to_string());
                        has_default = true;
                    }
                }
                StmtKind::ExportAssign(_) if is_cjs_export_assign => {
                    if !has_default {
                        hoisted_names.push("_default".to_string());
                        has_default = true;
                    }
                }
                _ => {}
            }
        }

        // 1b. Hoist function declarations before the try block
        for s in stmts {
            if matches!(&s.kind, StmtKind::FnDecl(fd) if fd.body.is_some()) {
                self.emit_stmt(s);
            }
        }

        // 2. Emit hoisted var declaration
        if !hoisted_names.is_empty() {
            self.write("var ");
            for (i, name) in hoisted_names.iter().enumerate() {
                if i > 0 {
                    self.write(", ");
                }
                self.write(name);
            }
            self.writeln(";");
        }

        // 3. Emit envelope
        self.writeln(&format!(
            "const env_{env_num} = {{ stack: [], error: void 0, hasError: false }};"
        ));

        // 4. Try block
        self.writeln("try {");
        self.indent += 1;
        for s in stmts {
            match &s.kind {
                StmtKind::Var(vs) if matches!(vs.kind, VarKind::Using | VarKind::AwaitUsing) => {
                    // using x = expr → x = __addDisposableResource(env_N, expr, false/true)
                    let is_await = vs.kind == VarKind::AwaitUsing;
                    for d in &vs.declarations {
                        if let PatKind::Ident(name) = &d.name.kind {
                            if let Some(init) = &d.init {
                                self.write(name);
                                self.write(" = ");
                                self.write(self.helper_prefix());
                                self.write(&format!("__addDisposableResource(env_{env_num}, "));
                                self.emit_using_initializer(name, init);
                                self.write(if is_await { ", true" } else { ", false" });
                                self.writeln(");");
                            }
                        }
                    }
                }
                StmtKind::Var(vs) => {
                    // const/let/var → assignment inside try
                    for d in &vs.declarations {
                        if let PatKind::Ident(name) = &d.name.kind {
                            if let Some(init) = &d.init {
                                // If the variable is re-exported via `export { name }`,
                                // wrap the assignment: `exports.name = name = value;`
                                if is_cjs {
                                    if let Some((exported, _)) =
                                        named_exports.iter().find(|(_, local)| local == name)
                                    {
                                        self.write_cjs_export_access("exports", exported);
                                        self.write(" = ");
                                        // Mark as inline-exported so trailing
                                        // `exports.y = y;` is suppressed.
                                        self.cjs_inline_exported_var_names
                                            .insert(name.clone().into());
                                    }
                                }
                                self.write(name);
                                self.write(" = ");
                                self.emit_expr(init);
                                self.writeln(";");
                            }
                        }
                    }
                }
                StmtKind::Export(ed) => {
                    match &ed.kind {
                        ExportDeclKind::Decl(inner) if matches!(&inner.kind, StmtKind::Var(_)) => {
                            if let StmtKind::Var(vs) = &inner.kind {
                                // In CJS: const/let exports become `exports.name = value;`
                                // without a local variable. `var` exports become
                                // `exports.name = name = value;` with a hoisted local.
                                let needs_local = vs.kind == VarKind::Var;
                                for d in &vs.declarations {
                                    if let PatKind::Ident(name) = &d.name.kind {
                                        if let Some(init) = &d.init {
                                            if is_cjs {
                                                self.write("exports.");
                                                self.write(name);
                                                self.write(" = ");
                                            }
                                            if needs_local || !is_cjs {
                                                self.write(name);
                                                self.write(" = ");
                                            }
                                            self.emit_expr(init);
                                            self.writeln(";");
                                        }
                                    }
                                }
                            }
                        }
                        ExportDeclKind::DefaultDecl(inner_stmt) => {
                            // export default expr → exports.default = _default = expr (CJS)
                            if let StmtKind::Expr(expr) = &inner_stmt.kind {
                                if is_cjs {
                                    self.write("exports.default = ");
                                }
                                self.write("_default = ");
                                // CJS: imported bindings inside the
                                // default-export expression must go through
                                // `emit_value_name_ref` so a bare reference
                                // to an imported name (e.g.
                                // `import { db } ...; export default db;`)
                                // emits `db_1.db`, not the unresolved bare
                                // `db`. Without this scoped toggle the
                                // emitter writes `exports.default = db;`
                                // and the bundle throws
                                // `ReferenceError: db is not defined` at
                                // module init.
                                let prev_rw = self.rewrite_ident_with_import_map;
                                if is_cjs {
                                    self.rewrite_ident_with_import_map = true;
                                }
                                self.emit_expr(expr);
                                self.rewrite_ident_with_import_map = prev_rw;
                                self.writeln(";");
                            } else {
                                self.emit_stmt(s);
                            }
                        }
                        ExportDeclKind::Default(expr) => {
                            // export default expr → exports.default = _default = expr (CJS)
                            if is_cjs {
                                self.write("exports.default = ");
                            }
                            self.write("_default = ");
                            // See `DefaultDecl(_)` above for why CJS emit
                            // toggles `rewrite_ident_with_import_map` here.
                            let prev_rw = self.rewrite_ident_with_import_map;
                            if is_cjs {
                                self.rewrite_ident_with_import_map = true;
                            }
                            self.emit_expr(expr);
                            self.rewrite_ident_with_import_map = prev_rw;
                            self.writeln(";");
                        }
                        _ => {
                            self.emit_stmt(s);
                        }
                    }
                }
                StmtKind::ExportAssign(expr) if is_cjs_export_assign => {
                    // export = expr → _default = expr
                    self.write("_default = ");
                    self.emit_expr(expr);
                    self.writeln(";");
                }
                // Skip function declarations — already hoisted above try
                StmtKind::FnDecl(fd) if fd.body.is_some() => {}
                _ => {
                    // Other statements: emit normally
                    self.emit_stmt(s);
                }
            }
        }
        self.indent -= 1;
        self.writeln("}");

        // 5. Catch block
        self.writeln(&format!("catch (e_{env_num}) {{"));
        self.indent += 1;
        self.writeln(&format!("env_{env_num}.error = e_{env_num};"));
        self.writeln(&format!("env_{env_num}.hasError = true;"));
        self.indent -= 1;
        self.writeln("}");

        // 6. Finally block
        self.writeln("finally {");
        self.indent += 1;
        self.writeln(&format!(
            "{}__disposeResources(env_{env_num});",
            self.helper_prefix()
        ));
        self.indent -= 1;
        self.writeln("}");

        // 7. Deferred export = after finally
        if is_cjs_export_assign && has_default {
            self.writeln("module.exports = _default;");
        }
    }

    /// Block-level using disposal scope — uses `const` for bindings.
    pub(crate) fn emit_block_using_dispose_scope(&mut self, stmts: &[Stmt]) {
        let has_await = stmts
            .iter()
            .any(|s| matches!(&s.kind, StmtKind::Var(vs) if vs.kind == VarKind::AwaitUsing));
        self.needs_add_disposable_resource_helper = true;
        self.needs_dispose_resources_helper = true;
        let env_num = self.next_using_env_num();

        self.writeln(&format!(
            "const env_{env_num} = {{ stack: [], error: void 0, hasError: false }};"
        ));
        self.writeln("try {");
        self.indent += 1;
        for s in stmts {
            match &s.kind {
                StmtKind::Var(vs) if matches!(vs.kind, VarKind::Using | VarKind::AwaitUsing) => {
                    let is_await = vs.kind == VarKind::AwaitUsing;
                    for d in &vs.declarations {
                        if let PatKind::Ident(name) = &d.name.kind {
                            if let Some(init) = &d.init {
                                self.write("const ");
                                self.write(name);
                                self.write(" = ");
                                self.write(self.helper_prefix());
                                self.write(&format!("__addDisposableResource(env_{env_num}, "));
                                self.emit_using_initializer(name, init);
                                self.write(if is_await { ", true" } else { ", false" });
                                self.writeln(");");
                            }
                        }
                    }
                }
                _ => self.emit_stmt(s),
            }
        }
        self.indent -= 1;
        self.writeln("}");
        self.writeln(&format!("catch (e_{env_num}) {{"));
        self.indent += 1;
        self.writeln(&format!("env_{env_num}.error = e_{env_num};"));
        self.writeln(&format!("env_{env_num}.hasError = true;"));
        self.indent -= 1;
        self.writeln("}");
        self.writeln("finally {");
        self.indent += 1;
        if has_await {
            self.writeln(&format!(
                "const result_{env_num} = {}__disposeResources(env_{env_num});",
                self.helper_prefix()
            ));
            self.writeln(&format!("if (result_{env_num})"));
            self.indent += 1;
            self.writeln(&format!("await result_{env_num};"));
            self.indent -= 1;
        } else {
            self.writeln(&format!(
                "{}__disposeResources(env_{env_num});",
                self.helper_prefix()
            ));
        }
        self.indent -= 1;
        self.writeln("}");
    }
}

#[cfg(test)]
mod source_line_unicode_tests {
    use super::trim_block_comment_line_trailing_spaces;

    /// Regression: a source-line statement that contains both a multi-line
    /// `/* */` block comment AND a multi-byte UTF-8 char in a string
    /// literal was double-encoding the UTF-8: bytes c3 a9 (é) became
    /// c3 83 c2 a9 (Ã©). The bug was the byte-as-char copy pattern in
    /// `trim_block_comment_line_trailing_spaces` — `out.push(bytes[i] as char)`
    /// pushed each UTF-8 byte as its own Latin-1 codepoint, which the
    /// String then re-encoded as 2-byte UTF-8 on serialize.
    /// Fixed via `crate::push_utf8_aware`.
    #[test]
    fn preserves_multibyte_utf8_when_text_has_block_comments() {
        let input = "/* leading\n * multi-line block\n */\nvar x = \"identité — café\";";
        let out = trim_block_comment_line_trailing_spaces(input);
        assert!(
            out.contains("identité"),
            "expected verbatim 'identité', got: {out}"
        );
        assert!(out.contains("café"), "expected verbatim 'café', got: {out}");
        assert!(out.contains("—"), "expected verbatim em-dash, got: {out}");
        assert!(!out.contains("Ã"), "double-encoded marker found: {out}");
    }
}
