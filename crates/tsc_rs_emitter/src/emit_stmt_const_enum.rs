//! Const enum inlining and reference resolution for the emitter.

use super::*;

impl<'a> Emitter<'a> {
    pub(crate) fn import_equals_rhs_is_type_only_require_spec(&self, rhs: &Expr) -> bool {
        matches!(&rhs.kind, ExprKind::Call(call)
            if matches!(&call.callee.kind, ExprKind::Ident(n) if n == "require")
                && call.args.first().and_then(|arg| match &arg.kind {
                    ExprKind::StrLit(spec) | ExprKind::NoSubstTemplate(spec) => Some(spec.as_str()),
                    _ => None,
                }).is_some_and(|spec| self.type_only_require_specs.contains(spec)))
    }

    fn member_path_is_const_enum_object(&self, path: &str) -> bool {
        self.const_enum_object_names.contains(path)
            || self
                .const_enum_object_names
                .iter()
                .any(|name| name.ends_with(&format!(".{path}")))
    }

    /// Returns true when an import-equals RHS expression resolves to a known
    /// runtime value in the current emit context.
    pub(crate) fn import_equals_rhs_has_runtime_value(&self, rhs: &Expr) -> bool {
        match &rhs.kind {
            ExprKind::Ident(name) => {
                self.emitted_var_names.contains(name.as_str())
                    || self.cjs_import_map.contains_key(name.as_str())
                    // `declare namespace` with value exports (class/enum/etc.)
                    // has runtime value for import-equals purposes.
                    || self.declare_ns_with_values.contains(name.as_str())
                    // If the name is not known to be type-only, assume it has
                    // a runtime value.  This handles undeclared/external names
                    // like `no` or `undefined` that TypeScript still emits.
                    || !self.type_only_decl_names.contains(name.as_str())
            }
            ExprKind::Member(mem) => {
                if let Some(path) = self.expr_member_path(rhs) {
                    if self.should_inline_const_enums()
                        && !self.preserve_const_enums_effective()
                        && self.member_path_is_const_enum_object(path.as_str())
                    {
                        return false;
                    }
                    if self.has_emitted_runtime_member_path(&path) {
                        return true;
                    }
                    // If this is a known namespace member path, consult the
                    // pre-scanned runtime namespace export map. This avoids
                    // treating erased `export import` aliases as runtime values.
                    if let Some((root, rest)) = path.split_once('.') {
                        let first_member = rest.split('.').next().unwrap_or(rest);
                        let top_level_key: AstString = root.into();
                        let cjs_key: AstString = format!("exports::{root}").into();
                        let suffix_key = format!("::{root}");
                        let mut namespace_keys: Vec<AstString> =
                            vec![top_level_key.clone(), cjs_key.clone()];
                        for key in self.cumulative_ns_exports.keys() {
                            if key.ends_with(&suffix_key) && !namespace_keys.contains(key) {
                                namespace_keys.push(key.clone());
                            }
                        }
                        for key in self.cumulative_ns_type_exports.keys() {
                            if key.ends_with(&suffix_key) && !namespace_keys.contains(key) {
                                namespace_keys.push(key.clone());
                            }
                        }
                        let has_namespace_info = namespace_keys.iter().any(|key| {
                            self.cumulative_ns_exports.contains_key(key)
                                || self.cumulative_ns_type_exports.contains_key(key)
                        });
                        let has_runtime_export = namespace_keys.iter().any(|key| {
                            self.cumulative_ns_exports
                                .get(key)
                                .is_some_and(|set| set.contains(first_member))
                        });
                        if has_runtime_export {
                            return true;
                        }
                        if has_namespace_info {
                            // Only erase if the member is a known type-only export.
                            // If the member is not declared at all (e.g., `toString`),
                            // it could be a runtime property — preserve it.
                            let is_type_only = namespace_keys.iter().any(|key| {
                                self.cumulative_ns_type_exports
                                    .get(key)
                                    .is_some_and(|set| set.contains(first_member))
                            });
                            return !is_type_only;
                        }
                    }
                }
                // Member access off an imported module value is runtime-reachable.
                if let ExprKind::Ident(obj) = &mem.object.kind {
                    if self.cjs_import_map.contains_key(obj.as_str()) {
                        return true;
                    }
                }
                // For deeper member chains (3+ segments like foo.bar.baz),
                // check if the parent member path has been emitted as a
                // runtime namespace. This handles IIFE-local parameter names
                // that prevent matching the full qualified path in output.
                if let ExprKind::Member(_) = &mem.object.kind {
                    if let Some(parent_path) = self.expr_member_path(&mem.object) {
                        if self.has_emitted_runtime_member_path(&parent_path) {
                            return true;
                        }
                    }
                }
                // Check the root of the member chain.
                let root = self.expr_root_ident(rhs);
                if let Some(ref root_name) = root {
                    // If the root is an emitted runtime value (e.g. a namespace
                    // var), member access on it is also runtime.
                    if self.emitted_var_names.contains(root_name.as_str()) {
                        return true;
                    }
                    // `declare namespace` with value exports: `import ab = A.B`
                    // where `declare namespace A.B { export class C {} }`.
                    if self.declare_ns_with_values.contains(root_name.as_str()) {
                        return true;
                    }
                    // If the root is undeclared (not in any tracking set),
                    // treat it as an external runtime value.
                    // E.g. `import m2 = no.mod;` where `no` is not declared.
                    if !self.type_only_decl_names.contains(root_name.as_str())
                        && !self.cjs_import_map.contains_key(root_name.as_str())
                    {
                        return true;
                    }
                }
                false
            }
            ExprKind::Paren(inner) | ExprKind::NonNull(inner) => {
                self.import_equals_rhs_has_runtime_value(inner)
            }
            ExprKind::TypeAssertion(ta) => self.import_equals_rhs_has_runtime_value(&ta.expr),
            ExprKind::As(a) => self.import_equals_rhs_has_runtime_value(&a.expr),
            ExprKind::Satisfies(s) => self.import_equals_rhs_has_runtime_value(&s.expr),
            ExprKind::Instantiation(inst) => self.import_equals_rhs_has_runtime_value(&inst.expr),
            _ => true,
        }
    }

    /// Build a dotted member path for identifier/member expressions.
    pub(crate) fn expr_member_path(&self, expr: &Expr) -> Option<String> {
        match &expr.kind {
            ExprKind::Ident(name) => Some(name.to_string()),
            ExprKind::Member(mem) => {
                let mut base = self.expr_member_path(&mem.object)?;
                base.push('.');
                base.push_str(&mem.property);
                Some(base)
            }
            ExprKind::Paren(inner) | ExprKind::NonNull(inner) => self.expr_member_path(inner),
            ExprKind::TypeAssertion(ta) => self.expr_member_path(&ta.expr),
            ExprKind::As(a) => self.expr_member_path(&a.expr),
            ExprKind::Satisfies(s) => self.expr_member_path(&s.expr),
            ExprKind::Instantiation(inst) => self.expr_member_path(&inst.expr),
            _ => None,
        }
    }

    /// Extract the root identifier from a (possibly nested) member expression.
    /// E.g. `foo.bar.baz` → `Some("foo")`.
    pub(crate) fn expr_root_ident(&self, expr: &Expr) -> Option<String> {
        match &expr.kind {
            ExprKind::Ident(name) => Some(name.to_string()),
            ExprKind::Member(mem) => self.expr_root_ident(&mem.object),
            ExprKind::Paren(inner) | ExprKind::NonNull(inner) => self.expr_root_ident(inner),
            ExprKind::TypeAssertion(ta) => self.expr_root_ident(&ta.expr),
            ExprKind::As(a) => self.expr_root_ident(&a.expr),
            ExprKind::Satisfies(s) => self.expr_root_ident(&s.expr),
            ExprKind::Instantiation(inst) => self.expr_root_ident(&inst.expr),
            _ => None,
        }
    }

    pub(crate) fn span_text(&self, span: Span) -> Option<&str> {
        let start = span.start as usize;
        let end = span.end as usize;
        if start >= end || end > self.source.len() {
            return None;
        }
        Some(&self.source[start..end])
    }

    /// In script files, keep some non-runtime `import x = Y` aliases to match
    /// TS cross-file duplicate handling. If `x` hasn't been declared earlier
    /// (in prior script files or earlier in this file), TS preserves the alias
    /// even when `Y` resolves to a type-only namespace.
    pub(crate) fn should_preserve_script_type_only_import_equals(
        &self,
        alias: &str,
        rhs: &Expr,
    ) -> bool {
        // Only applies to top-level script emit.
        if self.is_module_file || self.export_target.is_some() || self.fn_scope_depth > 0 {
            return false;
        }
        let ExprKind::Ident(rhs_name) = &rhs.kind else {
            return false;
        };
        if !self.type_only_decl_names.contains(rhs_name.as_str()) {
            return false;
        }
        if self.type_only_decl_names.contains(alias) {
            return false;
        }
        if self.seen_script_value_names.contains(alias) {
            return false;
        }
        if self.prior_script_value_names.contains(alias) {
            return false;
        }
        true
    }

    /// Return `(enum_object, member_name, comment_ref)` for const-enum access
    /// expressions that can be inlined.
    pub(crate) fn const_enum_ref_parts(&self, expr: &Expr) -> Option<(String, String, String)> {
        match &expr.kind {
            ExprKind::Member(mem) => {
                let obj = self.expr_member_path(&mem.object)?;
                let comment_ref = format!("{obj}.{}", mem.property);
                Some((obj, mem.property.to_string(), comment_ref))
            }
            ExprKind::ElemAccess(ea) => {
                let obj = self.expr_member_path(&ea.object)?;
                let member = match &ea.index.kind {
                    ExprKind::StrLit(s) | ExprKind::NoSubstTemplate(s) => s.to_string(),
                    ExprKind::NumLit(n) => n.to_string(),
                    _ => return None,
                };
                let comment_ref = self
                    .span_text(expr.span)
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| format!("{obj}[\"{member}\"]"));
                Some((obj, member, comment_ref))
            }
            _ => None,
        }
    }

    pub(crate) fn format_const_enum_number(value: f64) -> String {
        if value == 0.0 {
            return "0".to_string();
        }
        if value.is_finite() && value.fract() == 0.0 {
            return format!("{value:.0}");
        }
        let mut out = value.to_string();
        if out.contains('E') {
            out = out.to_ascii_lowercase();
        }
        out
    }

    pub(crate) fn decode_const_enum_member_name(raw: &str) -> Option<String> {
        if !raw.contains('\\') {
            return None;
        }
        let bytes = raw.as_bytes();
        let mut i = 0usize;
        let mut out = String::with_capacity(raw.len());
        while i < bytes.len() {
            let b = bytes[i];
            if b != b'\\' {
                let ch = raw[i..].chars().next()?;
                out.push(ch);
                i += ch.len_utf8();
                continue;
            }
            i += 1;
            if i >= bytes.len() {
                return None;
            }
            let esc = bytes[i] as char;
            match esc {
                'u' => {
                    i += 1;
                    if i < bytes.len() && bytes[i] == b'{' {
                        i += 1;
                        let start = i;
                        while i < bytes.len() && bytes[i] != b'}' {
                            i += 1;
                        }
                        if i >= bytes.len() || i == start {
                            return None;
                        }
                        let hex = &raw[start..i];
                        let code = u32::from_str_radix(hex, 16).ok()?;
                        let ch = char::from_u32(code)?;
                        out.push(ch);
                        i += 1; // consume '}'
                    } else {
                        if i + 4 > bytes.len() {
                            return None;
                        }
                        let hex = &raw[i..i + 4];
                        let code = u32::from_str_radix(hex, 16).ok()?;
                        let ch = char::from_u32(code)?;
                        out.push(ch);
                        i += 4;
                    }
                }
                'n' => {
                    out.push('\n');
                    i += 1;
                }
                'r' => {
                    out.push('\r');
                    i += 1;
                }
                't' => {
                    out.push('\t');
                    i += 1;
                }
                'b' => {
                    out.push('\u{0008}');
                    i += 1;
                }
                'f' => {
                    out.push('\u{000C}');
                    i += 1;
                }
                'v' => {
                    out.push('\u{000B}');
                    i += 1;
                }
                '0' => {
                    out.push('\0');
                    i += 1;
                }
                '\\' => {
                    out.push('\\');
                    i += 1;
                }
                '\'' => {
                    out.push('\'');
                    i += 1;
                }
                '"' => {
                    out.push('"');
                    i += 1;
                }
                '`' => {
                    out.push('`');
                    i += 1;
                }
                other => {
                    out.push(other);
                    i += 1;
                }
            }
        }
        Some(out)
    }

    pub(crate) fn emit_inlined_const_enum_value(
        &mut self,
        value: &ConstEnumValue,
        comment_ref: &str,
    ) {
        // Escape `*/` in comment text to prevent premature comment closure
        let safe_comment = comment_ref.replace("*/", "*_/");
        let safe_comment = safe_comment.as_str();
        match value {
            ConstEnumValue::String(s) => {
                self.write("\"");
                self.write(s);
                self.write("\"");
                if self.options.remove_comments != Some(true) {
                    self.write(" /* ");
                    self.write(safe_comment);
                    self.write(" */");
                }
            }
            ConstEnumValue::Number(n) => {
                let include_comment = self.options.remove_comments != Some(true);
                let negative = *n < 0.0 || (*n == 0.0 && n.is_sign_negative());
                let mut number_text = Self::format_const_enum_number(*n);
                let integer_literal = n.is_finite() && n.fract() == 0.0;
                if self.in_member_object_context && !include_comment && !negative && integer_literal
                {
                    number_text.push('.');
                }
                let needs_parens = self.in_member_object_context && negative;
                if needs_parens {
                    self.write("(");
                }
                self.write(&number_text);
                if include_comment {
                    self.write(" /* ");
                    self.write(safe_comment);
                    self.write(" */");
                }
                if needs_parens {
                    self.write(")");
                }
            }
        }
    }

    pub(crate) fn try_emit_const_enum_ref(&mut self, expr: &Expr) -> bool {
        let Some((obj_name, member_name, comment_ref)) = self.const_enum_ref_parts(expr) else {
            return false;
        };
        let mut value = self
            .const_enum_values
            .get(&(obj_name.clone(), member_name.clone()))
            .cloned();
        if value.is_none() {
            if let Some(decoded) = Self::decode_const_enum_member_name(&member_name) {
                value = self.const_enum_values.get(&(obj_name, decoded)).cloned();
            }
        }
        let Some(value) = value else {
            return false;
        };
        self.record_mapping(expr.span);
        self.emit_inlined_const_enum_value(&value, &comment_ref);
        true
    }

    /// Check whether a namespace/member path has already been emitted with a
    /// runtime assignment in output.
    pub(crate) fn has_emitted_runtime_member_path(&self, path: &str) -> bool {
        let direct_assign = format!("{path} =");
        if self.output.contains(&direct_assign) {
            return true;
        }
        let ns_or_enum_iife_assign = format!("{path} || ({path} =");
        if self.output.contains(&ns_or_enum_iife_assign) {
            return true;
        }
        // When a namespace IIFE parameter is renamed (e.g. `m` -> `m_1`),
        // the export assignment uses the renamed form (`m_1.prop =`).
        // Check for `<root>_N.<rest> =` patterns in the output.
        if let Some(dot_pos) = path.find('.') {
            let root = &path[..dot_pos];
            let rest = &path[dot_pos + 1..];
            for counter in 1..=10 {
                let renamed = format!("{}_{}.{} =", root, counter, rest);
                if self.output.contains(&renamed) {
                    return true;
                }
            }
            // Enum members use bracket notation: `Root[Root["Member"] = ...]`.
            // Check for this pattern to recognize enum member assignments.
            let bracket_assign = format!("{}[\"{}\"] =", root, rest);
            if self.output.contains(&bracket_assign) {
                return true;
            }
        }
        false
    }
}
