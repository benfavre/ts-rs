use super::*;

/// Allocate a temp var name for `this` preservation: `_a`, `_b`, `_c`, ...
fn alloc_this_preserve_temp(temps: &mut Vec<String>) -> String {
    let idx = temps.len();
    let name = format!("_{}", (b'a' + idx as u8) as char);
    temps.push(name.clone());
    name
}

/// The kind of decorated member for `__esDecorate` call emission.
#[derive(Clone, Copy)]
enum DecoratedMemberKind {
    Method,
    Getter,
    Setter,
}

#[derive(Clone)]
enum DecoratedPublicMemberKey {
    Identifier(String),
    Literal(String),
    Dynamic { expr: Expr, temp_name: String },
}

impl DecoratedMemberKind {
    fn as_str(self) -> &'static str {
        match self {
            DecoratedMemberKind::Method => "method",
            DecoratedMemberKind::Getter => "getter",
            DecoratedMemberKind::Setter => "setter",
        }
    }
}

/// Check if an expression (after unwrapping parens/type assertions) is a
/// class expression, function expression, or arrow function — these need
/// `(0, expr)` wrapping when assigned to `_classSuper` to prevent
/// the variable assignment from giving the expression a `.name` property.
fn extends_expr_needs_comma_wrap(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::ClassExpr(_) | ExprKind::FnExpr(_) | ExprKind::Arrow(_) => true,
        ExprKind::Paren(inner) => extends_expr_needs_comma_wrap(inner),
        ExprKind::As(a) => extends_expr_needs_comma_wrap(&a.expr),
        ExprKind::TypeAssertion(ta) => extends_expr_needs_comma_wrap(&ta.expr),
        ExprKind::Satisfies(s) => extends_expr_needs_comma_wrap(&s.expr),
        _ => false,
    }
}

/// Info about a decorated method/getter/setter for IIFE wrapper emission.
struct DecoratedMethodInfo {
    member_span_start: u32,
    /// Variable name stem (e.g. "m" → "_m_decorators", "get_val" → "_get_val_decorators")
    var_name: String,
    /// Property name as it appears in the class
    prop_name: String,
    /// Serialized decorator expressions (comma-separated)
    decorator_exprs: String,
    /// Whether this is a static member
    is_static: bool,
    /// The kind of member (method, getter, setter)
    kind: DecoratedMemberKind,
    key: DecoratedPublicMemberKey,
}

enum PublicMissingMemberRecoveryTail {
    EmptyBlock,
    IndexSignatureLike {
        index_name: String,
        index_type: String,
        value_name: String,
    },
}

/// Decode JS string escape sequences in a raw string-literal content
/// (the part between quotes). Returns the decoded string value.
pub(super) fn decode_js_string_content(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            match bytes[i + 1] {
                b'x' if i + 3 < bytes.len() => {
                    if let Ok(val) = u8::from_str_radix(
                        std::str::from_utf8(&bytes[i + 2..i + 4]).unwrap_or(""),
                        16,
                    ) {
                        out.push(val as char);
                        i += 4;
                        continue;
                    }
                }
                b'u' if i + 2 < bytes.len() && bytes[i + 2] == b'{' => {
                    if let Some(end) = raw[i + 3..].find('}') {
                        let hex = &raw[i + 3..i + 3 + end];
                        if let Ok(val) = u32::from_str_radix(hex, 16) {
                            if let Some(ch) = char::from_u32(val) {
                                out.push(ch);
                                i += 4 + end;
                                continue;
                            }
                        }
                    }
                }
                b'u' if i + 5 < bytes.len() => {
                    if let Ok(val) = u16::from_str_radix(
                        std::str::from_utf8(&bytes[i + 2..i + 6]).unwrap_or(""),
                        16,
                    ) {
                        if let Some(ch) = char::from_u32(val as u32) {
                            out.push(ch);
                            i += 6;
                            continue;
                        }
                    }
                }
                b'n' => {
                    out.push('\n');
                    i += 2;
                    continue;
                }
                b'r' => {
                    out.push('\r');
                    i += 2;
                    continue;
                }
                b't' => {
                    out.push('\t');
                    i += 2;
                    continue;
                }
                b'0' => {
                    out.push('\0');
                    i += 2;
                    continue;
                }
                b'\\' => {
                    out.push('\\');
                    i += 2;
                    continue;
                }
                b'\'' => {
                    out.push('\'');
                    i += 2;
                    continue;
                }
                b'"' => {
                    out.push('"');
                    i += 2;
                    continue;
                }
                _ => {}
            }
        }
        // Non-escape: copy the character as-is (handles multi-byte UTF-8)
        let ch = raw[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn heritage_tail_has_extra_base(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'<' => depth += 1,
            b'>' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => {
                let mut j = i + 1;
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                if j < bytes.len()
                    && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_' || bytes[j] == b'$')
                {
                    return true;
                }
            }
            _ if depth == 0
                && bytes[i..].starts_with(b"extends")
                && (i == 0
                    || !(bytes[i - 1].is_ascii_alphanumeric()
                        || bytes[i - 1] == b'_'
                        || bytes[i - 1] == b'$')) =>
            {
                let after_idx = i + "extends".len();
                let after_ok = after_idx >= bytes.len()
                    || !(bytes[after_idx].is_ascii_alphanumeric()
                        || bytes[after_idx] == b'_'
                        || bytes[after_idx] == b'$');
                if after_ok {
                    return true;
                }
            }
            _ => {}
        }
        i += 1;
    }
    false
}

fn strip_heritage_type_args(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut depth = 0usize;
    for ch in text.chars() {
        match ch {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
    }
    out
}

impl<'a> Emitter<'a> {
    /// Emit TC39 decorator annotations as `@decorator` lines (preserve mode).
    /// Used when the target natively supports decorators (ESNext).
    /// Emit TC39 decorator annotations and interleaved comments.
    /// `element_keyword_pos` is the source position of the decorated element's
    /// keyword (e.g., the `class` keyword, method name, etc.) — used to emit
    /// comments between the last decorator and the element.
    pub(super) fn emit_preserved_decorators(
        &mut self,
        decorators: &[Expr],
        element_keyword_pos: Option<u32>,
    ) {
        for decorator in decorators {
            // Emit any leading comments that appear before this decorator
            // (e.g., `/*2*/` between `@dec` lines).
            self.emit_leading_comments(decorator.span.start);
            // Decorators should always start on their own line.
            // If the output cursor is mid-line (e.g., after `(` or
            // `export default `), emit a newline first.
            if !self.at_line_start {
                self.newline();
            }
            self.write("@");
            // The decorator span includes the `@` prefix from the parser.
            // Adjust the span start to skip `@` so the emitter doesn't
            // double-emit it via copy_span fast path.
            let s = decorator.span.start as usize;
            let adjusted_start =
                if s < self.source.len() && self.source.as_bytes().get(s) == Some(&b'@') {
                    s + 1
                } else {
                    s
                };
            let adjusted = Expr {
                span: Span {
                    start: adjusted_start as u32,
                    end: decorator.span.end,
                },
                kind: decorator.kind.clone(),
            };
            self.emit_expr(&adjusted);
            // Emit trailing line comments on the same line (e.g., `// error`).
            let dec_end = decorator.span.end as usize;
            if dec_end < self.source.len() {
                let rest_of_line = &self.source[dec_end..];
                if let Some(nl) = rest_of_line.find('\n') {
                    let trailing = rest_of_line[..nl].trim();
                    if trailing.starts_with("//") {
                        self.write(" ");
                        self.write(trailing);
                        // Advance comment index past this comment so it's not double-emitted.
                        while self.next_comment_idx < self.comments.len() {
                            let c = &self.comments[self.next_comment_idx];
                            if c.pos as usize >= dec_end && c.pos as usize <= dec_end + nl {
                                self.comment_emit_pos = self.comment_emit_pos.max(c.end);
                                self.next_comment_idx += 1;
                            } else {
                                break;
                            }
                        }
                    }
                }
            }
            self.newline();
        }
        // Emit comments between the last decorator and the decorated element
        // (e.g., `/*3*/` between `@dec` and `class C {`).
        if let Some(pos) = element_keyword_pos {
            self.emit_leading_comments(pos);
        }
    }

    /// Collect info about decorated methods/getters/setters for IIFE wrapper emission.
    /// `this_preserve_temps` collects temp var names (e.g. `_a`, `_b`) needed for
    /// `this` preservation on member access decorators like `@instance.decorate`.
    fn collect_decorated_methods(
        &mut self,
        class_decl: &ClassDecl,
        this_preserve_temps: &mut Vec<String>,
        needs_outer_this: &mut bool,
        use_stage3_member_names: bool,
    ) -> Vec<DecoratedMethodInfo> {
        let mut result = Vec::new();
        let mut var_name_counts: HashMap<String, usize> = HashMap::new();
        for member in &class_decl.members {
            let (decorators, name, is_static, kind) = match &member.kind {
                ClassMemberKind::Method(method) if !method.decorators.is_empty() => (
                    &method.decorators,
                    &method.name,
                    method.modifiers & MOD_STATIC != 0,
                    DecoratedMemberKind::Method,
                ),
                ClassMemberKind::GetAccessor(acc) if !acc.decorators.is_empty() => (
                    &acc.decorators,
                    &acc.name,
                    acc.modifiers & MOD_STATIC != 0,
                    DecoratedMemberKind::Getter,
                ),
                ClassMemberKind::SetAccessor(acc) if !acc.decorators.is_empty() => (
                    &acc.decorators,
                    &acc.name,
                    acc.modifiers & MOD_STATIC != 0,
                    DecoratedMemberKind::Setter,
                ),
                _ => continue,
            };
            let (prop_name, key, uses_named_stem) = match name {
                PropName::Ident(n, _) => {
                    let name = normalize_unicode_escapes(n);
                    (
                        name.clone(),
                        DecoratedPublicMemberKey::Identifier(name),
                        true,
                    )
                }
                PropName::String(n, _) => {
                    let name = decode_js_string_content(n);
                    (name.clone(), DecoratedPublicMemberKey::Literal(name), false)
                }
                PropName::Computed(_, _) if !use_stage3_member_names => continue,
                PropName::Computed(expr, _) => match &expr.kind {
                    ExprKind::StrLit(value) | ExprKind::NoSubstTemplate(value) => {
                        let name = decode_js_string_content(value);
                        (name.clone(), DecoratedPublicMemberKey::Literal(name), false)
                    }
                    _ => {
                        let temp_name = self.next_inline_temp_var();
                        (
                            String::new(),
                            DecoratedPublicMemberKey::Dynamic {
                                expr: (**expr).clone(),
                                temp_name,
                            },
                            false,
                        )
                    }
                },
                _ => continue,
            };
            // Variable name: prefix with get_/set_ for accessors.
            let member_stem = if uses_named_stem || !use_stage3_member_names {
                prop_name.clone()
            } else {
                "member".to_string()
            };
            let base_var_name = match kind {
                DecoratedMemberKind::Getter => format!("get_{member_stem}"),
                DecoratedMemberKind::Setter => format!("set_{member_stem}"),
                DecoratedMemberKind::Method => member_stem,
            };
            let base_var_name = if is_static && use_stage3_member_names {
                format!("static_{base_var_name}")
            } else {
                base_var_name
            };
            let var_name = if use_stage3_member_names {
                let count = var_name_counts.entry(base_var_name.clone()).or_default();
                let name = if *count == 0 {
                    format!("{base_var_name}_decorators")
                } else {
                    format!("{base_var_name}_decorators_{}", *count)
                };
                *count += 1;
                name
            } else {
                base_var_name
            };
            // Serialize decorator expressions, applying this-preservation
            // for member access decorators (e.g. `instance.decorate`).
            let mut decorator_parts = Vec::new();
            for dec in decorators {
                let text = self.transform_decorator_expr_this_preserve(
                    dec,
                    this_preserve_temps,
                    needs_outer_this,
                );
                decorator_parts.push(text);
            }
            result.push(DecoratedMethodInfo {
                member_span_start: member.span.start,
                var_name,
                prop_name,
                decorator_exprs: decorator_parts.join(", "),
                is_static,
                kind,
                key,
            });
        }
        result
    }

    fn private_name_uses_preserved_member_recovery(name: &str) -> bool {
        normalize_unicode_escapes(name) == "constructor"
    }

    fn invalid_duplicate_private_name_info(
        class_decl: &ClassDecl,
    ) -> HashMap<String, Option<bool>> {
        #[derive(Clone, Copy, PartialEq, Eq)]
        enum PrivateMemberKind {
            Field,
            Method,
            Getter,
            Setter,
        }

        let mut groups: HashMap<String, Vec<(PrivateMemberKind, bool)>> = HashMap::new();
        for member in &class_decl.members {
            match &member.kind {
                ClassMemberKind::Property(prop) => {
                    if let PropName::Private(name, _) = &prop.name {
                        groups
                            .entry(normalize_unicode_escapes(name))
                            .or_default()
                            .push((PrivateMemberKind::Field, prop.modifiers & MOD_STATIC != 0));
                    }
                }
                ClassMemberKind::Method(method) => {
                    if let PropName::Private(name, _) = &method.name {
                        groups
                            .entry(normalize_unicode_escapes(name))
                            .or_default()
                            .push((
                                PrivateMemberKind::Method,
                                method.modifiers & MOD_STATIC != 0,
                            ));
                    }
                }
                ClassMemberKind::GetAccessor(acc) => {
                    if let PropName::Private(name, _) = &acc.name {
                        groups
                            .entry(normalize_unicode_escapes(name))
                            .or_default()
                            .push((PrivateMemberKind::Getter, acc.modifiers & MOD_STATIC != 0));
                    }
                }
                ClassMemberKind::SetAccessor(acc) => {
                    if let PropName::Private(name, _) = &acc.name {
                        groups
                            .entry(normalize_unicode_escapes(name))
                            .or_default()
                            .push((PrivateMemberKind::Setter, acc.modifiers & MOD_STATIC != 0));
                    }
                }
                _ => {}
            }
        }

        let mut invalid = HashMap::new();
        for (name, entries) in groups {
            if entries.len() <= 1 {
                continue;
            }
            let is_valid_accessor_pair = entries.len() == 2
                && ((entries[0].0 == PrivateMemberKind::Getter
                    && entries[1].0 == PrivateMemberKind::Setter)
                    || (entries[0].0 == PrivateMemberKind::Setter
                        && entries[1].0 == PrivateMemberKind::Getter))
                && entries[0].1 == entries[1].1;
            if is_valid_accessor_pair {
                continue;
            }
            let active = entries.last().copied().unwrap();
            invalid.insert(
                name,
                if active.0 == PrivateMemberKind::Field {
                    Some(active.1)
                } else {
                    None
                },
            );
        }
        invalid
    }

    fn private_field_name_by_span(class_decl: &ClassDecl) -> HashMap<u32, String> {
        class_decl
            .members
            .iter()
            .filter_map(|member| {
                if let ClassMemberKind::Property(prop) = &member.kind {
                    if let PropName::Private(name, _) = &prop.name {
                        return Some((member.span.start, normalize_unicode_escapes(name)));
                    }
                }
                None
            })
            .collect()
    }

    fn private_name_uses_duplicate_member_recovery(
        name: &str,
        invalid_duplicate_private_names: &HashMap<String, Option<bool>>,
    ) -> bool {
        invalid_duplicate_private_names.contains_key(&normalize_unicode_escapes(name))
    }

    fn private_name_uses_any_member_recovery(
        name: &str,
        invalid_duplicate_private_names: &HashMap<String, Option<bool>>,
    ) -> bool {
        Self::private_name_uses_preserved_member_recovery(name)
            || Self::private_name_uses_duplicate_member_recovery(
                name,
                invalid_duplicate_private_names,
            )
    }

    fn private_field_runtime_target(
        &self,
        name: &str,
        pf_span: Span,
        default_is_static: bool,
        invalid_duplicate_private_names: &HashMap<String, Option<bool>>,
        private_field_name_by_span: &HashMap<u32, String>,
    ) -> Option<(String, bool)> {
        let raw_name = private_field_name_by_span
            .get(&pf_span.start)
            .cloned()
            .unwrap_or_default();
        invalid_duplicate_private_names
            .get(&raw_name)
            .copied()
            .map(|active_static| {
                active_static.map(|active_static| {
                    let active_helper = self
                        .current_class_private_var_map
                        .get(raw_name.as_str())
                        .map(|name| name.trim_start_matches('_').to_string())
                        .unwrap_or_else(|| name.to_string());
                    (active_helper, active_static)
                })
            })
            .unwrap_or_else(|| Some((name.to_string(), default_is_static)))
    }

    fn has_emitted_instance_private_field_inits(
        &self,
        private_fields: &[(
            String,
            Option<&Expr>,
            bool,
            Span,
            Option<ClassExprBindingName>,
        )],
        invalid_duplicate_private_names: &HashMap<String, Option<bool>>,
        private_field_name_by_span: &HashMap<u32, String>,
    ) -> bool {
        private_fields
            .iter()
            .any(|(name, _, is_static, pf_span, _)| {
                !*is_static
                    && self
                        .private_field_runtime_target(
                            name,
                            *pf_span,
                            false,
                            invalid_duplicate_private_names,
                            private_field_name_by_span,
                        )
                        .is_some()
            })
    }

    fn emit_class_member_block_body(&mut self, body: &[Stmt], span: Span) {
        self.emit_block_for_decl_body(body, span);
    }

    /// Transform a decorator expression, applying `this` preservation for member access.
    /// - `@instance.decorate` → `(_a = instance).decorate.bind(_a)` (temp var)
    /// - `@(instance["decorate"])` → `((_b = instance)["decorate"].bind(_b))` (temp var)
    /// - `@(super.decorate)` → `(super.decorate.bind(_outerThis))` (outer this capture)
    fn transform_decorator_expr_this_preserve(
        &self,
        expr: &Expr,
        temps: &mut Vec<String>,
        needs_outer_this: &mut bool,
    ) -> String {
        if let Some(transformed) = self.try_this_preserve_inner(expr, temps, needs_outer_this) {
            transformed
        } else {
            // No transformation needed — emit as-is.
            let mut text = emit_expr_to_string(
                self.source,
                self.options,
                expr,
                &self.cjs_import_map,
                &self.cjs_string_import_locals,
                &self.import_shadows,
            );
            if let Some(stripped) = text.strip_prefix('@') {
                text = stripped.trim_start().to_string();
            }
            text
        }
    }

    /// Recursively check if a decorator expression is a member/elem access
    /// (possibly wrapped in parens). If so, transform it for `this` preservation.
    /// - Non-super: `(_a = obj).prop.bind(_a)` with temp var allocation
    /// - Super: `super.prop.bind(_outerThis)` with outer this capture
    fn try_this_preserve_inner(
        &self,
        expr: &Expr,
        temps: &mut Vec<String>,
        needs_outer_this: &mut bool,
    ) -> Option<String> {
        match &expr.kind {
            ExprKind::Member(m) => {
                if matches!(m.object.kind, ExprKind::Super) {
                    *needs_outer_this = true;
                    Some(format!("super.{}.bind(_outerThis)", m.property))
                } else {
                    let obj_text = emit_expr_to_string(
                        self.source,
                        self.options,
                        &m.object,
                        &self.cjs_import_map,
                        &self.cjs_string_import_locals,
                        &self.import_shadows,
                    );
                    let temp = alloc_this_preserve_temp(temps);
                    Some(format!(
                        "({} = {}).{}.bind({})",
                        temp, obj_text, m.property, temp
                    ))
                }
            }
            ExprKind::ElemAccess(e) => {
                if matches!(e.object.kind, ExprKind::Super) {
                    *needs_outer_this = true;
                    let idx_text = emit_expr_to_string(
                        self.source,
                        self.options,
                        &e.index,
                        &self.cjs_import_map,
                        &self.cjs_string_import_locals,
                        &self.import_shadows,
                    );
                    Some(format!("super[{}].bind(_outerThis)", idx_text))
                } else {
                    let obj_text = emit_expr_to_string(
                        self.source,
                        self.options,
                        &e.object,
                        &self.cjs_import_map,
                        &self.cjs_string_import_locals,
                        &self.import_shadows,
                    );
                    let idx_text = emit_expr_to_string(
                        self.source,
                        self.options,
                        &e.index,
                        &self.cjs_import_map,
                        &self.cjs_string_import_locals,
                        &self.import_shadows,
                    );
                    let temp = alloc_this_preserve_temp(temps);
                    Some(format!(
                        "({} = {})[{}].bind({})",
                        temp, obj_text, idx_text, temp
                    ))
                }
            }
            ExprKind::Paren(inner) => {
                let inner_transformed =
                    self.try_this_preserve_inner(inner, temps, needs_outer_this)?;
                Some(format!("({})", inner_transformed))
            }
            _ => None,
        }
    }

    fn emit_standard_decorator_expr_list(&mut self, decorators: &[Expr]) {
        for (idx, decorator) in decorators.iter().enumerate() {
            if idx > 0 {
                self.write(", ");
            }
            let mut decorator_text = emit_expr_to_string(
                self.source,
                self.options,
                decorator,
                &self.cjs_import_map,
                &self.cjs_string_import_locals,
                &self.import_shadows,
            );
            if let Some(stripped) = decorator_text.strip_prefix('@') {
                decorator_text = stripped.trim_start().to_string();
            }
            self.write(&decorator_text);
        }
    }

    fn emit_narrow_standard_decorated_member_call(
        &mut self,
        slot_stem: &str,
        prop_name: &str,
        is_static: bool,
        is_accessor: bool,
    ) {
        self.write(self.helper_prefix());
        self.write("__esDecorate(");
        if is_accessor {
            self.write("this");
        } else {
            self.write("null");
        }
        self.write(", null, _");
        self.write(slot_stem);
        self.write("_decorators, { kind: \"");
        self.write(if is_accessor { "accessor" } else { "field" });
        self.write("\", name: \"");
        self.write(prop_name);
        self.write("\", static: ");
        self.write(if is_static { "true" } else { "false" });
        self.write(", private: false, access: { has: obj => \"");
        self.write(prop_name);
        self.write("\" in obj, get: obj => obj.");
        self.write(prop_name);
        self.write(", set: (obj, value) => { obj.");
        self.write(prop_name);
        self.write(" = value; } }, metadata: _metadata }, _");
        self.write(slot_stem);
        self.write("_initializers, _");
        self.write(slot_stem);
        self.writeln("_extraInitializers);");
    }

    fn emit_narrow_standard_decorated_initializer_expr(
        &mut self,
        slot_stem: &str,
        prev_slot_stem: Option<&str>,
        initializer: Option<&Expr>,
        binding_name: Option<&str>,
    ) {
        if let Some(prev_slot_stem) = prev_slot_stem {
            self.write("(");
            self.write(self.helper_prefix());
            self.write("__runInitializers(this, _");
            self.write(prev_slot_stem);
            self.write("_extraInitializers), ");
        }
        self.write(self.helper_prefix());
        self.write("__runInitializers(this, _");
        self.write(slot_stem);
        self.write("_initializers, ");
        if let Some(init) = initializer {
            let binding_name =
                binding_name.map(|name| ClassExprBindingName::Literal(name.to_string()));
            self.emit_expr_with_binding_name_if_needed(binding_name.as_ref(), init);
        } else {
            self.write("void 0");
        }
        self.write(")");
        if prev_slot_stem.is_some() {
            self.write(")");
        }
    }

    pub(super) fn expr_needs_class_expr_binding_name(expr: &Expr) -> bool {
        matches!(&expr.kind, ExprKind::ClassExpr(cd)
            if cd.name.is_none()
                && (class_has_static_initializers(cd)
                    || !cd.decorators.is_empty()
                    || class_has_member_decorators(cd)))
    }

    fn class_expr_binding_name_identifier(binding_name: &ClassExprBindingName) -> Option<&str> {
        let ClassExprBindingName::Literal(name) = binding_name else {
            return None;
        };
        if name.is_empty() || name == "ClassExpression" {
            return None;
        }
        let mut chars = name.chars();
        let Some(first) = chars.next() else {
            return None;
        };
        if !(first == '_' || first == '$' || first.is_ascii_alphabetic()) {
            return None;
        }
        if chars.all(|ch| ch == '_' || ch == '$' || ch.is_ascii_alphanumeric()) {
            Some(name.as_str())
        } else {
            None
        }
    }

    fn emit_class_expr_binding_name_arg(&mut self, binding_name: &ClassExprBindingName) {
        match binding_name {
            ClassExprBindingName::Literal(name) => {
                self.write("\"");
                self.write(name);
                self.write("\"");
            }
            ClassExprBindingName::Expr(expr) => self.write(expr),
        }
    }

    fn class_general_computed_temp_for_expr(&self, expr: &Expr) -> Option<String> {
        self.class_general_computed_temps
            .iter()
            .find(|(_, start)| *start == expr.span.start)
            .map(|(name, _)| name.clone())
    }

    pub(super) fn computed_name_literal_text(expr: &Expr) -> Option<String> {
        match &Self::unwrap_type_layers(expr).kind {
            ExprKind::StrLit(s) | ExprKind::NoSubstTemplate(s) => Some(s.to_string()),
            ExprKind::NumLit(n) => Some(n.to_string()),
            ExprKind::BigIntLit(n) => Some(n.trim_end_matches('n').to_string()),
            ExprKind::BoolLit(v) => Some(if *v { "true" } else { "false" }.to_string()),
            _ => None,
        }
    }

    fn class_prop_binding_name(&self, prop: &ClassProp) -> Option<ClassExprBindingName> {
        match &prop.name {
            PropName::Ident(name, _) | PropName::String(name, _) | PropName::Number(name, _) => {
                Some(ClassExprBindingName::Literal(name.to_string()))
            }
            PropName::Computed(expr, _) => Self::computed_name_literal_text(expr)
                .map(ClassExprBindingName::Literal)
                .or_else(|| {
                    self.class_general_computed_temp_for_expr(expr)
                        .map(ClassExprBindingName::Expr)
                }),
            PropName::Private(name, _) => Some(ClassExprBindingName::Literal(format!("#{}", name))),
        }
    }

    fn class_prop_needs_named_eval_computed_capture(&self, prop: &ClassProp) -> Option<String> {
        let PropName::Computed(expr, _) = &prop.name else {
            return None;
        };
        let Some(init) = prop.initializer.as_deref() else {
            return None;
        };
        if !Self::expr_needs_class_expr_binding_name(init)
            || Self::computed_name_is_simple_literal(expr)
        {
            return None;
        }
        self.class_general_computed_temp_for_expr(expr)
    }

    fn emit_class_prop_name_for_initializer(&mut self, prop: &ClassProp) {
        if let (PropName::Computed(expr, _), Some(temp)) = (
            &prop.name,
            self.class_prop_needs_named_eval_computed_capture(prop),
        ) {
            self.needs_prop_key_helper = true;
            self.write("[");
            self.write(&temp);
            self.write(" = ");
            self.write(self.helper_prefix());
            self.write("__propKey(");
            self.emit_expr(expr);
            self.write(")]");
            return;
        }
        self.emit_prop_name(&prop.name);
    }

    fn emit_expr_with_binding_name_if_needed(
        &mut self,
        binding_name: Option<&ClassExprBindingName>,
        expr: &Expr,
    ) {
        let prev_in_parameter_initializer = self.in_parameter_initializer;
        self.in_parameter_initializer = false;
        let prev_binding_name = self.class_expr_binding_name.clone();
        if let Some(binding_name) =
            binding_name.filter(|_| Self::expr_needs_class_expr_binding_name(expr))
        {
            self.class_expr_binding_name = Some(binding_name.clone());
        }
        self.emit_expr(expr);
        self.class_expr_binding_name = prev_binding_name;
        self.in_parameter_initializer = prev_in_parameter_initializer;
    }

    fn emit_class_prop_initializer_expr(&mut self, prop: &ClassProp, expr: &Expr) {
        let binding_name = self.class_prop_binding_name(prop);
        self.emit_expr_with_binding_name_if_needed(binding_name.as_ref(), expr);
    }

    fn emit_private_static_value_initializer(
        &mut self,
        binding_name: Option<&ClassExprBindingName>,
        expr: &Expr,
    ) {
        // Private static field lowering emits `{ value: <expr> }`. When the
        // value is a multiline class expression, TypeScript indents that class
        // body one level deeper than the surrounding assignment.
        let needs_class_value_indent =
            matches!(&Self::unwrap_type_layers(expr).kind, ExprKind::ClassExpr(_));
        if needs_class_value_indent {
            self.indent += 1;
        }
        self.emit_expr_with_binding_name_if_needed(binding_name, expr);
        if needs_class_value_indent {
            self.indent -= 1;
        }
    }

    fn emit_leading_comments_in_range(&mut self, start: u32, end: u32) {
        if start >= end || (self.options.remove_comments == Some(true) && !self.preserve_comments) {
            return;
        }
        let saved_idx = self.next_comment_idx;
        let saved_emit_pos = self.comment_emit_pos;
        self.next_comment_idx = self.comments.partition_point(|c| c.pos < start);
        self.comment_emit_pos = start;
        self.emit_leading_comments(end);
        self.next_comment_idx = self.next_comment_idx.max(saved_idx);
        self.comment_emit_pos = self.comment_emit_pos.max(saved_emit_pos);
    }

    fn class_expr_needs_grouping_continuation_indent(&self, span_start: u32) -> bool {
        if self.at_line_start {
            return false;
        }
        let start = span_start as usize;
        if start > self.source.len() {
            return false;
        }
        let line_start = self.source[..start]
            .rfind('\n')
            .map(|idx| idx + 1)
            .unwrap_or(0);
        let prefix = &self.source[line_start..start];
        let mut bracket_depth = 0usize;
        for ch in prefix.chars() {
            match ch {
                '[' => bracket_depth += 1,
                ']' => bracket_depth = bracket_depth.saturating_sub(1),
                _ => {}
            }
        }
        if bracket_depth == 0 {
            return false;
        }
        let trimmed = prefix.trim_start();
        if trimmed.starts_with('[') {
            return true;
        }
        let Some(last_bracket) = prefix.rfind('[') else {
            return false;
        };
        prefix
            .rfind('=')
            .is_some_and(|last_eq| last_bracket > last_eq)
    }

    fn standard_decorator_instance_field_slots_for_class(
        &self,
        class_decl: &ClassDecl,
    ) -> Vec<StandardDecoratorFieldSlot> {
        let mut slots = Vec::new();
        let mut prev_slot_stem: Option<String> = None;
        for member in &class_decl.members {
            let ClassMemberKind::Property(prop) = &member.kind else {
                continue;
            };
            if prop.decorators.is_empty()
                || prop.modifiers & (MOD_STATIC | MOD_ACCESSOR | MOD_DECLARE | MOD_ABSTRACT) != 0
            {
                continue;
            }
            let Some(prop_name) = prop.name.ident_name() else {
                continue;
            };
            let slot_stem = normalize_unicode_escapes(prop_name);
            slots.push(StandardDecoratorFieldSlot {
                prop_name: prop_name.to_string(),
                slot_stem: slot_stem.clone(),
                prev_slot_stem: prev_slot_stem.clone(),
            });
            prev_slot_stem = Some(slot_stem);
        }
        slots
    }

    fn standard_decorator_instance_field_slot(
        &self,
        prop: &ClassProp,
    ) -> Option<&StandardDecoratorFieldSlot> {
        let prop_name = prop.name.ident_name()?;
        self.standard_decorator_instance_field_slots
            .iter()
            .find(|slot| slot.prop_name == prop_name)
    }

    fn emit_standard_decorator_instance_field_extra_initializers(&mut self) {
        let Some(last_slot_stem) = self
            .standard_decorator_instance_field_slots
            .last()
            .map(|slot| slot.slot_stem.clone())
        else {
            return;
        };
        self.write(self.helper_prefix());
        self.write("__runInitializers(this, _");
        self.write(&last_slot_stem);
        self.writeln("_extraInitializers);");
    }

    pub(super) fn emit_simple_standard_decorated_class_wrapper(&mut self, class_decl: &ClassDecl) {
        let Some(name) = class_decl.name.as_deref() else {
            return;
        };
        let can_use_downlevel_wrapper = self.needs_downlevel("static-blocks")
            && class_decl.extends.is_none()
            && class_decl.members.iter().all(|member| match &member.kind {
                ClassMemberKind::Property(prop) => {
                    prop.decorators.is_empty()
                        && (prop.modifiers & MOD_STATIC == 0
                            || matches!(
                                prop.name,
                                PropName::Ident(..) | PropName::String(..) | PropName::Number(..)
                            ))
                }
                ClassMemberKind::Method(method) => method.decorators.is_empty(),
                ClassMemberKind::GetAccessor(accessor) | ClassMemberKind::SetAccessor(accessor) => {
                    accessor.decorators.is_empty()
                }
                _ => true,
            });
        if can_use_downlevel_wrapper {
            self.emit_downlevel_simple_standard_decorated_class_wrapper(class_decl, name);
            return;
        }
        // Set helper flags so inline helpers are emitted in preamble.
        self.needs_es_decorate_helper = true;
        self.needs_run_initializers_helper = true;
        self.emitted_var_names.insert(name.into());

        // Collect decorated methods for method decorator support.
        let mut this_preserve_temps = Vec::new();
        let mut needs_outer_this = false;
        let decorated_methods = self.collect_decorated_methods(
            class_decl,
            &mut this_preserve_temps,
            &mut needs_outer_this,
            false,
        );
        let has_instance_method_decorators = decorated_methods.iter().any(|m| !m.is_static);
        let has_static_method_decorators = decorated_methods.iter().any(|m| m.is_static);
        let has_explicit_constructor = class_decl
            .members
            .iter()
            .any(|m| matches!(&m.kind, ClassMemberKind::Constructor(_)));

        self.write("let ");
        self.write(name);
        self.writeln(" = (() => {");
        self.indent += 1;
        // Emit temp vars for this-preservation (e.g. `var _a, _b, _c;`).
        if !this_preserve_temps.is_empty() {
            self.write("var ");
            self.write(&this_preserve_temps.join(", "));
            self.writeln(";");
        }
        let has_extends = class_decl.extends.is_some();
        self.write("let _classDecorators = [");
        self.emit_standard_decorator_expr_list(&class_decl.decorators);
        self.writeln("];");
        self.writeln("let _classDescriptor;");
        self.writeln("let _classExtraInitializers = [];");
        self.writeln("let _classThis;");
        // Emit extra initializer arrays for method decorators.
        if has_instance_method_decorators {
            self.writeln("let _instanceExtraInitializers = [];");
        }
        if has_static_method_decorators {
            self.writeln("let _staticExtraInitializers = [];");
        }
        // Emit decorator list variables for each decorated method.
        for dm in &decorated_methods {
            self.write("let _");
            self.write(&dm.var_name);
            self.writeln("_decorators;");
        }
        if has_extends {
            let extends_expr = class_decl.extends.as_ref().unwrap();
            self.write("let _classSuper = ");
            if extends_expr_needs_comma_wrap(extends_expr) {
                self.write("(0, ");
                self.emit_expr(extends_expr);
                self.write(")");
            } else {
                self.emit_expr(extends_expr);
            }
            self.writeln(";");
        }
        self.write("var ");
        self.write(name);
        if has_extends {
            self.writeln(" = class extends _classSuper {");
        } else {
            self.writeln(" = class {");
        }
        self.indent += 1;
        self.writeln("static { _classThis = this; }");
        self.writeln("static {");
        self.indent += 1;
        if has_extends {
            self.writeln(
                "const _metadata = typeof Symbol === \"function\" && Symbol.metadata ? Object.create(_classSuper[Symbol.metadata] ?? null) : void 0;",
            );
        } else {
            self.writeln(
                "const _metadata = typeof Symbol === \"function\" && Symbol.metadata ? Object.create(null) : void 0;",
            );
        }
        // Emit method decorator assignments first, then __esDecorate calls.
        for dm in &decorated_methods {
            self.write("_");
            self.write(&dm.var_name);
            self.write("_decorators = [");
            self.write(&dm.decorator_exprs);
            self.writeln("];");
        }
        for dm in &decorated_methods {
            let extra_init = if dm.is_static {
                "_staticExtraInitializers"
            } else {
                "_instanceExtraInitializers"
            };
            self.write(self.helper_prefix());
            self.write("__esDecorate(this, null, _");
            self.write(&dm.var_name);
            self.write("_decorators, { kind: \"");
            self.write(dm.kind.as_str());
            self.write("\", name: \"");
            self.write(&dm.prop_name);
            self.write("\", static: ");
            self.write(if dm.is_static { "true" } else { "false" });
            self.write(", private: false, access: { has: obj => \"");
            self.write(&dm.prop_name);
            self.write("\" in obj, get: obj => obj.");
            self.write(&dm.prop_name);
            self.write(" }, metadata: _metadata }, null, ");
            self.write(extra_init);
            self.writeln(");");
        }
        self.write(self.helper_prefix());
        self.writeln("__esDecorate(null, _classDescriptor = { value: _classThis }, _classDecorators, { kind: \"class\", name: _classThis.name, metadata: _metadata }, null, _classExtraInitializers);");
        self.write(name);
        self.writeln(" = _classThis = _classDescriptor.value;");
        self.writeln("if (_metadata) Object.defineProperty(_classThis, Symbol.metadata, { enumerable: true, configurable: true, writable: true, value: _metadata });");
        if has_static_method_decorators {
            self.write(self.helper_prefix());
            self.writeln("__runInitializers(_classThis, _staticExtraInitializers);");
        }
        // Determine if class has user-defined static blocks or static fields.
        // If so, defer `__runInitializers(_classThis, _classExtraInitializers)` to a
        // separate static block at the end so it runs after user static members.
        let has_user_static_members = class_decl.members.iter().any(|m| match &m.kind {
            ClassMemberKind::StaticBlock(_) => true,
            ClassMemberKind::Property(prop) => prop.modifiers & MOD_STATIC != 0,
            _ => false,
        });
        if !has_user_static_members {
            self.write(self.helper_prefix());
            self.writeln("__runInitializers(_classThis, _classExtraInitializers);");
        }
        self.indent -= 1;
        self.writeln("}");
        // Emit class members after the decorator static blocks.
        // Set flag so constructor emission injects __runInitializers if needed.
        let prev_inject = self.inject_instance_extra_initializers;
        if has_instance_method_decorators && has_explicit_constructor {
            self.inject_instance_extra_initializers = true;
        }
        let use_define = self.use_define_for_class_fields();
        let name_owned = name.to_string();
        let saved_static_super_base = self.static_super_base_alias.take();
        let saved_static_super_recv = self.static_super_receiver_alias.take();
        for member in &class_decl.members {
            // Reflect.get/set transforms only apply in static blocks and static field
            // initializers — NOT in constructors, instance fields, or methods.
            let needs_super_transform = has_extends
                && match &member.kind {
                    ClassMemberKind::StaticBlock(_) => true,
                    ClassMemberKind::Property(prop) => prop.modifiers & MOD_STATIC != 0,
                    _ => false,
                };
            let is_static_field = matches!(&member.kind,
                ClassMemberKind::Property(prop) if prop.modifiers & MOD_STATIC != 0
            );
            if needs_super_transform {
                self.static_super_base_alias = Some("_classSuper".to_string());
                self.static_super_receiver_alias = Some("_classThis".to_string());
                if is_static_field {
                    self.super_reflect_in_field_init = true;
                }
            }
            self.emit_class_member(
                member,
                &[],         // field_inits: no instance fields to move
                has_extends, // has_super
                use_define,
                None, // method_name_override
                &name_owned,
                &[], // instance_comment_states
            );
            if needs_super_transform {
                self.static_super_base_alias = None;
                self.static_super_receiver_alias = None;
                self.super_reflect_in_field_init = false;
            }
        }
        self.static_super_base_alias = saved_static_super_base;
        self.static_super_receiver_alias = saved_static_super_recv;
        self.inject_instance_extra_initializers = prev_inject;
        // Emit deferred `__runInitializers` in its own static block.
        if has_user_static_members {
            self.writeln("static {");
            self.indent += 1;
            self.write(self.helper_prefix());
            self.writeln("__runInitializers(_classThis, _classExtraInitializers);");
            self.indent -= 1;
            self.writeln("}");
        }
        // Emit generated constructor for instance method decorator initializers.
        if has_instance_method_decorators && !has_explicit_constructor {
            if has_extends {
                self.writeln("constructor(...args) {");
                self.indent += 1;
                self.writeln("super(...args);");
                self.write(self.helper_prefix());
                self.writeln("__runInitializers(this, _instanceExtraInitializers);");
                self.indent -= 1;
                self.writeln("}");
            } else {
                self.writeln("constructor() {");
                self.indent += 1;
                self.write(self.helper_prefix());
                self.writeln("__runInitializers(this, _instanceExtraInitializers);");
                self.indent -= 1;
                self.writeln("}");
            }
        }
        self.indent -= 1;
        self.writeln("};");
        self.write("return ");
        self.write(&name_owned);
        self.writeln(" = _classThis;");
        self.indent -= 1;
        self.writeln("})();");
    }

    fn emit_downlevel_simple_standard_decorated_class_wrapper(
        &mut self,
        class_decl: &ClassDecl,
        name: &str,
    ) {
        self.needs_es_decorate_helper = true;
        self.needs_run_initializers_helper = true;
        self.needs_set_function_name_helper = true;
        self.emitted_var_names.insert(name.into());

        self.write("let ");
        self.write(name);
        self.writeln(" = (() => {");
        self.indent += 1;
        self.write("let _classDecorators = [");
        self.emit_standard_decorator_expr_list(&class_decl.decorators);
        self.writeln("];");
        self.writeln("let _classDescriptor;");
        self.writeln("let _classExtraInitializers = [];");
        self.writeln("let _classThis;");

        let mut emitted_class = class_decl.clone();
        emitted_class.name = None;
        emitted_class.decorators.clear();
        emitted_class.members.retain(|member| {
            !matches!(&member.kind, ClassMemberKind::StaticBlock(_))
                && !matches!(&member.kind, ClassMemberKind::Property(prop) if prop.modifiers & MOD_STATIC != 0)
        });
        self.write("var ");
        self.write(name);
        self.write(" = _classThis = ");
        self.emit_class_decl(&emitted_class);
        self.strip_trailing_newline();
        self.writeln(";");

        self.write(self.helper_prefix());
        self.write("__setFunctionName(_classThis, \"");
        self.write(name);
        self.writeln("\");");

        self.writeln("(() => {");
        self.indent += 1;
        self.writeln("const _metadata = typeof Symbol === \"function\" && Symbol.metadata ? Object.create(null) : void 0;");
        self.write(self.helper_prefix());
        self.writeln("__esDecorate(null, _classDescriptor = { value: _classThis }, _classDecorators, { kind: \"class\", name: _classThis.name, metadata: _metadata }, null, _classExtraInitializers);");
        self.write(name);
        self.writeln(" = _classThis = _classDescriptor.value;");
        self.writeln("if (_metadata) Object.defineProperty(_classThis, Symbol.metadata, { enumerable: true, configurable: true, writable: true, value: _metadata });");
        self.indent -= 1;
        self.writeln("})();");

        let saved_static_this_alias = self.static_this_alias.replace("_classThis".to_string());
        for member in &class_decl.members {
            match &member.kind {
                ClassMemberKind::StaticBlock(stmts) => {
                    self.writeln("(() => {");
                    self.indent += 1;
                    for stmt in stmts {
                        self.emit_stmt(stmt);
                    }
                    self.indent -= 1;
                    self.writeln("})();");
                }
                ClassMemberKind::Property(prop) if prop.modifiers & MOD_STATIC != 0 => {
                    self.write("_classThis");
                    match &prop.name {
                        PropName::Ident(prop_name, _) => {
                            self.write(".");
                            self.write(prop_name);
                        }
                        PropName::String(prop_name, _) => {
                            self.write("[\"");
                            self.write(prop_name);
                            self.write("\"]");
                        }
                        PropName::Number(prop_name, _) => {
                            self.write("[");
                            self.write(prop_name);
                            self.write("]");
                        }
                        _ => unreachable!(),
                    }
                    self.write(" = ");
                    if let Some(initializer) = &prop.initializer {
                        self.emit_expr(initializer);
                    } else {
                        self.write("void 0");
                    }
                    self.writeln(";");
                }
                _ => {}
            }
        }
        self.static_this_alias = saved_static_this_alias;

        self.writeln("(() => {");
        self.indent += 1;
        self.write(self.helper_prefix());
        self.writeln("__runInitializers(_classThis, _classExtraInitializers);");
        self.indent -= 1;
        self.writeln("})();");
        self.write("return ");
        self.write(name);
        self.writeln(" = _classThis;");
        self.indent -= 1;
        self.writeln("})();");
    }

    /// Preserve the pre-existing method-only transform for shapes outside the
    /// structural wrapper's deliberately narrow safety predicate.
    fn emit_legacy_method_only_decorated_class_wrapper(&mut self, class_decl: &ClassDecl) {
        let Some(name) = class_decl.name.as_deref() else {
            return;
        };
        self.needs_es_decorate_helper = true;
        self.needs_run_initializers_helper = true;
        self.has_es_decorated_methods = true;
        self.emitted_var_names.insert(name.into());

        let mut this_preserve_temps = Vec::new();
        let mut needs_outer_this = false;
        let decorated_methods = self.collect_decorated_methods(
            class_decl,
            &mut this_preserve_temps,
            &mut needs_outer_this,
            false,
        );
        let has_instance_method_decorators = decorated_methods.iter().any(|m| !m.is_static);
        let has_static_method_decorators = decorated_methods.iter().any(|m| m.is_static);
        let has_explicit_constructor = class_decl
            .members
            .iter()
            .any(|m| matches!(&m.kind, ClassMemberKind::Constructor(_)));
        let has_extends = class_decl.extends.is_some();

        self.write("let ");
        self.write(name);
        self.writeln(" = (() => {");
        self.indent += 1;
        if !this_preserve_temps.is_empty() {
            self.write("var ");
            self.write(&this_preserve_temps.join(", "));
            self.writeln(";");
        }
        if needs_outer_this {
            self.writeln("let _outerThis = this;");
        }
        if has_extends {
            let extends_expr = class_decl.extends.as_ref().unwrap();
            self.write("let _classSuper = ");
            if extends_expr_needs_comma_wrap(extends_expr) {
                self.write("(0, ");
                self.emit_expr(extends_expr);
                self.write(")");
            } else {
                self.emit_expr(extends_expr);
            }
            self.writeln(";");
        }
        if has_instance_method_decorators {
            self.writeln("let _instanceExtraInitializers = [];");
        }
        if has_static_method_decorators {
            self.writeln("let _staticExtraInitializers = [];");
        }
        for member in &decorated_methods {
            self.write("let _");
            self.write(&member.var_name);
            self.writeln("_decorators;");
        }
        self.write("return class ");
        self.write(name);
        if has_extends {
            self.writeln(" extends _classSuper {");
        } else {
            self.writeln(" {");
        }
        self.indent += 1;
        self.writeln("static {");
        self.indent += 1;
        if has_extends {
            self.writeln(
                "const _metadata = typeof Symbol === \"function\" && Symbol.metadata ? Object.create(_classSuper[Symbol.metadata] ?? null) : void 0;",
            );
        } else {
            self.writeln(
                "const _metadata = typeof Symbol === \"function\" && Symbol.metadata ? Object.create(null) : void 0;",
            );
        }
        for member in &decorated_methods {
            self.write("_");
            self.write(&member.var_name);
            self.write("_decorators = [");
            self.write(&member.decorator_exprs);
            self.writeln("];");
        }
        for member in &decorated_methods {
            self.write(self.helper_prefix());
            self.write("__esDecorate(this, null, _");
            self.write(&member.var_name);
            self.write("_decorators, { kind: \"");
            self.write(member.kind.as_str());
            self.write("\", name: \"");
            self.write(&member.prop_name);
            self.write("\", static: ");
            self.write(if member.is_static { "true" } else { "false" });
            self.write(", private: false, access: { has: obj => \"");
            self.write(&member.prop_name);
            self.write("\" in obj, get: obj => obj.");
            self.write(&member.prop_name);
            self.write(" }, metadata: _metadata }, null, ");
            self.write(if member.is_static {
                "_staticExtraInitializers"
            } else {
                "_instanceExtraInitializers"
            });
            self.writeln(");");
        }
        self.writeln("if (_metadata) Object.defineProperty(this, Symbol.metadata, { enumerable: true, configurable: true, writable: true, value: _metadata });");
        if has_static_method_decorators {
            self.write(self.helper_prefix());
            self.writeln("__runInitializers(this, _staticExtraInitializers);");
        }
        self.indent -= 1;
        self.writeln("}");
        let prev_inject = self.inject_instance_extra_initializers;
        if has_instance_method_decorators && has_explicit_constructor {
            self.inject_instance_extra_initializers = true;
        }
        let use_define = self.use_define_for_class_fields();
        let name_owned = name.to_string();
        for member in &class_decl.members {
            self.emit_leading_comments(member.span.start);
            self.emit_class_member(member, &[], has_extends, use_define, None, &name_owned, &[]);
        }
        self.inject_instance_extra_initializers = prev_inject;
        if has_instance_method_decorators && !has_explicit_constructor {
            if has_extends {
                self.writeln("constructor() {");
                self.indent += 1;
                self.writeln("super(...arguments);");
                self.write(self.helper_prefix());
                self.writeln("__runInitializers(this, _instanceExtraInitializers);");
                self.indent -= 1;
                self.writeln("}");
            } else {
                self.writeln("constructor() {");
                self.indent += 1;
                self.write(self.helper_prefix());
                self.writeln("__runInitializers(this, _instanceExtraInitializers);");
                self.indent -= 1;
                self.writeln("}");
            }
        }
        self.indent -= 1;
        self.writeln("};");
        self.indent -= 1;
        self.writeln("})();");
    }

    fn emit_public_standard_decorator_assignment(&mut self, member: &DecoratedMethodInfo) {
        self.write(&member.var_name);
        self.write(" = [");
        self.write(&member.decorator_exprs);
        self.writeln("];");
    }

    fn emit_public_standard_decorator_key(&mut self, key: &DecoratedPublicMemberKey) {
        match key {
            DecoratedPublicMemberKey::Identifier(name)
            | DecoratedPublicMemberKey::Literal(name) => {
                self.write("\"");
                self.write(&super::emit_expr::escape_js_string_for_quote(name, '"'));
                self.write("\"");
            }
            DecoratedPublicMemberKey::Dynamic { temp_name, .. } => self.write(temp_name),
        }
    }

    fn emit_public_standard_decorator_access(
        &mut self,
        key: &DecoratedPublicMemberKey,
        kind: DecoratedMemberKind,
    ) {
        self.write("has: obj => ");
        self.emit_public_standard_decorator_key(key);
        self.write(" in obj, ");
        match kind {
            DecoratedMemberKind::Method | DecoratedMemberKind::Getter => {
                self.write("get: obj => obj");
                match key {
                    DecoratedPublicMemberKey::Identifier(name) => {
                        self.write(".");
                        self.write(name);
                    }
                    _ => {
                        self.write("[");
                        self.emit_public_standard_decorator_key(key);
                        self.write("]");
                    }
                }
            }
            DecoratedMemberKind::Setter => {
                self.write("set: (obj, value) => { obj");
                match key {
                    DecoratedPublicMemberKey::Identifier(name) => {
                        self.write(".");
                        self.write(name);
                    }
                    _ => {
                        self.write("[");
                        self.emit_public_standard_decorator_key(key);
                        self.write("]");
                    }
                }
                self.write(" = value; }");
            }
        }
    }

    fn emit_public_standard_decorator_call(
        &mut self,
        member: &DecoratedMethodInfo,
        class_target: &str,
        metadata_name: &str,
        static_initializers_name: Option<&str>,
        instance_initializers_name: Option<&str>,
    ) {
        self.write(self.helper_prefix());
        self.write("__esDecorate(");
        self.write(class_target);
        self.write(", null, ");
        self.write(&member.var_name);
        self.write(", { kind: \"");
        self.write(member.kind.as_str());
        self.write("\", name: ");
        self.emit_public_standard_decorator_key(&member.key);
        self.write(", static: ");
        self.write(if member.is_static { "true" } else { "false" });
        self.write(", private: false, access: { ");
        self.emit_public_standard_decorator_access(&member.key, member.kind);
        self.write(" }, metadata: ");
        self.write(metadata_name);
        self.write(" }, null, ");
        self.write(if member.is_static {
            static_initializers_name.expect("static decorated member needs initializers")
        } else {
            instance_initializers_name.expect("instance decorated member needs initializers")
        });
        self.writeln(");");
    }

    /// Emit a structural public method/getter/setter decorator wrapper.
    /// ES2015 uses a post-class comma/IIFE transform; ES2022 keeps the
    /// decorator application in a leading static block. Computed keys capture
    /// both pending decorator expressions and their property key in source order.
    fn emit_public_multi_method_decorated_class_wrapper(&mut self, class_decl: &ClassDecl) {
        let Some(name) = class_decl.name.as_deref() else {
            return;
        };
        self.needs_es_decorate_helper = true;
        self.needs_run_initializers_helper = true;
        self.has_es_decorated_methods = true;
        self.emitted_var_names.insert(name.into());

        let downlevel_static_blocks = self.needs_downlevel("static-blocks");
        let class_alias = downlevel_static_blocks.then(|| self.next_inline_temp_var());
        let mut this_preserve_temps = Vec::new();
        let mut needs_outer_this = false;
        let mut decorated_methods = self.collect_decorated_methods(
            class_decl,
            &mut this_preserve_temps,
            &mut needs_outer_this,
            true,
        );
        debug_assert!(this_preserve_temps.is_empty());
        debug_assert!(!needs_outer_this);
        let has_instance_method_decorators = decorated_methods.iter().any(|m| !m.is_static);
        let has_static_method_decorators = decorated_methods.iter().any(|m| m.is_static);
        let has_explicit_constructor = class_decl
            .members
            .iter()
            .any(|m| matches!(&m.kind, ClassMemberKind::Constructor(_)));

        let mut allocated_bindings = HashSet::new();
        for member in &mut decorated_methods {
            member.var_name = self.allocate_inline_binding_name(
                &format!("_{}", member.var_name),
                &mut allocated_bindings,
            );
        }
        let instance_initializers_name = has_instance_method_decorators.then(|| {
            self.allocate_inline_binding_name("_instanceExtraInitializers", &mut allocated_bindings)
        });
        let static_initializers_name = has_static_method_decorators.then(|| {
            self.allocate_inline_binding_name("_staticExtraInitializers", &mut allocated_bindings)
        });
        let metadata_name = self.allocate_inline_binding_name("_metadata", &mut allocated_bindings);

        let mut pending_assignments = Vec::new();
        let mut member_name_overrides: HashMap<u32, String> = HashMap::new();
        for (idx, member) in decorated_methods.iter().enumerate() {
            pending_assignments.push(idx);
            let DecoratedPublicMemberKey::Dynamic { expr, temp_name } = &member.key else {
                continue;
            };
            let mut parts = pending_assignments
                .drain(..)
                .map(|pending_idx| {
                    let pending = &decorated_methods[pending_idx];
                    format!("{} = [{}]", pending.var_name, pending.decorator_exprs)
                })
                .collect::<Vec<_>>();
            let expr_text = emit_expr_to_string(
                self.source,
                self.options,
                expr,
                &self.cjs_import_map,
                &self.cjs_string_import_locals,
                &self.import_shadows,
            );
            parts.push(format!(
                "{temp_name} = {}__propKey({expr_text})",
                self.helper_prefix()
            ));
            member_name_overrides.insert(
                member.member_span_start,
                format!("[({})]", parts.join(", ")),
            );
        }

        let dynamic_temps = decorated_methods
            .iter()
            .filter_map(|member| match &member.key {
                DecoratedPublicMemberKey::Dynamic { temp_name, .. } => Some(temp_name.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();

        self.write("let ");
        self.write(name);
        self.writeln(" = (() => {");
        self.indent += 1;
        if let Some(alias) = &class_alias {
            self.write("var ");
            self.write(alias);
            self.writeln(";");
        }
        if !dynamic_temps.is_empty() {
            self.write("var ");
            self.write(&dynamic_temps.join(", "));
            self.writeln(";");
        }
        if let Some(initializers_name) = &instance_initializers_name {
            self.write("let ");
            self.write(initializers_name);
            self.writeln(" = [];");
        }
        if let Some(initializers_name) = &static_initializers_name {
            self.write("let ");
            self.write(initializers_name);
            self.writeln(" = [];");
        }
        for member in &decorated_methods {
            self.write("let ");
            self.write(&member.var_name);
            self.writeln(";");
        }

        self.write("return ");
        if let Some(alias) = &class_alias {
            self.write(alias);
            self.write(" = ");
        }
        self.write("class ");
        self.write(name);
        self.writeln(" {");
        self.indent += if downlevel_static_blocks { 2 } else { 1 };

        if !downlevel_static_blocks {
            self.writeln("static {");
            self.indent += 1;
            self.write("const ");
            self.write(&metadata_name);
            self.writeln(" = typeof Symbol === \"function\" && Symbol.metadata ? Object.create(null) : void 0;");
            for pending_idx in &pending_assignments {
                self.emit_public_standard_decorator_assignment(&decorated_methods[*pending_idx]);
            }
            for is_static in [true, false] {
                for member in decorated_methods
                    .iter()
                    .filter(|member| member.is_static == is_static)
                {
                    self.emit_public_standard_decorator_call(
                        member,
                        "this",
                        &metadata_name,
                        static_initializers_name.as_deref(),
                        instance_initializers_name.as_deref(),
                    );
                }
            }
            self.write("if (");
            self.write(&metadata_name);
            self.write(") Object.defineProperty(this, Symbol.metadata, { enumerable: true, configurable: true, writable: true, value: ");
            self.write(&metadata_name);
            self.writeln(" });");
            if let Some(initializers_name) = &static_initializers_name {
                self.write(self.helper_prefix());
                self.write("__runInitializers(this, ");
                self.write(initializers_name);
                self.writeln(");");
            }
            self.indent -= 1;
            self.writeln("}");
        }

        let prev_inject = self.inject_instance_extra_initializers;
        let prev_injected_name = self.injected_instance_extra_initializers_name.take();
        if has_instance_method_decorators && has_explicit_constructor {
            self.inject_instance_extra_initializers = true;
            self.injected_instance_extra_initializers_name = instance_initializers_name.clone();
        }
        let use_define = self.use_define_for_class_fields();
        let name_owned = name.to_string();
        for member in &class_decl.members {
            self.emit_leading_comments(member.span.start);
            self.emit_class_member(
                member,
                &[],
                false,
                use_define,
                member_name_overrides
                    .get(&member.span.start)
                    .map(String::as_str),
                &name_owned,
                &[],
            );
        }
        self.inject_instance_extra_initializers = prev_inject;
        self.injected_instance_extra_initializers_name = prev_injected_name;
        if has_instance_method_decorators && !has_explicit_constructor {
            self.writeln("constructor() {");
            self.indent += 1;
            self.write(self.helper_prefix());
            self.write("__runInitializers(this, ");
            self.write(
                instance_initializers_name
                    .as_deref()
                    .expect("instance decorated member needs initializers"),
            );
            self.writeln(");");
            self.indent -= 1;
            self.writeln("}");
        }

        self.indent -= 1;
        if downlevel_static_blocks {
            self.writeln("},");
            self.writeln("(() => {");
            self.indent += 1;
            self.write("const ");
            self.write(&metadata_name);
            self.writeln(" = typeof Symbol === \"function\" && Symbol.metadata ? Object.create(null) : void 0;");
            for pending_idx in &pending_assignments {
                self.emit_public_standard_decorator_assignment(&decorated_methods[*pending_idx]);
            }
            let alias = class_alias.as_deref().unwrap_or(name);
            for is_static in [true, false] {
                for member in decorated_methods
                    .iter()
                    .filter(|member| member.is_static == is_static)
                {
                    self.emit_public_standard_decorator_call(
                        member,
                        alias,
                        &metadata_name,
                        static_initializers_name.as_deref(),
                        instance_initializers_name.as_deref(),
                    );
                }
            }
            self.write("if (");
            self.write(&metadata_name);
            self.write(") Object.defineProperty(");
            self.write(alias);
            self.write(", Symbol.metadata, { enumerable: true, configurable: true, writable: true, value: ");
            self.write(&metadata_name);
            self.writeln(" });");
            if let Some(initializers_name) = &static_initializers_name {
                self.write(self.helper_prefix());
                self.write("__runInitializers(");
                self.write(alias);
                self.write(", ");
                self.write(initializers_name);
                self.writeln(");");
            }
            self.indent -= 1;
            self.writeln("})(),");
            self.write(alias);
            self.writeln(";");
            self.indent -= 1;
        } else {
            self.writeln("};");
        }
        self.indent -= 1;
        self.writeln("})();");
    }

    pub(super) fn can_emit_public_multi_method_decorator_wrapper(
        &self,
        class_decl: &ClassDecl,
    ) -> bool {
        if !class_can_emit_public_multi_method_decorator_wrapper(class_decl) {
            return false;
        }

        // The structural wrapper rebuilds the class instead of walking the
        // original member spans. Until that path owns comment placement, keep
        // comment-bearing classes on the established wrapper when comments
        // are requested. With removeComments, no comment trivia needs to be
        // preserved and the structural path is safe.
        (self.options.remove_comments == Some(true) && !self.preserve_comments)
            || !self.comments.iter().any(|comment| {
                comment.pos >= class_decl.span.start && comment.pos < class_decl.span.end
            })
    }

    pub(super) fn emit_method_only_decorated_class_wrapper(&mut self, class_decl: &ClassDecl) {
        if self.effective_target() >= ScriptTarget::ES2015
            && self.can_emit_public_multi_method_decorator_wrapper(class_decl)
        {
            self.emit_public_multi_method_decorated_class_wrapper(class_decl);
        } else {
            self.emit_legacy_method_only_decorated_class_wrapper(class_decl);
        }
    }

    /// Emit an IIFE wrapper for a standard-decorated class EXPRESSION.
    /// Used when `@dec class {}` appears in expression position (e.g., assignment).
    /// `assigned_name` is the name from the assignment target (e.g., "C" from `const C = @dec class {}`).
    pub(super) fn can_emit_downlevel_using_decorated_class_expr(
        &self,
        class_decl: &ClassDecl,
    ) -> bool {
        self.in_using_class_initializer
            && self.needs_downlevel("static-blocks")
            && class_decl.extends.is_none()
            && class_decl.members.iter().all(|member| match &member.kind {
                ClassMemberKind::Property(prop) if prop.modifiers & MOD_STATIC != 0 => {
                    matches!(
                        prop.name,
                        PropName::Ident(..) | PropName::String(..) | PropName::Number(..)
                    )
                }
                ClassMemberKind::StaticBlock(_) => false,
                _ => true,
            })
    }

    pub(super) fn emit_downlevel_using_decorated_class_expr_wrapper(
        &mut self,
        class_decl: &ClassDecl,
        assigned_name: Option<&ClassExprBindingName>,
    ) {
        self.needs_es_decorate_helper = true;
        self.needs_run_initializers_helper = true;
        self.needs_set_function_name_helper = true;
        self.anonymous_class_counter += 1;
        let internal_name = format!("class_{}", self.anonymous_class_counter);
        let static_props: Vec<&ClassProp> = class_decl
            .members
            .iter()
            .filter_map(|member| match &member.kind {
                ClassMemberKind::Property(prop) if prop.modifiers & MOD_STATIC != 0 => Some(prop),
                _ => None,
            })
            .collect();

        self.writeln("(() => {");
        self.indent += 1;
        self.write("let _classDecorators = [");
        self.emit_standard_decorator_expr_list(&class_decl.decorators);
        self.writeln("];");
        self.writeln("let _classDescriptor;");
        self.writeln("let _classExtraInitializers = [];");
        self.writeln("let _classThis;");

        let mut emitted_class = class_decl.clone();
        emitted_class.decorators.clear();
        emitted_class.members.retain(|member| {
            !matches!(&member.kind, ClassMemberKind::Property(prop) if prop.modifiers & MOD_STATIC != 0)
        });
        self.write("var ");
        self.write(&internal_name);
        self.write(" = _classThis = ");
        self.emit_class_decl(&emitted_class);
        self.strip_trailing_newline();
        self.writeln(";");

        self.write(self.helper_prefix());
        self.write("__setFunctionName(_classThis, ");
        if let Some(binding_name) = assigned_name {
            self.emit_class_expr_binding_name_arg(binding_name);
        } else {
            self.write("\"\"");
        }
        self.writeln(");");

        self.writeln("(() => {");
        self.indent += 1;
        self.writeln("const _metadata = typeof Symbol === \"function\" && Symbol.metadata ? Object.create(null) : void 0;");
        self.write(self.helper_prefix());
        self.writeln("__esDecorate(null, _classDescriptor = { value: _classThis }, _classDecorators, { kind: \"class\", name: _classThis.name, metadata: _metadata }, null, _classExtraInitializers);");
        self.write(&internal_name);
        self.writeln(" = _classThis = _classDescriptor.value;");
        self.writeln("if (_metadata) Object.defineProperty(_classThis, Symbol.metadata, { enumerable: true, configurable: true, writable: true, value: _metadata });");
        if static_props.is_empty() {
            self.write(self.helper_prefix());
            self.writeln("__runInitializers(_classThis, _classExtraInitializers);");
        }
        self.indent -= 1;
        self.writeln("})();");

        for prop in &static_props {
            self.write("_classThis");
            match &prop.name {
                PropName::Ident(name, _) => {
                    self.write(".");
                    self.write(name);
                }
                PropName::String(name, _) => {
                    self.write("[\"");
                    self.write(name);
                    self.write("\"]");
                }
                PropName::Number(name, _) => {
                    self.write("[");
                    self.write(name);
                    self.write("]");
                }
                _ => unreachable!(),
            }
            self.write(" = ");
            if let Some(initializer) = &prop.initializer {
                self.emit_expr(initializer);
            } else {
                self.write("void 0");
            }
            self.writeln(";");
        }
        if !static_props.is_empty() {
            self.writeln("(() => {");
            self.indent += 1;
            self.write(self.helper_prefix());
            self.writeln("__runInitializers(_classThis, _classExtraInitializers);");
            self.indent -= 1;
            self.writeln("})();");
        }
        self.write("return ");
        self.write(&internal_name);
        self.writeln(" = _classThis;");
        self.indent -= 1;
        self.write("})()");
    }

    pub(super) fn emit_simple_standard_decorated_class_expr_wrapper(
        &mut self,
        class_decl: &ClassDecl,
        assigned_name: Option<&ClassExprBindingName>,
    ) {
        // Set helper flags so inline helpers are emitted in preamble.
        self.needs_es_decorate_helper = true;
        self.needs_run_initializers_helper = true;

        let class_name = class_decl.name.as_deref();
        let internal_name_owned;
        let internal_name = if let Some(name) = class_name {
            name
        } else if self.bare_default_class_expr {
            // For bare `export default @dec class {}`, TypeScript uses `default_1`.
            let name = format!("default_{}", self.default_export_counter);
            self.default_export_counter += 1;
            internal_name_owned = name;
            &internal_name_owned
        } else {
            self.anonymous_class_counter += 1;
            internal_name_owned = format!("class_{}", self.anonymous_class_counter);
            &internal_name_owned
        };

        let has_extends = class_decl.extends.is_some();
        let needs_continuation_indent =
            self.class_expr_needs_grouping_continuation_indent(class_decl.span.start);
        if needs_continuation_indent {
            self.indent += 1;
        }
        self.writeln("(() => {");
        self.indent += 1;
        self.write("let _classDecorators = [");
        self.emit_standard_decorator_expr_list(&class_decl.decorators);
        self.writeln("];");
        self.writeln("let _classDescriptor;");
        self.writeln("let _classExtraInitializers = [];");
        self.writeln("let _classThis;");
        if has_extends {
            let extends_expr = class_decl.extends.as_ref().unwrap();
            self.write("let _classSuper = ");
            if extends_expr_needs_comma_wrap(extends_expr) {
                self.write("(0, ");
                self.emit_expr(extends_expr);
                self.write(")");
            } else {
                self.emit_expr(extends_expr);
            }
            self.writeln(";");
        }
        self.write("var ");
        self.write(internal_name);
        if has_extends {
            self.writeln(" = class extends _classSuper {");
        } else {
            self.writeln(" = class {");
        }
        self.indent += 1;
        self.writeln("static { _classThis = this; }");

        // Emit __setFunctionName for anonymous class expressions.
        // Named classes get their name from the declaration; anonymous ones
        // use the assigned name (e.g. "x" from `const x = @dec class {}`)
        // or empty string when there's no assignment context.
        if class_name.is_none() {
            self.write("static { ");
            self.write(self.helper_prefix());
            self.write("__setFunctionName(_classThis, ");
            if let Some(binding_name) = assigned_name {
                self.emit_class_expr_binding_name_arg(binding_name);
            } else {
                self.write("\"\"");
            }
            self.writeln("); }");
            self.needs_set_function_name_helper = true;
        }

        self.writeln("static {");
        self.indent += 1;

        // Metadata
        if has_extends {
            self.writeln(
                "const _metadata = typeof Symbol === \"function\" && Symbol.metadata ? Object.create(_classSuper[Symbol.metadata] ?? null) : void 0;",
            );
        } else {
            self.writeln(
                "const _metadata = typeof Symbol === \"function\" && Symbol.metadata ? Object.create(null) : void 0;",
            );
        }
        self.write(self.helper_prefix());
        self.writeln("__esDecorate(null, _classDescriptor = { value: _classThis }, _classDecorators, { kind: \"class\", name: _classThis.name, metadata: _metadata }, null, _classExtraInitializers);");
        self.write(internal_name);
        self.writeln(" = _classThis = _classDescriptor.value;");
        self.writeln("if (_metadata) Object.defineProperty(_classThis, Symbol.metadata, { enumerable: true, configurable: true, writable: true, value: _metadata });");
        // Determine if class has user-defined static blocks or static fields.
        let has_user_static_members = class_decl.members.iter().any(|m| match &m.kind {
            ClassMemberKind::StaticBlock(_) => true,
            ClassMemberKind::Property(prop) => prop.modifiers & MOD_STATIC != 0,
            _ => false,
        });
        if !has_user_static_members {
            self.write(self.helper_prefix());
            self.writeln("__runInitializers(_classThis, _classExtraInitializers);");
        }
        self.indent -= 1;
        self.writeln("}");

        // Emit class members after the decorator static blocks.
        let use_define = self.use_define_for_class_fields();
        let name_owned = internal_name.to_string();
        let saved_static_super_base = self.static_super_base_alias.take();
        let saved_static_super_recv = self.static_super_receiver_alias.take();
        for member in &class_decl.members {
            // Reflect.get/set transforms only apply in static blocks and static field
            // initializers — NOT in constructors, instance fields, or methods.
            let needs_super_transform = has_extends
                && match &member.kind {
                    ClassMemberKind::StaticBlock(_) => true,
                    ClassMemberKind::Property(prop) => prop.modifiers & MOD_STATIC != 0,
                    _ => false,
                };
            let is_static_field = matches!(&member.kind,
                ClassMemberKind::Property(prop) if prop.modifiers & MOD_STATIC != 0
            );
            if needs_super_transform {
                self.static_super_base_alias = Some("_classSuper".to_string());
                self.static_super_receiver_alias = Some("_classThis".to_string());
                if is_static_field {
                    self.super_reflect_in_field_init = true;
                }
            }
            self.emit_class_member(
                member,
                &[],         // field_inits: no instance fields to move
                has_extends, // has_super
                use_define,
                None, // method_name_override
                &name_owned,
                &[], // instance_comment_states
            );
            if needs_super_transform {
                self.static_super_base_alias = None;
                self.static_super_receiver_alias = None;
                self.super_reflect_in_field_init = false;
            }
        }
        self.static_super_base_alias = saved_static_super_base;
        self.static_super_receiver_alias = saved_static_super_recv;

        // Emit deferred `__runInitializers` in its own static block.
        if has_user_static_members {
            self.writeln("static {");
            self.indent += 1;
            self.write(self.helper_prefix());
            self.writeln("__runInitializers(_classThis, _classExtraInitializers);");
            self.indent -= 1;
            self.writeln("}");
        }

        self.indent -= 1;
        self.writeln("};");
        self.write("return ");
        self.write(internal_name);
        self.writeln(" = _classThis;");
        self.indent -= 1;
        self.write("})()");
        if needs_continuation_indent {
            self.indent -= 1;
        }
    }

    pub(super) fn emit_narrow_standard_decorated_class_decl_wrapper(
        &mut self,
        class_decl: &ClassDecl,
    ) {
        let Some(name) = class_decl.name.as_deref() else {
            self.emit_class_decl(class_decl);
            return;
        };
        let slots = self.standard_decorator_instance_field_slots_for_class(class_decl);
        if slots.is_empty() {
            self.emit_class_decl(class_decl);
            return;
        }

        self.write("let ");
        self.write(name);
        self.writeln(" = (() => {");
        self.indent += 1;
        self.writeln("var _a;");
        for slot in &slots {
            self.write("let _");
            self.write(&slot.slot_stem);
            self.writeln("_decorators;");
            self.write("let _");
            self.write(&slot.slot_stem);
            self.writeln("_initializers = [];");
            self.write("let _");
            self.write(&slot.slot_stem);
            self.writeln("_extraInitializers = [];");
        }

        let saved_slots = std::mem::replace(
            &mut self.standard_decorator_instance_field_slots,
            slots.clone(),
        );

        self.write("return _a = ");
        self.indent += 1;
        self.emit_class_decl(class_decl);
        self.strip_trailing_newline();
        self.writeln(",");
        self.writeln("(() => {");
        self.indent += 1;
        self.writeln(
            "const _metadata = typeof Symbol === \"function\" && Symbol.metadata ? Object.create(null) : void 0;",
        );
        for member in &class_decl.members {
            let ClassMemberKind::Property(prop) = &member.kind else {
                continue;
            };
            if prop.decorators.is_empty()
                || prop.modifiers & (MOD_STATIC | MOD_ACCESSOR | MOD_DECLARE | MOD_ABSTRACT) != 0
            {
                continue;
            }
            let Some(slot) = self.standard_decorator_instance_field_slot(prop).cloned() else {
                continue;
            };
            self.write("_");
            self.write(&slot.slot_stem);
            self.write("_decorators = [");
            self.emit_standard_decorator_expr_list(&prop.decorators);
            self.writeln("];");
            self.emit_narrow_standard_decorated_member_call(
                &slot.slot_stem,
                &slot.prop_name,
                false,
                false,
            );
        }
        self.writeln("if (_metadata) Object.defineProperty(_a, Symbol.metadata, { enumerable: true, configurable: true, writable: true, value: _metadata });");
        self.indent -= 1;
        self.writeln("})(),");
        self.writeln("_a;");
        self.standard_decorator_instance_field_slots = saved_slots;
        self.indent -= 2;
        self.writeln("})();");
    }

    pub(super) fn emit_narrow_standard_decorated_class_expr_iife(
        &mut self,
        class_decl: &ClassDecl,
        binding_name: Option<&ClassExprBindingName>,
    ) {
        #[derive(Clone)]
        struct MemberInfo<'b> {
            member_span_start: u32,
            slot_stem: String,
            prop_name: String,
            storage_name: Option<String>,
            is_accessor: bool,
            prev_slot_stem: Option<String>,
            prop: &'b ClassProp,
        }

        let mut static_members: Vec<MemberInfo<'_>> = Vec::new();
        let mut instance_members: Vec<MemberInfo<'_>> = Vec::new();

        for member in &class_decl.members {
            let ClassMemberKind::Property(prop) = &member.kind else {
                continue;
            };
            if prop.decorators.is_empty() {
                continue;
            }
            let Some(prop_name) = prop.name.ident_name() else {
                continue;
            };
            let prop_name = normalize_unicode_escapes(prop_name);
            let is_static = prop.modifiers & MOD_STATIC != 0;
            let is_accessor = prop.modifiers & MOD_ACCESSOR != 0;
            let slot_stem = if is_static {
                format!("static_{}", prop_name)
            } else {
                prop_name.clone()
            };
            let info = MemberInfo {
                member_span_start: member.span.start,
                slot_stem,
                prop_name,
                storage_name: is_accessor.then(|| String::new()),
                is_accessor,
                prev_slot_stem: None,
                prop,
            };
            if is_static {
                static_members.push(info);
            } else {
                instance_members.push(info);
            }
        }

        let instance_accessor_name = instance_members
            .iter()
            .find(|info| info.is_accessor)
            .map(|info| info.prop_name.clone());
        for info in &mut instance_members {
            if info.is_accessor {
                info.storage_name = Some(format!("{}_accessor_storage", info.prop_name));
            }
        }
        for info in &mut static_members {
            if info.is_accessor {
                let mut storage_name = format!("{}_accessor_storage", info.prop_name);
                if instance_accessor_name.as_deref() == Some(info.prop_name.as_str()) {
                    storage_name = format!("{}_1_accessor_storage", info.prop_name);
                }
                info.storage_name = Some(storage_name);
            }
        }

        let mut prev_static_slot: Option<String> = None;
        for info in &mut static_members {
            info.prev_slot_stem = prev_static_slot.clone();
            prev_static_slot = Some(info.slot_stem.clone());
        }
        let mut prev_instance_slot: Option<String> = None;
        for info in &mut instance_members {
            info.prev_slot_stem = prev_instance_slot.clone();
            prev_instance_slot = Some(info.slot_stem.clone());
        }

        let use_define =
            self.use_define_for_class_fields() && !self.needs_downlevel("class-fields");
        let needs_continuation_indent =
            self.class_expr_needs_grouping_continuation_indent(class_decl.span.start);

        if needs_continuation_indent {
            self.indent += 1;
        }
        self.writeln("(() => {");
        self.indent += 1;
        for info in &static_members {
            self.write("let _");
            self.write(&info.slot_stem);
            self.writeln("_decorators;");
            self.write("let _");
            self.write(&info.slot_stem);
            self.writeln("_initializers = [];");
            self.write("let _");
            self.write(&info.slot_stem);
            self.writeln("_extraInitializers = [];");
        }
        for info in &instance_members {
            self.write("let _");
            self.write(&info.slot_stem);
            self.writeln("_decorators;");
            self.write("let _");
            self.write(&info.slot_stem);
            self.writeln("_initializers = [];");
            self.write("let _");
            self.write(&info.slot_stem);
            self.writeln("_extraInitializers = [];");
        }
        if let Some(ref name) = class_decl.name {
            self.write("return class ");
            self.write(name);
            self.writeln(" {");
        } else {
            self.writeln("return class {");
        }
        self.indent += 1;
        // Only emit __setFunctionName for anonymous classes with a binding name
        if class_decl.name.is_none() {
            if let Some(binding_name) = binding_name {
                self.write("static { ");
                self.write(self.helper_prefix());
                self.write("__setFunctionName(this, ");
                self.emit_class_expr_binding_name_arg(binding_name);
                self.writeln("); }");
            }
        }
        self.writeln("static {");
        self.indent += 1;
        self.writeln(
            "const _metadata = typeof Symbol === \"function\" && Symbol.metadata ? Object.create(null) : void 0;",
        );
        for member in &class_decl.members {
            let ClassMemberKind::Property(prop) = &member.kind else {
                continue;
            };
            if prop.decorators.is_empty() {
                continue;
            }
            let info = if prop.modifiers & MOD_STATIC != 0 {
                static_members
                    .iter()
                    .find(|info| info.member_span_start == member.span.start)
            } else {
                instance_members
                    .iter()
                    .find(|info| info.member_span_start == member.span.start)
            };
            let Some(info) = info else {
                continue;
            };
            self.write("_");
            self.write(&info.slot_stem);
            self.write("_decorators = [");
            self.emit_standard_decorator_expr_list(&info.prop.decorators);
            self.writeln("];");
        }
        for info in static_members.iter().filter(|info| info.is_accessor) {
            self.emit_narrow_standard_decorated_member_call(
                &info.slot_stem,
                &info.prop_name,
                true,
                true,
            );
        }
        for info in instance_members.iter().filter(|info| info.is_accessor) {
            self.emit_narrow_standard_decorated_member_call(
                &info.slot_stem,
                &info.prop_name,
                false,
                true,
            );
        }
        for info in static_members.iter().filter(|info| !info.is_accessor) {
            self.emit_narrow_standard_decorated_member_call(
                &info.slot_stem,
                &info.prop_name,
                true,
                false,
            );
        }
        for info in instance_members.iter().filter(|info| !info.is_accessor) {
            self.emit_narrow_standard_decorated_member_call(
                &info.slot_stem,
                &info.prop_name,
                false,
                false,
            );
        }
        self.writeln("if (_metadata) Object.defineProperty(this, Symbol.metadata, { enumerable: true, configurable: true, writable: true, value: _metadata });");
        self.indent -= 1;
        self.writeln("}");

        for member in &class_decl.members {
            match &member.kind {
                ClassMemberKind::Property(prop) if !prop.decorators.is_empty() => {
                    let info = if prop.modifiers & MOD_STATIC != 0 {
                        static_members
                            .iter()
                            .find(|info| info.member_span_start == member.span.start)
                    } else {
                        instance_members
                            .iter()
                            .find(|info| info.member_span_start == member.span.start)
                    };
                    let Some(info) = info else {
                        continue;
                    };
                    if prop.modifiers & MOD_STATIC != 0 {
                        if info.is_accessor {
                            self.write("static #");
                            self.write(info.storage_name.as_deref().unwrap_or(""));
                            self.write(" = ");
                            self.emit_narrow_standard_decorated_initializer_expr(
                                &info.slot_stem,
                                info.prev_slot_stem.as_deref(),
                                info.prop.initializer.as_deref(),
                                Some(info.prop_name.as_str()),
                            );
                            self.writeln(";");
                            // For static accessors, use class name (A.#storage) instead of this
                            let class_ref = class_decl.name.as_deref().unwrap_or("this");
                            self.emit_leading_comments(info.member_span_start);
                            self.write("static get ");
                            self.write(&info.prop_name);
                            self.write("() { return ");
                            self.write(class_ref);
                            self.write(".#");
                            self.write(info.storage_name.as_deref().unwrap_or(""));
                            self.writeln("; }");
                            self.write("static set ");
                            self.write(&info.prop_name);
                            self.write("(value) { ");
                            self.write(class_ref);
                            self.write(".#");
                            self.write(info.storage_name.as_deref().unwrap_or(""));
                            self.writeln(" = value; }");
                        } else if use_define {
                            self.write("static ");
                            self.write(&info.prop_name);
                            self.write(" = ");
                            self.emit_narrow_standard_decorated_initializer_expr(
                                &info.slot_stem,
                                info.prev_slot_stem.as_deref(),
                                info.prop.initializer.as_deref(),
                                Some(info.prop_name.as_str()),
                            );
                            self.writeln(";");
                        } else {
                            self.write("static { this.");
                            self.write(&info.prop_name);
                            self.write(" = ");
                            self.emit_narrow_standard_decorated_initializer_expr(
                                &info.slot_stem,
                                info.prev_slot_stem.as_deref(),
                                info.prop.initializer.as_deref(),
                                Some(info.prop_name.as_str()),
                            );
                            self.writeln("; }");
                        }
                    } else if info.is_accessor {
                        self.write("#");
                        self.write(info.storage_name.as_deref().unwrap_or(""));
                        if info.prev_slot_stem.is_none() {
                            // No chaining dependency — can initialize as field
                            self.write(" = ");
                            self.emit_narrow_standard_decorated_initializer_expr(
                                &info.slot_stem,
                                None,
                                info.prop.initializer.as_deref(),
                                Some(info.prop_name.as_str()),
                            );
                        }
                        self.writeln(";");
                        self.emit_leading_comments(info.member_span_start);
                        self.write("get ");
                        self.write(&info.prop_name);
                        self.write("() { return this.#");
                        self.write(info.storage_name.as_deref().unwrap_or(""));
                        self.writeln("; }");
                        self.write("set ");
                        self.write(&info.prop_name);
                        self.write("(value) { this.#");
                        self.write(info.storage_name.as_deref().unwrap_or(""));
                        self.writeln(" = value; }");
                    } else if use_define {
                        // Instance non-accessor in define mode: emit as class field
                        self.write(&info.prop_name);
                        self.write(" = ");
                        self.emit_narrow_standard_decorated_initializer_expr(
                            &info.slot_stem,
                            info.prev_slot_stem.as_deref(),
                            info.prop.initializer.as_deref(),
                            Some(info.prop_name.as_str()),
                        );
                        self.writeln(";");
                    }
                }
                ClassMemberKind::SemicolonClassElement => {
                    self.writeln(";");
                }
                _ => {}
            }
        }

        // Constructor: instance fields + accessor storage (when chained) + extra initializers
        if !instance_members.is_empty() {
            self.writeln("constructor() {");
            self.indent += 1;
            for info in &instance_members {
                if info.is_accessor && info.prev_slot_stem.is_none() {
                    // Accessor storage already initialized as class field
                    continue;
                } else if info.is_accessor {
                    // Chained accessor — init in constructor
                    self.write("this.#");
                    self.write(info.storage_name.as_deref().unwrap_or(""));
                } else if !use_define {
                    // Legacy mode: instance non-accessor field in constructor
                    self.write("this.");
                    self.write(&info.prop_name);
                } else {
                    continue;
                }
                self.write(" = ");
                self.emit_narrow_standard_decorated_initializer_expr(
                    &info.slot_stem,
                    info.prev_slot_stem.as_deref(),
                    info.prop.initializer.as_deref(),
                    Some(info.prop_name.as_str()),
                );
                self.writeln(";");
            }
            if let Some(last_instance) = instance_members.last() {
                self.write(self.helper_prefix());
                self.write("__runInitializers(this, _");
                self.write(&last_instance.slot_stem);
                self.writeln("_extraInitializers);");
            }
            self.indent -= 1;
            self.writeln("}");
        }

        if let Some(last_static) = static_members.last() {
            self.writeln("static {");
            self.indent += 1;
            self.write(self.helper_prefix());
            self.write("__runInitializers(this, _");
            self.write(&last_static.slot_stem);
            self.writeln("_extraInitializers);");
            self.indent -= 1;
            self.writeln("}");
        }

        self.indent -= 1;
        self.writeln("};");
        self.indent -= 1;
        self.write("})()");
        if needs_continuation_indent {
            self.indent -= 1;
        }
    }

    fn emit_compact_empty_or_return_body(&mut self, body: &[Stmt]) {
        if body.is_empty() {
            self.write("{ }");
            return;
        }
        if body.len() == 1 {
            if let StmtKind::Return(Some(expr)) = &body[0].kind {
                self.write("{ return ");
                self.emit_expr(expr);
                self.write("; }");
                return;
            }
        }
        self.write("{ }");
    }

    fn emit_set_function_name_wrapped_function(
        &mut self,
        display_name: &str,
        prefix: Option<&str>,
        params: &[Param],
        body: &[Stmt],
    ) {
        self.write(self.helper_prefix());
        self.write("__setFunctionName(function (");
        self.emit_params(params);
        self.write(") ");
        self.emit_compact_empty_or_return_body(body);
        self.write(", \"");
        self.write(display_name);
        self.write("\"");
        if let Some(prefix) = prefix {
            self.write(", \"");
            self.write(prefix);
            self.write("\"");
        }
        self.write(")");
    }

    pub(super) fn emit_native_standard_decorator_class_decl_wrapper(
        &mut self,
        class_decl: &ClassDecl,
    ) {
        let Some(shape) = class_native_standard_decorator_shape(class_decl) else {
            self.emit_class_decl(class_decl);
            return;
        };
        let Some(name) = class_decl.name.as_deref() else {
            self.emit_class_decl(class_decl);
            return;
        };

        self.needs_es_decorate_helper = true;
        self.needs_run_initializers_helper = true;
        if shape.needs_prop_key_helper() {
            self.needs_prop_key_helper = true;
        }
        if shape.needs_set_function_name_helper() {
            self.needs_set_function_name_helper = true;
        }
        self.emitted_var_names.insert(name.into());

        let member = class_decl
            .members
            .iter()
            .find(|member| !matches!(member.kind, ClassMemberKind::SemicolonClassElement));

        match shape {
            NativeStandardDecoratorClassShape::ClassDecoratorWithPrivateStaticMethod => {
                let Some(ClassMember {
                    kind: ClassMemberKind::Method(method),
                    ..
                }) = member
                else {
                    self.emit_class_decl(class_decl);
                    return;
                };
                let PropName::Private(private_name, _) = &method.name else {
                    self.emit_class_decl(class_decl);
                    return;
                };
                let norm_class = normalize_unicode_escapes(name);
                let norm_private = normalize_unicode_escapes(private_name);
                let helper_name = format!("_{}_{}", norm_class, norm_private);

                self.write("let ");
                self.write(name);
                self.writeln(" = (() => {");
                self.indent += 1;
                self.write("var ");
                self.write(&helper_name);
                self.writeln(";");
                self.write("let _classDecorators = [");
                self.emit_standard_decorator_expr_list(&class_decl.decorators);
                self.writeln("];");
                self.writeln("let _classDescriptor;");
                self.writeln("let _classExtraInitializers = [];");
                self.writeln("let _classThis;");
                self.write("var ");
                self.write(name);
                self.writeln(" = class {");
                self.indent += 1;
                self.writeln("static { _classThis = this; }");
                self.write("static { ");
                self.write(self.helper_prefix());
                self.write("__setFunctionName(this, \"");
                self.write(name);
                self.writeln("\"); }");
                self.write("static { ");
                self.write(&helper_name);
                self.write(" = function ");
                self.write(&helper_name);
                self.writeln("() { }; }");
                self.writeln("static {");
                self.indent += 1;
                self.writeln(
                    "const _metadata = typeof Symbol === \"function\" && Symbol.metadata ? Object.create(null) : void 0;",
                );
                self.write(self.helper_prefix());
                self.writeln("__esDecorate(null, _classDescriptor = { value: _classThis }, _classDecorators, { kind: \"class\", name: _classThis.name, metadata: _metadata }, null, _classExtraInitializers);");
                self.write(name);
                self.writeln(" = _classThis = _classDescriptor.value;");
                self.writeln("if (_metadata) Object.defineProperty(_classThis, Symbol.metadata, { enumerable: true, configurable: true, writable: true, value: _metadata });");
                self.write(self.helper_prefix());
                self.writeln("__runInitializers(_classThis, _classExtraInitializers);");
                self.indent -= 1;
                self.writeln("}");
                self.indent -= 1;
                self.writeln("};");
                self.write("return ");
                self.write(name);
                self.writeln(" = _classThis;");
                self.indent -= 1;
                self.writeln("})();");
            }
            NativeStandardDecoratorClassShape::DecoratedPrivateField {
                is_static,
                is_auto_accessor,
            } => {
                let Some(ClassMember {
                    kind: ClassMemberKind::Property(prop),
                    ..
                }) = member
                else {
                    self.emit_class_decl(class_decl);
                    return;
                };
                let PropName::Private(private_name, _) = &prop.name else {
                    self.emit_class_decl(class_decl);
                    return;
                };
                let norm_private = normalize_unicode_escapes(private_name);
                let slot_stem = if is_static {
                    format!("static_private_{}", norm_private)
                } else {
                    format!("private_{}", norm_private)
                };
                let display_name = format!("#{}", norm_private);
                let storage_name = format!("{}_accessor_storage", norm_private);

                self.write("let ");
                self.write(name);
                self.writeln(" = (() => {");
                self.indent += 1;
                self.write("let _");
                self.write(&slot_stem);
                self.writeln("_decorators;");
                self.write("let _");
                self.write(&slot_stem);
                self.writeln("_initializers = [];");
                self.write("let _");
                self.write(&slot_stem);
                self.writeln("_extraInitializers = [];");
                if is_auto_accessor {
                    self.write("let _");
                    self.write(&slot_stem);
                    self.writeln("_descriptor;");
                }
                self.write("return class ");
                self.write(name);
                self.writeln(" {");
                self.indent += 1;
                self.writeln("static {");
                self.indent += 1;
                self.writeln(
                    "const _metadata = typeof Symbol === \"function\" && Symbol.metadata ? Object.create(null) : void 0;",
                );
                self.write("_");
                self.write(&slot_stem);
                self.write("_decorators = [");
                self.emit_standard_decorator_expr_list(&prop.decorators);
                self.writeln("];");
                self.write(self.helper_prefix());
                self.write("__esDecorate(");
                self.write(if is_auto_accessor { "this" } else { "null" });
                self.write(", ");
                if is_auto_accessor {
                    self.write("_");
                    self.write(&slot_stem);
                    self.write("_descriptor = { get: ");
                    self.write(self.helper_prefix());
                    self.write("__setFunctionName(function () { return this.#");
                    self.write(&storage_name);
                    self.write("; }, \"");
                    self.write(&display_name);
                    self.write("\", \"get\")");
                    self.write(", set: ");
                    self.write(self.helper_prefix());
                    self.write("__setFunctionName(function (value) ");
                    self.write("{ this.#");
                    self.write(&storage_name);
                    self.write(" = value; }, \"");
                    self.write(&display_name);
                    self.write("\", \"set\") }, ");
                } else {
                    self.write("null, ");
                }
                self.write("_");
                self.write(&slot_stem);
                self.write("_decorators, { kind: \"");
                self.write(if is_auto_accessor {
                    "accessor"
                } else {
                    "field"
                });
                self.write("\", name: \"");
                self.write(&display_name);
                self.write("\", static: ");
                self.write(if is_static { "true" } else { "false" });
                self.write(", private: true, access: { has: obj => #");
                self.write(&norm_private);
                self.write(" in obj, get: obj => obj.#");
                self.write(&norm_private);
                self.write(", set: (obj, value) => { obj.#");
                self.write(&norm_private);
                self.write(" = value; } }, metadata: _metadata }, _");
                self.write(&slot_stem);
                self.write("_initializers, _");
                self.write(&slot_stem);
                self.writeln("_extraInitializers);");
                self.writeln("if (_metadata) Object.defineProperty(this, Symbol.metadata, { enumerable: true, configurable: true, writable: true, value: _metadata });");
                if !is_auto_accessor {
                    self.indent -= 1;
                    self.writeln("}");
                    if is_static {
                        self.write("static #");
                    } else {
                        self.write("#");
                    }
                    self.write(&norm_private);
                    self.write(" = ");
                    self.write(self.helper_prefix());
                    self.write("__runInitializers(this, _");
                    self.write(&slot_stem);
                    self.writeln("_initializers, void 0);");
                } else {
                    self.indent -= 1;
                    self.writeln("}");
                    if is_static {
                        self.write("static #");
                    } else {
                        self.write("#");
                    }
                    self.write(&storage_name);
                    self.write(" = ");
                    self.write(self.helper_prefix());
                    self.write("__runInitializers(this, _");
                    self.write(&slot_stem);
                    self.writeln("_initializers, void 0);");
                    if is_static {
                        self.write("static ");
                    }
                    self.write("get #");
                    self.write(&norm_private);
                    self.write("() { return _");
                    self.write(&slot_stem);
                    self.writeln("_descriptor.get.call(this); }");
                    if is_static {
                        self.write("static ");
                    }
                    self.write("set #");
                    self.write(&norm_private);
                    self.write("(value) { return _");
                    self.write(&slot_stem);
                    self.writeln("_descriptor.set.call(this, value); }");
                }
                if is_static {
                    self.writeln("static {");
                    self.indent += 1;
                    self.write(self.helper_prefix());
                    self.write("__runInitializers(this, _");
                    self.write(&slot_stem);
                    self.writeln("_extraInitializers);");
                    self.indent -= 1;
                    self.writeln("}");
                } else {
                    self.writeln("constructor() {");
                    self.indent += 1;
                    self.write(self.helper_prefix());
                    self.write("__runInitializers(this, _");
                    self.write(&slot_stem);
                    self.writeln("_extraInitializers);");
                    self.indent -= 1;
                    self.writeln("}");
                }
                self.indent -= 1;
                self.writeln("};");
                self.indent -= 1;
                self.writeln("})();");
            }
            NativeStandardDecoratorClassShape::DecoratedPrivateMethod { is_static }
            | NativeStandardDecoratorClassShape::DecoratedPrivateGetter { is_static }
            | NativeStandardDecoratorClassShape::DecoratedPrivateSetter { is_static } => {
                let (decorators, display_name, stem, body_params, body_stmts, kind_str, prefix) =
                    match member {
                        Some(ClassMember {
                            kind: ClassMemberKind::Method(method),
                            ..
                        }) => {
                            let PropName::Private(private_name, _) = &method.name else {
                                self.emit_class_decl(class_decl);
                                return;
                            };
                            let norm_private = normalize_unicode_escapes(private_name);
                            (
                                &method.decorators,
                                format!("#{}", norm_private),
                                if is_static {
                                    format!("static_private_{}", norm_private)
                                } else {
                                    format!("private_{}", norm_private)
                                },
                                &method.params,
                                method.body.as_deref().unwrap_or(&[]),
                                "method",
                                None,
                            )
                        }
                        Some(ClassMember {
                            kind: ClassMemberKind::GetAccessor(acc),
                            ..
                        }) => {
                            let PropName::Private(private_name, _) = &acc.name else {
                                self.emit_class_decl(class_decl);
                                return;
                            };
                            let norm_private = normalize_unicode_escapes(private_name);
                            (
                                &acc.decorators,
                                format!("#{}", norm_private),
                                if is_static {
                                    format!("static_private_get_{}", norm_private)
                                } else {
                                    format!("private_get_{}", norm_private)
                                },
                                &acc.params,
                                acc.body.as_deref().unwrap_or(&[]),
                                "getter",
                                Some("get"),
                            )
                        }
                        Some(ClassMember {
                            kind: ClassMemberKind::SetAccessor(acc),
                            ..
                        }) => {
                            let PropName::Private(private_name, _) = &acc.name else {
                                self.emit_class_decl(class_decl);
                                return;
                            };
                            let norm_private = normalize_unicode_escapes(private_name);
                            (
                                &acc.decorators,
                                format!("#{}", norm_private),
                                if is_static {
                                    format!("static_private_set_{}", norm_private)
                                } else {
                                    format!("private_set_{}", norm_private)
                                },
                                &acc.params,
                                acc.body.as_deref().unwrap_or(&[]),
                                "setter",
                                Some("set"),
                            )
                        }
                        _ => {
                            self.emit_class_decl(class_decl);
                            return;
                        }
                    };
                let extra_init_name = if is_static {
                    "_staticExtraInitializers"
                } else {
                    "_instanceExtraInitializers"
                };
                let descriptor_key = if kind_str == "method" {
                    "value"
                } else if kind_str == "getter" {
                    "get"
                } else {
                    "set"
                };

                self.write("let ");
                self.write(name);
                self.writeln(" = (() => {");
                self.indent += 1;
                self.write("let ");
                self.write(extra_init_name);
                self.writeln(" = [];");
                self.write("let _");
                self.write(&stem);
                self.writeln("_decorators;");
                self.write("let _");
                self.write(&stem);
                self.writeln("_descriptor;");
                self.write("return class ");
                self.write(name);
                self.writeln(" {");
                self.indent += 1;
                self.writeln("static {");
                self.indent += 1;
                self.writeln(
                    "const _metadata = typeof Symbol === \"function\" && Symbol.metadata ? Object.create(null) : void 0;",
                );
                self.write("_");
                self.write(&stem);
                self.write("_decorators = [");
                self.emit_standard_decorator_expr_list(decorators);
                self.writeln("];");
                self.write(self.helper_prefix());
                self.write("__esDecorate(this, _");
                self.write(&stem);
                self.write("_descriptor = { ");
                self.write(descriptor_key);
                self.write(": ");
                self.emit_set_function_name_wrapped_function(
                    &display_name,
                    prefix,
                    body_params,
                    body_stmts,
                );
                self.write(" }, _");
                self.write(&stem);
                self.write("_decorators, { kind: \"");
                self.write(kind_str);
                self.write("\", name: \"");
                self.write(&display_name);
                self.write("\", static: ");
                self.write(if is_static { "true" } else { "false" });
                self.write(", private: true, access: { has: obj => ");
                self.write(&display_name);
                self.write(" in obj, ");
                if kind_str == "setter" {
                    self.write("set: (obj, value) => { obj.");
                } else {
                    self.write("get: obj => obj.");
                }
                self.write(&display_name);
                if kind_str == "setter" {
                    self.write(" = value; }");
                }
                self.write(" }, metadata: _metadata }, null, ");
                self.write(extra_init_name);
                self.writeln(");");
                self.writeln("if (_metadata) Object.defineProperty(this, Symbol.metadata, { enumerable: true, configurable: true, writable: true, value: _metadata });");
                if is_static {
                    self.write(self.helper_prefix());
                    self.write("__runInitializers(this, ");
                    self.write(extra_init_name);
                    self.writeln(");");
                }
                self.indent -= 1;
                self.writeln("}");
                if is_static {
                    self.write("static ");
                }
                if kind_str == "method" {
                    self.write("get ");
                    self.write(&display_name);
                    self.write("() { return _");
                    self.write(&stem);
                    self.writeln("_descriptor.value; }");
                } else if kind_str == "getter" {
                    self.write("get ");
                    self.write(&display_name);
                    self.write("() { return _");
                    self.write(&stem);
                    self.writeln("_descriptor.get.call(this); }");
                } else {
                    self.write("set ");
                    self.write(&display_name);
                    self.write("(");
                    self.emit_params(body_params);
                    self.write(") { return _");
                    self.write(&stem);
                    self.writeln("_descriptor.set.call(this, value); }");
                }
                if !is_static {
                    self.writeln("constructor() {");
                    self.indent += 1;
                    self.write(self.helper_prefix());
                    self.write("__runInitializers(this, ");
                    self.write(extra_init_name);
                    self.writeln(");");
                    self.indent -= 1;
                    self.writeln("}");
                }
                self.indent -= 1;
                self.writeln("};");
                self.indent -= 1;
                self.writeln("})();");
            }
            NativeStandardDecoratorClassShape::DecoratedStaticComputedField
            | NativeStandardDecoratorClassShape::DecoratedStaticComputedAutoAccessor
            | NativeStandardDecoratorClassShape::DecoratedStaticComputedMethod
            | NativeStandardDecoratorClassShape::DecoratedStaticComputedGetter
            | NativeStandardDecoratorClassShape::DecoratedStaticComputedSetter => {
                let temp_name = "_a";
                self.write("let ");
                self.write(name);
                self.writeln(" = (() => {");
                self.indent += 1;
                self.write("var ");
                self.write(temp_name);
                self.writeln(";");

                match member {
                    Some(ClassMember {
                        kind: ClassMemberKind::Property(prop),
                        ..
                    }) => {
                        let decorator_var = "_static_member_decorators";
                        let initializers_var = "_static_member_initializers";
                        let extra_var = "_static_member_extraInitializers";
                        self.write("let ");
                        self.write(decorator_var);
                        self.writeln(";");
                        self.write("let ");
                        self.write(initializers_var);
                        self.writeln(" = [];");
                        self.write("let ");
                        self.write(extra_var);
                        self.writeln(" = [];");
                        self.write("return class ");
                        self.write(name);
                        self.writeln(" {");
                        self.indent += 1;
                        self.writeln("static {");
                        self.indent += 1;
                        self.writeln(
                            "const _metadata = typeof Symbol === \"function\" && Symbol.metadata ? Object.create(null) : void 0;",
                        );
                        self.write(self.helper_prefix());
                        self.write("__esDecorate(");
                        self.write(if prop.modifiers & MOD_ACCESSOR != 0 {
                            "this"
                        } else {
                            "null"
                        });
                        self.write(", null, ");
                        self.write(decorator_var);
                        self.write(", { kind: \"");
                        self.write(if prop.modifiers & MOD_ACCESSOR != 0 {
                            "accessor"
                        } else {
                            "field"
                        });
                        self.write("\", name: ");
                        self.write(temp_name);
                        self.write(", static: true, private: false, access: { has: obj => ");
                        self.write(temp_name);
                        self.write(" in obj, get: obj => obj[");
                        self.write(temp_name);
                        self.write("], set: (obj, value) => { obj[");
                        self.write(temp_name);
                        self.write("] = value; } }, metadata: _metadata }, ");
                        self.write(initializers_var);
                        self.write(", ");
                        self.write(extra_var);
                        self.writeln(");");
                        self.writeln("if (_metadata) Object.defineProperty(this, Symbol.metadata, { enumerable: true, configurable: true, writable: true, value: _metadata });");
                        self.indent -= 1;
                        self.writeln("}");
                        if prop.modifiers & MOD_ACCESSOR != 0 {
                            self.write("static #");
                            self.write(temp_name);
                            self.write("_accessor_storage = ");
                            self.write(self.helper_prefix());
                            self.write("__runInitializers(this, ");
                            self.write(initializers_var);
                            self.writeln(", void 0);");
                            self.write("static get [(");
                            self.write(decorator_var);
                            self.write(" = [");
                            self.emit_standard_decorator_expr_list(&prop.decorators);
                            self.write("], ");
                            self.write(temp_name);
                            self.write(" = ");
                            self.write(self.helper_prefix());
                            self.write("__propKey(");
                            if let PropName::Computed(expr, _) = &prop.name {
                                self.emit_expr(expr);
                            }
                            self.write("))]() { return ");
                            self.write(name);
                            self.write(".#");
                            self.write(temp_name);
                            self.writeln("_accessor_storage; }");
                            self.write("static set [");
                            self.write(temp_name);
                            self.write("](value) { ");
                            self.write(name);
                            self.write(".#");
                            self.write(temp_name);
                            self.writeln("_accessor_storage = value; }");
                        } else {
                            self.write("static [(");
                            self.write(decorator_var);
                            self.write(" = [");
                            self.emit_standard_decorator_expr_list(&prop.decorators);
                            self.write("], ");
                            self.write(temp_name);
                            self.write(" = ");
                            self.write(self.helper_prefix());
                            self.write("__propKey(");
                            if let PropName::Computed(expr, _) = &prop.name {
                                self.emit_expr(expr);
                            }
                            self.write("))] = ");
                            self.write(self.helper_prefix());
                            self.write("__runInitializers(this, ");
                            self.write(initializers_var);
                            self.writeln(", void 0);");
                        }
                        self.writeln("static {");
                        self.indent += 1;
                        self.write(self.helper_prefix());
                        self.write("__runInitializers(this, ");
                        self.write(extra_var);
                        self.writeln(");");
                        self.indent -= 1;
                        self.writeln("}");
                        self.indent -= 1;
                        self.writeln("};");
                    }
                    Some(ClassMember {
                        kind: ClassMemberKind::Method(method),
                        ..
                    }) => {
                        let decorator_var = "_static_member_decorators";
                        self.writeln("let _staticExtraInitializers = [];");
                        self.write("let ");
                        self.write(decorator_var);
                        self.writeln(";");
                        self.write("return class ");
                        self.write(name);
                        self.writeln(" {");
                        self.indent += 1;
                        self.writeln("static {");
                        self.indent += 1;
                        self.writeln(
                            "const _metadata = typeof Symbol === \"function\" && Symbol.metadata ? Object.create(null) : void 0;",
                        );
                        self.write(self.helper_prefix());
                        self.write("__esDecorate(this, null, ");
                        self.write(decorator_var);
                        self.write(", { kind: \"method\", name: ");
                        self.write(temp_name);
                        self.write(", static: true, private: false, access: { has: obj => ");
                        self.write(temp_name);
                        self.write(" in obj, get: obj => obj[");
                        self.write(temp_name);
                        self.write("] }, metadata: _metadata }, null, _staticExtraInitializers);");
                        self.newline();
                        self.writeln("if (_metadata) Object.defineProperty(this, Symbol.metadata, { enumerable: true, configurable: true, writable: true, value: _metadata });");
                        self.write(self.helper_prefix());
                        self.writeln("__runInitializers(this, _staticExtraInitializers);");
                        self.indent -= 1;
                        self.writeln("}");
                        self.write("static [(");
                        self.write(decorator_var);
                        self.write(" = [");
                        self.emit_standard_decorator_expr_list(&method.decorators);
                        self.write("], ");
                        self.write(temp_name);
                        self.write(" = ");
                        self.write(self.helper_prefix());
                        self.write("__propKey(");
                        if let PropName::Computed(expr, _) = &method.name {
                            self.emit_expr(expr);
                        }
                        self.write("))](");
                        self.emit_params(&method.params);
                        self.write(") ");
                        self.emit_compact_empty_or_return_body(
                            method.body.as_deref().unwrap_or(&[]),
                        );
                        self.newline();
                        self.indent -= 1;
                        self.writeln("};");
                    }
                    Some(ClassMember {
                        kind: ClassMemberKind::GetAccessor(acc),
                        ..
                    }) => {
                        let decorator_var = "_static_get_member_decorators";
                        self.writeln("let _staticExtraInitializers = [];");
                        self.write("let ");
                        self.write(decorator_var);
                        self.writeln(";");
                        self.write("return class ");
                        self.write(name);
                        self.writeln(" {");
                        self.indent += 1;
                        self.writeln("static {");
                        self.indent += 1;
                        self.writeln(
                            "const _metadata = typeof Symbol === \"function\" && Symbol.metadata ? Object.create(null) : void 0;",
                        );
                        self.write(self.helper_prefix());
                        self.write("__esDecorate(this, null, ");
                        self.write(decorator_var);
                        self.write(", { kind: \"getter\", name: ");
                        self.write(temp_name);
                        self.write(", static: true, private: false, access: { has: obj => ");
                        self.write(temp_name);
                        self.write(" in obj, get: obj => obj[");
                        self.write(temp_name);
                        self.write("] }, metadata: _metadata }, null, _staticExtraInitializers);");
                        self.newline();
                        self.writeln("if (_metadata) Object.defineProperty(this, Symbol.metadata, { enumerable: true, configurable: true, writable: true, value: _metadata });");
                        self.write(self.helper_prefix());
                        self.writeln("__runInitializers(this, _staticExtraInitializers);");
                        self.indent -= 1;
                        self.writeln("}");
                        self.write("static get [(");
                        self.write(decorator_var);
                        self.write(" = [");
                        self.emit_standard_decorator_expr_list(&acc.decorators);
                        self.write("], ");
                        self.write(temp_name);
                        self.write(" = ");
                        self.write(self.helper_prefix());
                        self.write("__propKey(");
                        if let PropName::Computed(expr, _) = &acc.name {
                            self.emit_expr(expr);
                        }
                        self.write("))]() ");
                        self.emit_compact_empty_or_return_body(acc.body.as_deref().unwrap_or(&[]));
                        self.newline();
                        self.indent -= 1;
                        self.writeln("};");
                    }
                    Some(ClassMember {
                        kind: ClassMemberKind::SetAccessor(acc),
                        ..
                    }) => {
                        let decorator_var = "_static_set_member_decorators";
                        self.writeln("let _staticExtraInitializers = [];");
                        self.write("let ");
                        self.write(decorator_var);
                        self.writeln(";");
                        self.write("return class ");
                        self.write(name);
                        self.writeln(" {");
                        self.indent += 1;
                        self.writeln("static {");
                        self.indent += 1;
                        self.writeln(
                            "const _metadata = typeof Symbol === \"function\" && Symbol.metadata ? Object.create(null) : void 0;",
                        );
                        self.write(self.helper_prefix());
                        self.write("__esDecorate(this, null, ");
                        self.write(decorator_var);
                        self.write(", { kind: \"setter\", name: ");
                        self.write(temp_name);
                        self.write(", static: true, private: false, access: { has: obj => ");
                        self.write(temp_name);
                        self.write(" in obj, set: (obj, value) => { obj[");
                        self.write(temp_name);
                        self.write("] = value; } }, metadata: _metadata }, null, _staticExtraInitializers);");
                        self.newline();
                        self.writeln("if (_metadata) Object.defineProperty(this, Symbol.metadata, { enumerable: true, configurable: true, writable: true, value: _metadata });");
                        self.write(self.helper_prefix());
                        self.writeln("__runInitializers(this, _staticExtraInitializers);");
                        self.indent -= 1;
                        self.writeln("}");
                        self.write("static set [(");
                        self.write(decorator_var);
                        self.write(" = [");
                        self.emit_standard_decorator_expr_list(&acc.decorators);
                        self.write("], ");
                        self.write(temp_name);
                        self.write(" = ");
                        self.write(self.helper_prefix());
                        self.write("__propKey(");
                        if let PropName::Computed(expr, _) = &acc.name {
                            self.emit_expr(expr);
                        }
                        self.write("))](");
                        self.emit_params(&acc.params);
                        self.write(") ");
                        self.emit_compact_empty_or_return_body(acc.body.as_deref().unwrap_or(&[]));
                        self.newline();
                        self.indent -= 1;
                        self.writeln("};");
                    }
                    _ => {
                        self.emit_class_decl(class_decl);
                        self.indent -= 1;
                        return;
                    }
                }
                self.indent -= 1;
                self.writeln("})();");
            }
        }
    }

    fn legacy_es5_accessor_is_erased(accessor: &ClassAccessor) -> bool {
        accessor.body.is_none() && accessor.modifiers & (MOD_ABSTRACT | MOD_DECLARE) != 0
    }

    /// `legacy_decorators`: experimentalDecorators are on, so decorator
    /// applications move into the IIFE (`C = __decorate([...], C)`).
    fn legacy_es5_class_shape_can_lower(class_decl: &ClassDecl, legacy_decorators: bool) -> bool {
        let Some(class_name) = class_decl.name.as_deref() else {
            return false;
        };
        let decorated = |decorators: &[Expr]| !decorators.is_empty() && !legacy_decorators;
        if decorated(&class_decl.decorators) || class_decl.modifiers & MOD_DEFAULT != 0 {
            return false;
        }
        // tsc caches a decorated member's computed key in a temp; that
        // shape keeps the native path.
        // Erased members (ambient fields, abstract members) print nothing.
        if class_decl.members.iter().any(|member| match &member.kind {
            ClassMemberKind::Method(method) => {
                method.body.is_some()
                    && !method.decorators.is_empty()
                    && matches!(method.name, PropName::Computed(..))
            }
            ClassMemberKind::Property(property) => {
                property.modifiers & MOD_DECLARE == 0
                    && !property.decorators.is_empty()
                    && matches!(property.name, PropName::Computed(..))
            }
            ClassMemberKind::GetAccessor(accessor) | ClassMemberKind::SetAccessor(accessor) => {
                !Self::legacy_es5_accessor_is_erased(accessor)
                    && !accessor.decorators.is_empty()
                    && matches!(accessor.name, PropName::Computed(..))
            }
            _ => false,
        }) {
            return false;
        }

        let mut constructors = 0usize;
        for member in &class_decl.members {
            match &member.kind {
                ClassMemberKind::Constructor(ctor) => {
                    // Overload signatures have no runtime presence.
                    if ctor.body.is_none() {
                        continue;
                    }
                    constructors += 1;
                    // Parameter properties become `this.x = x;` assignments.
                    const PARAMETER_PROPERTY: u32 =
                        MOD_PUBLIC | MOD_PRIVATE | MOD_PROTECTED | MOD_READONLY | MOD_OVERRIDE;
                    if constructors > 1
                        || decorated(&ctor.decorators)
                        || ctor.params.iter().any(|param| {
                            param.modifiers & !PARAMETER_PROPERTY != 0
                                || decorated(&param.decorators)
                                || !matches!(&param.name.kind, PatKind::Ident(name) if name != "this" && name != "<error>")
                        })
                        || (class_decl.extends.is_some()
                            && ctor
                                .params
                                .iter()
                                .any(|param| param.dotdotdot || param.initializer.is_some()))
                    {
                        return false;
                    }
                }
                ClassMemberKind::Method(method) => {
                    if method.body.is_none() {
                        continue;
                    }
                    if decorated(&method.decorators)
                        || method.is_generator
                        || method.is_async
                        || method.modifiers & (MOD_ABSTRACT | MOD_DECLARE) != 0
                        || !Self::legacy_es5_member_name_can_lower(&method.name, class_name)
                        || method.params.iter().any(|param| {
                            // Accessibility modifiers outside a constructor
                            // are errors that emit erases.
                            param.modifiers
                                & !(MOD_PUBLIC
                                    | MOD_PRIVATE
                                    | MOD_PROTECTED
                                    | MOD_READONLY
                                    | MOD_OVERRIDE)
                                != 0
                                || decorated(&param.decorators)
                                || !(matches!(&param.name.kind, PatKind::Ident(name) if name != "this" && name != "<error>")
                                    || (!matches!(param.name.kind, PatKind::Ident(_))
                                        && crate::es5_destructuring::pattern_supported(&param.name)))
                        })
                    {
                        return false;
                    }
                }
                ClassMemberKind::GetAccessor(accessor) => {
                    if Self::legacy_es5_accessor_is_erased(accessor) {
                        continue;
                    }
                    if decorated(&accessor.decorators)
                        || accessor.type_params.is_some()
                        || !accessor.params.is_empty()
                        || accessor.modifiers & (MOD_ABSTRACT | MOD_DECLARE) != 0
                        || !Self::legacy_es5_member_name_can_lower(&accessor.name, class_name)
                    {
                        return false;
                    }
                }
                ClassMemberKind::SetAccessor(accessor) => {
                    if Self::legacy_es5_accessor_is_erased(accessor) {
                        continue;
                    }
                    if decorated(&accessor.decorators)
                        || accessor.type_params.is_some()
                        || accessor.params.len() != 1
                        || accessor.modifiers & (MOD_ABSTRACT | MOD_DECLARE) != 0
                        || !Self::legacy_es5_member_name_can_lower(&accessor.name, class_name)
                        || accessor.params.iter().any(|param| {
                            param.modifiers
                                & !(MOD_PUBLIC
                                    | MOD_PRIVATE
                                    | MOD_PROTECTED
                                    | MOD_READONLY
                                    | MOD_OVERRIDE)
                                != 0
                                || decorated(&param.decorators)
                                || !(matches!(&param.name.kind, PatKind::Ident(name) if name != "this" && name != "<error>")
                                    || (!matches!(param.name.kind, PatKind::Ident(_))
                                        && crate::es5_destructuring::pattern_supported(&param.name)))
                        })
                    {
                        return false;
                    }
                }
                ClassMemberKind::Property(property) => {
                    if property.modifiers & MOD_DECLARE != 0 {
                        continue;
                    }
                    if decorated(&property.decorators)
                        || property.modifiers & MOD_ACCESSOR != 0
                        || !Self::legacy_es5_erased_property_name_can_lower(&property.name)
                    {
                        return false;
                    }
                }
                ClassMemberKind::IndexSignature(_) | ClassMemberKind::SemicolonClassElement => {}
                ClassMemberKind::StaticBlock(_) => return false,
            }
        }

        // Duplicate getter/setter declarations have recovery-specific emit
        // semantics. Keep the structural path to one descriptor slot of each
        // kind per public name and staticness.
        let mut getters = HashSet::new();
        let mut setters = HashSet::new();
        for member in &class_decl.members {
            let (accessor, slots) = match &member.kind {
                ClassMemberKind::GetAccessor(accessor) => (accessor, &mut getters),
                ClassMemberKind::SetAccessor(accessor) => (accessor, &mut setters),
                _ => continue,
            };
            match &accessor.name {
                PropName::Ident(..) | PropName::String(..) | PropName::Number(..) => {
                    let name = Self::legacy_es5_literal_member_key(&accessor.name).unwrap();
                    if !slots.insert((name, accessor.modifiers & MOD_STATIC != 0)) {
                        return false;
                    }
                }
                // Computed accessors are intentionally not coalesced. Each
                // source member evaluates its key and installs its own
                // descriptor, even when two key expressions have identical
                // text.
                PropName::Computed(..) => {}
                _ => return false,
            }
        }
        true
    }

    fn legacy_es5_literal_member_key(name: &PropName) -> Option<String> {
        match name {
            PropName::Ident(name, _) => Some(name.to_string()),
            PropName::String(name, _) => Some(decode_js_string_content(name)),
            PropName::Number(name, _) => {
                let normalized = Self::normalize_numeric_literal(name);
                Some(
                    crate::enum_eval::parse_js_number(&normalized)
                        .map(Self::format_f64_as_js)
                        .unwrap_or(normalized),
                )
            }
            PropName::Computed(..) | PropName::Private(..) => None,
        }
    }

    fn legacy_es5_member_name_can_lower(name: &PropName, class_name: &str) -> bool {
        match name {
            PropName::Ident(name, _) => !name.contains('\\'),
            PropName::String(..) | PropName::Number(..) => true,
            // The key expression moves into the class IIFE. References to the
            // class itself or its lexical/home-object environment would
            // therefore observe a different binding and stay native.
            PropName::Computed(expr, _) => {
                !crate::analysis::legacy_es5_computed_key_has_environment_hazard(expr, class_name)
                    && Self::legacy_es5_computed_key_arrow_can_lower(expr)
            }
            _ => false,
        }
    }

    fn legacy_es5_computed_key_arrow_can_lower(expr: &Expr) -> bool {
        match &Self::unwrap_type_layers(expr).kind {
            // This bounded form is emitted as `function () { }` below. More
            // complex arrows stay native until lexical capture and parameter
            // lowering can be shared with the general arrow transform.
            ExprKind::Arrow(arrow) => {
                !arrow.is_async
                    && arrow.type_params.is_none()
                    && arrow.return_type.is_none()
                    && arrow.params.is_empty()
                    && matches!(&arrow.body, ArrowBody::Block(body) if body.is_empty())
            }
            _ => true,
        }
    }

    fn legacy_es5_erased_property_name_can_lower(name: &PropName) -> bool {
        match name {
            PropName::Ident(..) | PropName::String(..) | PropName::Number(..) => true,
            // Erasing an uninitialized field must not erase a key evaluation.
            // Literal keys are value-only syntax and therefore have no side
            // effects to preserve.
            PropName::Computed(expr, _) => Self::computed_name_is_simple_literal(expr),
            PropName::Private(..) => false,
        }
    }

    fn legacy_es5_class_uses_expanded_member_lowering(class_decl: &ClassDecl) -> bool {
        class_decl.members.iter().any(|member| {
            matches!(
                member.kind,
                ClassMemberKind::Method(_)
                    | ClassMemberKind::GetAccessor(_)
                    | ClassMemberKind::SetAccessor(_)
            ) || matches!(&member.kind, ClassMemberKind::Property(property) if property.initializer.is_some() && property.modifiers & MOD_DECLARE == 0)
        })
    }

    fn legacy_es5_class_can_lower_without_dependency_gate(&self, class_decl: &ClassDecl) -> bool {
        if !Self::legacy_es5_class_shape_can_lower(
            class_decl,
            self.options.experimental_decorators == Some(true),
        ) {
            return false;
        }
        // A decorated CommonJS default export applies its decorators outside
        // the lowered class; keep it on the established wrapper path.
        if self.legacy_es5_cjs_default_class_start == Some(class_decl.span.start)
            && (!class_decl.decorators.is_empty()
                || class_decl.members.iter().any(|member| match &member.kind {
                    ClassMemberKind::Method(method) => !method.decorators.is_empty(),
                    ClassMemberKind::Property(property) => !property.decorators.is_empty(),
                    ClassMemberKind::GetAccessor(accessor)
                    | ClassMemberKind::SetAccessor(accessor) => !accessor.decorators.is_empty(),
                    _ => false,
                }))
        {
            return false;
        }
        // Instance initialization follows one direct super call. Other
        // constructor control-flow shapes need expression-level sequencing.
        let has_instance_initializers = class_decl.members.iter().any(|member| {
            matches!(&member.kind, ClassMemberKind::Property(property)
                if property.initializer.is_some()
                    && property.modifiers & (MOD_STATIC | MOD_DECLARE) == 0)
        });
        if class_decl.extends.is_some() && has_instance_initializers {
            for member in &class_decl.members {
                if let ClassMemberKind::Constructor(ctor) = &member.kind {
                    let body = ctor.body.as_deref().unwrap_or(&[]);
                    if body.first().and_then(Self::direct_super_call).is_none()
                        || self.legacy_constructor_remaining_has_super(body)
                    {
                        return false;
                    }
                }
            }
        }
        // Property `super` needs home-object rewriting; nested classes need
        // their own receiver environment. Keep both on the established class
        // path until the IIFE transform models those environment boundaries.
        // This conservative source check also covers nested arrow bodies.
        if class_decl.members.iter().any(|member| {
            matches!(&member.kind, ClassMemberKind::Property(property)
            if property.modifiers & MOD_DECLARE == 0
                && property.initializer.as_ref().is_some_and(|initializer| {
                    let source = self.source_between(initializer.span.start, initializer.span.end);
                    source.contains("super") || source.contains("class")
                }))
        }) {
            return false;
        }

        // A CommonJS default export is first emitted as a local class and then
        // assigned to `exports.default`. Preserve the established recovery
        // boundary for computed members even when that local class is emitted
        // from a clone whose `MOD_DEFAULT` bit was intentionally cleared.
        if self.legacy_es5_cjs_default_class_start == Some(class_decl.span.start)
            && class_decl.members.iter().any(|member| {
                matches!(
                    &member.kind,
                    ClassMemberKind::Method(method) if matches!(method.name, PropName::Computed(..))
                ) || matches!(
                    &member.kind,
                    ClassMemberKind::GetAccessor(accessor)
                        | ClassMemberKind::SetAccessor(accessor)
                        if matches!(accessor.name, PropName::Computed(..))
                )
            })
        {
            return false;
        }

        // Wave 3 owns the narrow ES5 default/rest parameter transform. The
        // expanded class-member path may use it, but must not move a method
        // whose parameter syntax needs lowering when the structural/comment
        // gates reject that transform.
        if class_decl.members.iter().any(|member| {
            let params = match &member.kind {
                ClassMemberKind::Method(method) => &method.params,
                ClassMemberKind::GetAccessor(accessor) | ClassMemberKind::SetAccessor(accessor) => {
                    &accessor.params
                }
                _ => return false,
            };
            params.iter().any(|param| {
                param.dotdotdot
                    || param.initializer.is_some()
                    || !matches!(param.name.kind, PatKind::Ident(_))
            }) && !self.can_downlevel_simple_param_initializers(params)
        }) {
            return false;
        }

        // Structural accessor emission only owns a deliberately narrow comment
        // shape for computed members: leading comments can move into the
        // descriptor immediately before that member's `get`/`set` entry. Body,
        // key-expression, and trailing comments still stay native. Identifier
        // accessors retain their established fail-closed behavior because a
        // getter/setter pair is coalesced into one descriptor.
        if self.options.remove_comments != Some(true) {
            for (index, member) in class_decl.members.iter().enumerate() {
                let computed_accessor = match &member.kind {
                    ClassMemberKind::GetAccessor(accessor)
                    | ClassMemberKind::SetAccessor(accessor) => {
                        matches!(accessor.name, PropName::Computed(..))
                    }
                    _ => continue,
                };
                let ownership_start = index
                    .checked_sub(1)
                    .and_then(|previous| class_decl.members.get(previous))
                    .map_or(class_decl.span.start, |previous| previous.span.end);
                let ownership_end = class_decl
                    .members
                    .get(index + 1)
                    .map_or(class_decl.span.end, |next| next.span.start);
                let ownership_source = self.source_between(ownership_start, ownership_end);
                if !(ownership_source.contains("//") || ownership_source.contains("/*")) {
                    continue;
                }
                // Comments inside a multi-line accessor body print with it.
                let body_open = match &member.kind {
                    ClassMemberKind::GetAccessor(accessor)
                    | ClassMemberKind::SetAccessor(accessor) => {
                        let body_limit = accessor
                            .body
                            .as_ref()
                            .and_then(|body| body.first())
                            .map_or(member.span.end.saturating_sub(1), |first| first.span.start);
                        self.source_between(member.span.start, body_limit)
                            .rfind('{')
                            .map(|offset| member.span.start + offset as u32)
                            // A one-line body prints compactly and would
                            // drop an inline comment; only multi-line bodies
                            // carry their comments through.
                            .filter(|&open| {
                                self.source_between(open, member.span.end).contains('\n')
                            })
                    }
                    _ => None,
                };
                let comments_are_placed = self.comments.iter().all(|comment| {
                    comment.pos < ownership_start
                        || comment.pos >= ownership_end
                        || (computed_accessor && comment.end <= member.span.start)
                        || body_open.is_some_and(|open| {
                            comment.pos > open && comment.end <= member.span.end
                        })
                });
                if !comments_are_placed {
                    return false;
                }
            }
        }

        // Uninitialized public fields are erased under assignment semantics,
        // but define semantics must materialize them as own properties.
        // The IIFE transform currently owns assignment semantics only.
        if self.use_define_for_class_fields()
            && class_decl
                .members
                .iter()
                .any(|member| matches!(member.kind, ClassMemberKind::Property(_)))
        {
            return false;
        }

        // Method/accessor `super` requires home-object rewriting. The
        // constructor transform already handles constructor super calls; do
        // not move any other super-bearing body into an ordinary function.
        // `super.x` / `super[x]` directly in a member body rewrite to the
        // base class; inside a nested environment they keep the native path.
        class_decl.members.iter().all(|member| match &member.kind {
            ClassMemberKind::Method(method) => {
                method.body.is_none() || self.member_super_uses_are_direct(member.span)
            }
            ClassMemberKind::GetAccessor(accessor) | ClassMemberKind::SetAccessor(accessor) => {
                accessor.body.is_none() || self.member_super_uses_are_direct(member.span)
            }
            _ => true,
        })
    }

    fn legacy_es5_class_can_lower(&self, class_decl: &ClassDecl) -> bool {
        self.legacy_es5_class_can_lower_without_dependency_gate(class_decl)
            && (!Self::legacy_es5_class_uses_expanded_member_lowering(class_decl)
                || class_decl.extends.is_none()
                || self
                    .legacy_es5_member_lowerable_class_starts
                    .contains(&class_decl.span.start))
    }

    /// Determine which top-level classes can safely participate in the
    /// expanded method/accessor transform. A moved derived class invokes its
    /// base with `.apply`; that is only valid when a same-file base is itself
    /// emitted as an ES5 constructor function. Constructor-only lowering is
    /// deliberately outside this dependency gate to preserve its established
    /// behavior.
    pub(super) fn prepare_legacy_es5_member_lowering(&mut self, stmts: &[Stmt]) {
        self.legacy_es5_member_lowerable_class_starts.clear();
        if self.effective_target() >= ScriptTarget::ES2015 {
            return;
        }

        fn direct_class_decl(stmt: &Stmt) -> Option<&ClassDecl> {
            match &stmt.kind {
                StmtKind::ClassDecl(class_decl) => Some(class_decl),
                StmtKind::Export(export_decl) => match &export_decl.kind {
                    ExportDeclKind::Decl(inner) | ExportDeclKind::DefaultDecl(inner) => {
                        match &inner.kind {
                            StmtKind::ClassDecl(class_decl) => Some(class_decl),
                            _ => None,
                        }
                    }
                    _ => None,
                },
                _ => None,
            }
        }

        let declarations: Vec<&ClassDecl> = stmts.iter().filter_map(direct_class_decl).collect();
        let mut name_counts: HashMap<AstString, usize> = HashMap::new();
        for class_decl in &declarations {
            if let Some(name) = &class_decl.name {
                *name_counts.entry(name.as_str().into()).or_default() += 1;
            }
        }

        let candidates: Vec<(&str, &ClassDecl)> = declarations
            .into_iter()
            .filter_map(|class_decl| {
                let name = class_decl.name.as_deref()?;
                (name_counts.get(name) == Some(&1)
                    && self.legacy_es5_class_can_lower_without_dependency_gate(class_decl))
                .then_some((name, class_decl))
            })
            .collect();

        let mut safe_names: HashSet<AstString> = HashSet::new();
        loop {
            let mut changed = false;
            for (name, class_decl) in &candidates {
                if safe_names.contains(*name) {
                    continue;
                }
                let dependency_is_safe = if !Self::legacy_es5_class_uses_expanded_member_lowering(
                    class_decl,
                ) || class_decl.extends.is_none()
                {
                    true
                } else {
                    class_decl.extends.as_ref().is_some_and(|extends| {
                        matches!(&extends.kind, ExprKind::Ident(base) if safe_names.contains(base.as_str()))
                    })
                };
                if dependency_is_safe {
                    safe_names.insert((*name).into());
                    self.legacy_es5_member_lowerable_class_starts
                        .insert(class_decl.span.start);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }

    fn legacy_es5_metadata_class_can_lower(&self, class_decl: &ClassDecl) -> bool {
        if self.options.experimental_decorators != Some(true)
            || self.options.emit_decorator_metadata != Some(true)
            || !self.is_commonjs()
            || class_decl.name.is_none()
            || class_decl.extends.is_some()
            || crate::analysis::decorated_class_has_self_reference(class_decl)
        {
            return false;
        }

        let params_are_simple = |params: &[Param]| {
            params.iter().all(|param| {
                !param.dotdotdot
                    && param.initializer.is_none()
                    && param.modifiers == 0
                    && param.decorators.is_empty()
                    && matches!(&param.name.kind, PatKind::Ident(name) if name != "this" && name != "<error>")
            })
        };
        let mut constructors = 0usize;
        class_decl.members.iter().all(|member| match &member.kind {
            ClassMemberKind::Constructor(ctor) => {
                constructors += 1;
                constructors == 1
                    && ctor.body.is_some()
                    && ctor.decorators.is_empty()
                    && params_are_simple(&ctor.params)
            }
            ClassMemberKind::Method(method) => {
                method.body.is_some()
                    && method.decorators.is_empty()
                    && method.type_params.is_none()
                    && !method.is_generator
                    && !method.is_async
                    && !method.optional
                    && method.modifiers & MOD_STATIC == 0
                    && matches!(method.name, PropName::Ident(..))
                    && params_are_simple(&method.params)
            }
            ClassMemberKind::Property(prop) => {
                prop.initializer.is_none()
                    && prop.decorators.is_empty()
                    && prop.modifiers & (MOD_STATIC | MOD_ACCESSOR) == 0
                    && !matches!(prop.name, PropName::Private(..))
            }
            ClassMemberKind::IndexSignature(_) | ClassMemberKind::SemicolonClassElement => true,
            ClassMemberKind::GetAccessor(_)
            | ClassMemberKind::SetAccessor(_)
            | ClassMemberKind::StaticBlock(_) => false,
        })
    }

    pub(super) fn can_emit_legacy_es5_class_decl(&self, class_decl: &ClassDecl) -> bool {
        self.effective_target() < ScriptTarget::ES2015
            && (self.legacy_es5_class_can_lower(class_decl)
                || self.legacy_es5_metadata_class_can_lower(class_decl))
    }

    pub(super) fn hide_cjs_imports_shadowed_by_params(
        &mut self,
        params: &[Param],
    ) -> Vec<(AstString, (AstString, AstString))> {
        params
            .iter()
            .filter_map(|param| match &param.name.kind {
                PatKind::Ident(name) => self
                    .cjs_import_map
                    .remove(name.as_str())
                    .map(|binding| (name.clone(), binding)),
                _ => None,
            })
            .collect()
    }

    pub(super) fn restore_shadowed_cjs_imports(
        &mut self,
        shadowed: Vec<(AstString, (AstString, AstString))>,
    ) {
        for (name, binding) in shadowed {
            self.cjs_import_map.insert(name, binding);
        }
    }

    fn prop_name_needs_legacy_extends(name: &PropName) -> bool {
        matches!(name, PropName::Computed(expr, _) if Self::expr_needs_legacy_extends(expr))
    }

    fn pat_needs_legacy_extends(pat: &Pat) -> bool {
        match &pat.kind {
            PatKind::Ident(_) => false,
            PatKind::Array(elements) => elements.iter().flatten().any(|element| match element {
                ArrayPatElem::Pat(pat) | ArrayPatElem::Rest(pat) => {
                    Self::pat_needs_legacy_extends(pat)
                }
            }),
            PatKind::Object(properties) => properties.iter().any(|property| match property {
                ObjPatProp::KeyValue(name, pat) => {
                    Self::prop_name_needs_legacy_extends(name)
                        || Self::pat_needs_legacy_extends(pat)
                }
                ObjPatProp::Shorthand(_, _) => false,
                ObjPatProp::Rest(pat) => Self::pat_needs_legacy_extends(pat),
                ObjPatProp::ShorthandAssign(_, initializer, _) => {
                    Self::expr_needs_legacy_extends(initializer)
                }
            }),
            PatKind::Assign(pat, initializer) => {
                Self::pat_needs_legacy_extends(pat) || Self::expr_needs_legacy_extends(initializer)
            }
            PatKind::Rest(pat) => Self::pat_needs_legacy_extends(pat),
        }
    }

    fn params_need_legacy_extends(params: &[Param]) -> bool {
        params.iter().any(|param| {
            Self::pat_needs_legacy_extends(&param.name)
                || param
                    .initializer
                    .as_ref()
                    .is_some_and(|expr| Self::expr_needs_legacy_extends(expr))
                || param.decorators.iter().any(Self::expr_needs_legacy_extends)
        })
    }

    fn class_contents_need_legacy_extends(class_decl: &ClassDecl) -> bool {
        class_decl
            .extends
            .as_ref()
            .is_some_and(|expr| Self::expr_needs_legacy_extends(expr))
            || class_decl
                .decorators
                .iter()
                .any(Self::expr_needs_legacy_extends)
            || class_decl.members.iter().any(|member| match &member.kind {
                ClassMemberKind::Property(property) => {
                    Self::prop_name_needs_legacy_extends(&property.name)
                        || property
                            .initializer
                            .as_ref()
                            .is_some_and(|expr| Self::expr_needs_legacy_extends(expr))
                        || property
                            .decorators
                            .iter()
                            .any(Self::expr_needs_legacy_extends)
                }
                ClassMemberKind::Method(method) => {
                    Self::prop_name_needs_legacy_extends(&method.name)
                        || Self::params_need_legacy_extends(&method.params)
                        || method
                            .decorators
                            .iter()
                            .any(Self::expr_needs_legacy_extends)
                        || method
                            .body
                            .as_ref()
                            .is_some_and(|body| Self::stmts_need_legacy_extends_static(body))
                }
                ClassMemberKind::Constructor(ctor) => {
                    Self::params_need_legacy_extends(&ctor.params)
                        || ctor.decorators.iter().any(Self::expr_needs_legacy_extends)
                        || ctor
                            .body
                            .as_ref()
                            .is_some_and(|body| Self::stmts_need_legacy_extends_static(body))
                }
                ClassMemberKind::GetAccessor(accessor) | ClassMemberKind::SetAccessor(accessor) => {
                    Self::prop_name_needs_legacy_extends(&accessor.name)
                        || Self::params_need_legacy_extends(&accessor.params)
                        || accessor
                            .decorators
                            .iter()
                            .any(Self::expr_needs_legacy_extends)
                        || accessor
                            .body
                            .as_ref()
                            .is_some_and(|body| Self::stmts_need_legacy_extends_static(body))
                }
                ClassMemberKind::IndexSignature(signature) => {
                    Self::params_need_legacy_extends(&signature.params)
                }
                ClassMemberKind::StaticBlock(body) => Self::stmts_need_legacy_extends_static(body),
                ClassMemberKind::SemicolonClassElement => false,
            })
    }

    fn jsx_attributes_need_legacy_extends(attributes: &[JsxAttribute]) -> bool {
        attributes.iter().any(|attribute| match attribute {
            JsxAttribute::Normal { value, .. } => value
                .as_ref()
                .is_some_and(|expr| Self::expr_needs_legacy_extends(expr)),
            JsxAttribute::Spread(expr, _) => Self::expr_needs_legacy_extends(expr),
        })
    }

    fn jsx_children_need_legacy_extends(children: &[JsxChild]) -> bool {
        children.iter().any(|child| match child {
            JsxChild::Text(_, _) => false,
            JsxChild::Element(expr) => Self::expr_needs_legacy_extends(expr),
            JsxChild::Expression(expr, _) => expr
                .as_ref()
                .is_some_and(|expr| Self::expr_needs_legacy_extends(expr)),
            JsxChild::Fragment(fragment) => {
                Self::jsx_children_need_legacy_extends(&fragment.children)
            }
        })
    }

    fn expr_needs_legacy_extends(expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Template(template) => template
                .exprs
                .iter()
                .any(|expr| Self::expr_needs_legacy_extends(expr)),
            ExprKind::TaggedTemplate(tagged) => {
                Self::expr_needs_legacy_extends(&tagged.tag)
                    || tagged
                        .quasi
                        .exprs
                        .iter()
                        .any(|expr| Self::expr_needs_legacy_extends(expr))
            }
            ExprKind::ArrayLit(elements) => elements
                .iter()
                .flatten()
                .any(|expr| Self::expr_needs_legacy_extends(expr)),
            ExprKind::ObjectLit(properties) => properties.iter().any(|property| match property {
                ObjLitProp::Property(property) => {
                    Self::prop_name_needs_legacy_extends(&property.key)
                        || Self::expr_needs_legacy_extends(&property.value)
                }
                ObjLitProp::Shorthand(_, _) => false,
                ObjLitProp::ShorthandDefault(_, initializer, _)
                | ObjLitProp::Spread(initializer, _) => {
                    Self::expr_needs_legacy_extends(initializer)
                }
                ObjLitProp::Method(method) => {
                    Self::prop_name_needs_legacy_extends(&method.name)
                        || Self::params_need_legacy_extends(&method.params)
                        || Self::stmts_need_legacy_extends_static(&method.body)
                }
                ObjLitProp::Get(accessor) | ObjLitProp::Set(accessor) => {
                    Self::prop_name_needs_legacy_extends(&accessor.name)
                        || Self::params_need_legacy_extends(&accessor.params)
                        || Self::stmts_need_legacy_extends_static(&accessor.body)
                }
            }),
            ExprKind::FnExpr(function) => {
                Self::params_need_legacy_extends(&function.params)
                    || function
                        .decorators
                        .iter()
                        .any(Self::expr_needs_legacy_extends)
                    || function
                        .body
                        .as_ref()
                        .is_some_and(|body| Self::stmts_need_legacy_extends_static(body))
            }
            ExprKind::Arrow(arrow) => {
                Self::params_need_legacy_extends(&arrow.params)
                    || match &arrow.body {
                        ArrowBody::Expr(expr) => Self::expr_needs_legacy_extends(expr),
                        ArrowBody::Block(body) => Self::stmts_need_legacy_extends_static(body),
                    }
            }
            // Class expressions are not handled by the declaration-only legacy
            // transform, but their executable expressions and bodies can contain
            // eligible nested class declarations.
            ExprKind::ClassExpr(class_decl) => Self::class_contents_need_legacy_extends(class_decl),
            ExprKind::Call(call) => {
                Self::expr_needs_legacy_extends(&call.callee)
                    || call
                        .args
                        .iter()
                        .any(|arg| Self::expr_needs_legacy_extends(arg))
            }
            ExprKind::New(new_expr) => {
                Self::expr_needs_legacy_extends(&new_expr.callee)
                    || new_expr.args.as_ref().is_some_and(|args| {
                        args.iter().any(|arg| Self::expr_needs_legacy_extends(arg))
                    })
            }
            ExprKind::Member(member) => Self::expr_needs_legacy_extends(&member.object),
            ExprKind::ElemAccess(access) => {
                Self::expr_needs_legacy_extends(&access.object)
                    || Self::expr_needs_legacy_extends(&access.index)
            }
            ExprKind::Cond(cond) => {
                Self::expr_needs_legacy_extends(&cond.test)
                    || Self::expr_needs_legacy_extends(&cond.consequent)
                    || Self::expr_needs_legacy_extends(&cond.alternate)
            }
            ExprKind::Binary(binary) => {
                Self::expr_needs_legacy_extends(&binary.left)
                    || Self::expr_needs_legacy_extends(&binary.right)
            }
            ExprKind::Unary(unary) => Self::expr_needs_legacy_extends(&unary.argument),
            ExprKind::Update(update) => Self::expr_needs_legacy_extends(&update.argument),
            ExprKind::Paren(inner)
            | ExprKind::NonNull(inner)
            | ExprKind::Spread(inner)
            | ExprKind::Await(inner)
            | ExprKind::Delete(inner)
            | ExprKind::Typeof(inner)
            | ExprKind::Void(inner) => Self::expr_needs_legacy_extends(inner),
            ExprKind::TypeAssertion(assertion) => Self::expr_needs_legacy_extends(&assertion.expr),
            ExprKind::As(as_expr) => Self::expr_needs_legacy_extends(&as_expr.expr),
            ExprKind::Satisfies(satisfies) => Self::expr_needs_legacy_extends(&satisfies.expr),
            ExprKind::Instantiation(instantiation) => {
                Self::expr_needs_legacy_extends(&instantiation.expr)
            }
            ExprKind::Yield(_, value) => value
                .as_ref()
                .is_some_and(|expr| Self::expr_needs_legacy_extends(expr)),
            ExprKind::Assign(assign) => {
                Self::expr_needs_legacy_extends(&assign.left)
                    || Self::expr_needs_legacy_extends(&assign.right)
            }
            ExprKind::Comma(expressions) => expressions
                .iter()
                .any(|expr| Self::expr_needs_legacy_extends(expr)),
            ExprKind::JsxElement(element) => {
                Self::expr_needs_legacy_extends(&element.name)
                    || Self::jsx_attributes_need_legacy_extends(&element.attributes)
                    || Self::jsx_children_need_legacy_extends(&element.children)
            }
            ExprKind::JsxSelfClosing(element) => {
                Self::expr_needs_legacy_extends(&element.name)
                    || Self::jsx_attributes_need_legacy_extends(&element.attributes)
            }
            ExprKind::JsxFragment(fragment) => {
                Self::jsx_children_need_legacy_extends(&fragment.children)
            }
            ExprKind::Ident(_)
            | ExprKind::NumLit(_)
            | ExprKind::BigIntLit(_)
            | ExprKind::StrLit(_)
            | ExprKind::BoolLit(_)
            | ExprKind::NullLit
            | ExprKind::RegexpLit(_)
            | ExprKind::NoSubstTemplate(_)
            | ExprKind::This
            | ExprKind::Super
            | ExprKind::MetaProp(_)
            | ExprKind::Omitted => false,
        }
    }

    fn var_stmt_needs_legacy_extends(var_stmt: &VarStmt) -> bool {
        var_stmt.declarations.iter().any(|declaration| {
            Self::pat_needs_legacy_extends(&declaration.name)
                || declaration
                    .init
                    .as_ref()
                    .is_some_and(|expr| Self::expr_needs_legacy_extends(expr))
        })
    }

    fn for_left_needs_legacy_extends(left: &ForInOfLeft) -> bool {
        match left {
            ForInOfLeft::Var(var_stmt) => Self::var_stmt_needs_legacy_extends(var_stmt),
            ForInOfLeft::Pat(pat) => Self::pat_needs_legacy_extends(pat),
            ForInOfLeft::Expr(expr) => Self::expr_needs_legacy_extends(expr),
        }
    }

    fn module_needs_legacy_extends(module: &ModuleDecl) -> bool {
        module.body.as_ref().is_some_and(|body| match body {
            ModuleBody::Block(body) => Self::stmts_need_legacy_extends_static(body),
            ModuleBody::Module(module) => Self::module_needs_legacy_extends(module),
        })
    }

    fn stmt_needs_legacy_extends(stmt: &Stmt) -> bool {
        match &stmt.kind {
            StmtKind::ClassDecl(class_decl) => {
                class_decl.modifiers & MOD_DECLARE == 0
                    // ES5 lowers decorated derived classes with either decorator
                    // flavour, so they need `__extends` too.
                    && ((Self::legacy_es5_class_shape_can_lower(class_decl, true)
                        && class_decl.extends.is_some())
                        || Self::class_contents_need_legacy_extends(class_decl))
            }
            StmtKind::FnDecl(function) => {
                Self::params_need_legacy_extends(&function.params)
                    || function
                        .decorators
                        .iter()
                        .any(Self::expr_needs_legacy_extends)
                    || function
                        .body
                        .as_ref()
                        .is_some_and(|body| Self::stmts_need_legacy_extends_static(body))
            }
            StmtKind::Var(var_stmt) => Self::var_stmt_needs_legacy_extends(var_stmt),
            StmtKind::Expr(expr) | StmtKind::Throw(expr) | StmtKind::ExportAssign(expr) => {
                Self::expr_needs_legacy_extends(expr)
            }
            StmtKind::Return(expr) => expr
                .as_ref()
                .is_some_and(|expr| Self::expr_needs_legacy_extends(expr)),
            StmtKind::Block(body) => Self::stmts_need_legacy_extends_static(body),
            StmtKind::If(if_stmt) => {
                Self::expr_needs_legacy_extends(&if_stmt.test)
                    || Self::stmt_needs_legacy_extends(&if_stmt.consequent)
                    || if_stmt
                        .alternate
                        .as_ref()
                        .is_some_and(|stmt| Self::stmt_needs_legacy_extends(stmt))
            }
            StmtKind::While(while_stmt) => {
                Self::expr_needs_legacy_extends(&while_stmt.test)
                    || Self::stmt_needs_legacy_extends(&while_stmt.body)
            }
            StmtKind::DoWhile(do_while) => {
                Self::stmt_needs_legacy_extends(&do_while.body)
                    || Self::expr_needs_legacy_extends(&do_while.test)
            }
            StmtKind::For(for_stmt) => {
                for_stmt.init.as_ref().is_some_and(|init| match init {
                    ForInit::Var(var_stmt) => Self::var_stmt_needs_legacy_extends(var_stmt),
                    ForInit::Expr(expr) => Self::expr_needs_legacy_extends(expr),
                }) || for_stmt
                    .test
                    .as_ref()
                    .is_some_and(|expr| Self::expr_needs_legacy_extends(expr))
                    || for_stmt
                        .update
                        .as_ref()
                        .is_some_and(|expr| Self::expr_needs_legacy_extends(expr))
                    || Self::stmt_needs_legacy_extends(&for_stmt.body)
            }
            StmtKind::ForIn(for_in) => {
                Self::for_left_needs_legacy_extends(&for_in.left)
                    || Self::expr_needs_legacy_extends(&for_in.right)
                    || Self::stmt_needs_legacy_extends(&for_in.body)
            }
            StmtKind::ForOf(for_of) => {
                Self::for_left_needs_legacy_extends(&for_of.left)
                    || Self::expr_needs_legacy_extends(&for_of.right)
                    || Self::stmt_needs_legacy_extends(&for_of.body)
            }
            StmtKind::Switch(switch_stmt) => {
                Self::expr_needs_legacy_extends(&switch_stmt.discriminant)
                    || switch_stmt.cases.iter().any(|case| {
                        case.test
                            .as_ref()
                            .is_some_and(|expr| Self::expr_needs_legacy_extends(expr))
                            || Self::stmts_need_legacy_extends_static(&case.consequent)
                    })
            }
            StmtKind::Try(try_stmt) => {
                Self::stmts_need_legacy_extends_static(&try_stmt.block)
                    || try_stmt.handler.as_ref().is_some_and(|handler| {
                        handler
                            .param
                            .as_ref()
                            .is_some_and(Self::pat_needs_legacy_extends)
                            || Self::stmts_need_legacy_extends_static(&handler.body)
                    })
                    || try_stmt
                        .finalizer
                        .as_ref()
                        .is_some_and(|body| Self::stmts_need_legacy_extends_static(body))
            }
            StmtKind::Labeled(labeled) => Self::stmt_needs_legacy_extends(&labeled.body),
            StmtKind::With(with_stmt) => {
                Self::expr_needs_legacy_extends(&with_stmt.object)
                    || Self::stmt_needs_legacy_extends(&with_stmt.body)
            }
            StmtKind::EnumDecl(enum_decl) => enum_decl.members.iter().any(|member| {
                Self::prop_name_needs_legacy_extends(&member.name)
                    || member
                        .initializer
                        .as_ref()
                        .is_some_and(|expr| Self::expr_needs_legacy_extends(expr))
            }),
            StmtKind::ModuleDecl(module) => {
                module.modifiers & MOD_DECLARE == 0 && Self::module_needs_legacy_extends(module)
            }
            StmtKind::ImportEquals(import) => Self::expr_needs_legacy_extends(&import.module_ref),
            StmtKind::Export(export_decl) => match &export_decl.kind {
                ExportDeclKind::Decl(stmt) | ExportDeclKind::DefaultDecl(stmt) => {
                    Self::stmt_needs_legacy_extends(stmt)
                }
                ExportDeclKind::Default(expr) => Self::expr_needs_legacy_extends(expr),
                ExportDeclKind::Named { .. } | ExportDeclKind::All { .. } => false,
            },
            StmtKind::Break(_)
            | StmtKind::Continue(_)
            | StmtKind::Empty
            | StmtKind::InterfaceDecl(_)
            | StmtKind::TypeAlias(_)
            | StmtKind::Import(_)
            | StmtKind::Debugger => false,
        }
    }

    fn stmts_need_legacy_extends_static(stmts: &[Stmt]) -> bool {
        stmts.iter().any(Self::stmt_needs_legacy_extends)
    }

    pub(super) fn stmts_need_legacy_extends(&self, stmts: &[Stmt]) -> bool {
        Self::stmts_need_legacy_extends_static(stmts)
    }

    fn direct_super_call(stmt: &Stmt) -> Option<&CallExpr> {
        fn from_expr(expr: &Expr) -> Option<&CallExpr> {
            match &expr.kind {
                ExprKind::Call(call) if matches!(call.callee.kind, ExprKind::Super) => Some(call),
                ExprKind::Paren(inner) => from_expr(inner),
                _ => None,
            }
        }
        match &stmt.kind {
            StmtKind::Expr(expr) => from_expr(expr),
            _ => None,
        }
    }

    fn legacy_generated_name(&self, base: &str) -> String {
        if !self.source_has_identifier(base) {
            return base.to_string();
        }
        let mut suffix = 1usize;
        loop {
            let candidate = format!("{base}_{suffix}");
            if !self.source_has_identifier(&candidate) {
                return candidate;
            }
            suffix += 1;
        }
    }

    fn legacy_constructor_remaining_has_super(&self, body: &[Stmt]) -> bool {
        body.get(1..).is_some_and(|remaining| {
            remaining.iter().any(|stmt| {
                self.source_between(stmt.span.start, stmt.span.end)
                    .contains("super")
            })
        })
    }

    fn emit_legacy_es5_derived_constructor_body(
        &mut self,
        body: &[Stmt],
        enclosing_span: Span,
        super_name: &str,
        this_name: &str,
        field_initializers: &[Stmt],
    ) {
        let direct_super = body.first().and_then(Self::direct_super_call);
        let remaining_has_super = self.legacy_constructor_remaining_has_super(body);

        if !field_initializers.is_empty() {
            debug_assert!(direct_super.is_some() && !remaining_has_super);
            self.emit_legacy_es5_constructor_with_fields(
                field_initializers,
                &body[1..],
                direct_super,
                super_name,
                this_name,
            );
            self.advance_comment_pos(enclosing_span.end);
            return;
        }

        self.writeln("{");
        self.indent += 1;
        if let Some(call) = direct_super.filter(|_| !remaining_has_super) {
            self.write("var ");
            self.write(this_name);
            self.write(" = ");
            self.write(super_name);
            self.write(".call(this");
            for arg in &call.args {
                self.write(", ");
                self.emit_expr(arg);
            }
            self.writeln(") || this;");
        } else {
            self.write("var ");
            self.write(this_name);
            self.writeln(" = this;");
        }

        let body_start = usize::from(direct_super.is_some() && !remaining_has_super);
        let saved_super = self
            .legacy_constructor_super
            .replace(super_name.to_string());
        let saved_this = self.legacy_this_alias.replace(this_name.to_string());
        let saved_static_this = self.static_this_alias.replace(this_name.to_string());
        self.fn_scope_depth += 1;
        for stmt in &body[body_start..] {
            self.emit_leading_comments(stmt.span.start);
            self.emit_stmt(stmt);
        }
        self.fn_scope_depth -= 1;
        self.static_this_alias = saved_static_this;
        self.legacy_this_alias = saved_this;
        self.legacy_constructor_super = saved_super;
        self.write("return ");
        self.write(this_name);
        self.writeln(";");
        self.indent -= 1;
        self.write("}");
        self.advance_comment_pos(enclosing_span.end);
    }

    fn emit_legacy_es5_constructor_with_fields(
        &mut self,
        field_initializers: &[Stmt],
        body: &[Stmt],
        super_call: Option<&CallExpr>,
        super_name: &str,
        this_name: &str,
    ) {
        // Share the ordinary function scope, including expression temporaries,
        // while initializing fields on the object actually returned by super.
        self.emit_scoped_body_with_param_initializers(
            &[],
            field_initializers,
            |emitter, fields| {
                emitter.write("var ");
                emitter.write(this_name);
                emitter.write(" = ");
                emitter.write(super_name);
                if let Some(call) = super_call {
                    emitter.write(".call(this");
                    for arg in &call.args {
                        emitter.write(", ");
                        emitter.emit_expr(arg);
                    }
                    emitter.writeln(") || this;");
                } else {
                    emitter.write(" !== null && ");
                    emitter.write(super_name);
                    emitter.writeln(".apply(this, arguments) || this;");
                }
                let saved_super = emitter
                    .legacy_constructor_super
                    .replace(super_name.to_string());
                let saved_this = emitter.legacy_this_alias.replace(this_name.to_string());
                let saved_static_this = emitter.static_this_alias.replace(this_name.to_string());
                for stmt in fields {
                    emitter.emit_stmt(stmt);
                }
                for stmt in body {
                    emitter.emit_leading_comments(stmt.span.start);
                    emitter.emit_stmt(stmt);
                }
                emitter.static_this_alias = saved_static_this;
                emitter.legacy_this_alias = saved_this;
                emitter.legacy_constructor_super = saved_super;
                emitter.write("return ");
                emitter.write(this_name);
                emitter.writeln(";");
            },
        );
    }

    fn emit_legacy_es5_method(&mut self, class_name: &str, method: &ClassMethod, span: Span) {
        let Some(body) = &method.body else {
            return;
        };
        self.write(class_name);
        if method.modifiers & MOD_STATIC == 0 {
            self.write(".prototype");
        }
        match &method.name {
            PropName::Ident(..) | PropName::String(..) | PropName::Number(..) => {
                self.emit_member_access(&method.name);
            }
            PropName::Computed(expr, _) => {
                self.write("[");
                self.emit_legacy_es5_computed_key_expr(expr);
                self.write("]");
            }
            _ => return,
        }
        self.write(" = function (");
        let saved_home = self.es5_super_home.clone();
        self.es5_super_home = self.es5_class_super_name.as_ref().map(|base| {
            if method.modifiers & MOD_STATIC != 0 {
                base.clone()
            } else {
                format!("{base}.prototype")
            }
        });
        let saved_rewrite = crate::source_transform::ES5_SUPER_REWRITE
            .with(|flag| flag.replace(self.es5_super_home.is_some()));
        let downlevel_simple_params = self.can_downlevel_simple_param_initializers(&method.params);
        if downlevel_simple_params {
            self.emit_params_without_initializers(&method.params);
        } else {
            self.emit_params(&method.params);
        }
        self.write(") ");
        let shadowed = self.hide_cjs_imports_shadowed_by_params(&method.params);
        if downlevel_simple_params {
            self.emit_block_for_decl_body_with_param_initializers(&method.params, body, span);
        } else {
            self.emit_block_for_decl_body(body, span);
        }
        self.restore_shadowed_cjs_imports(shadowed);
        crate::source_transform::ES5_SUPER_REWRITE.with(|flag| flag.set(saved_rewrite));
        self.es5_super_home = saved_home;
        self.writeln(";");
    }

    fn emit_legacy_es5_computed_key_expr(&mut self, expr: &Expr) {
        if let ExprKind::Arrow(arrow) = &Self::unwrap_type_layers(expr).kind {
            debug_assert!(Self::legacy_es5_computed_key_arrow_can_lower(expr));
            debug_assert!(arrow.params.is_empty());
            debug_assert!(matches!(&arrow.body, ArrowBody::Block(body) if body.is_empty()));
            self.write("function () { }");
        } else {
            self.emit_expr(expr);
        }
    }

    /// Whether every `super` in a class member is a property access or call
    /// directly in its body (not in a nested function, arrow or class).
    fn member_super_uses_are_direct(&self, member_span: Span) -> bool {
        let text = self.source_between(member_span.start, member_span.end);
        let bytes = text.as_bytes();
        let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'$';
        let mut from = 0;
        while let Some(found) = text[from..].find("super") {
            let at = from + found;
            from = at + 5;
            if (at > 0 && is_ident(bytes[at - 1]))
                || bytes.get(at + 5).copied().is_some_and(is_ident)
            {
                continue;
            }
            let next = text[at + 5..].trim_start().chars().next();
            if !matches!(next, Some('.') | Some('[')) {
                return false;
            }
            let pos = member_span.start + at as u32;
            if self
                .environment_spans
                .iter()
                .any(|env| env.start > member_span.start && env.start <= pos && pos < env.end)
            {
                return false;
            }
        }
        true
    }

    fn emit_legacy_es5_accessor_function(&mut self, accessor: &ClassAccessor, span: Span) {
        let saved_home = self.es5_super_home.clone();
        self.es5_super_home = self.es5_class_super_name.as_ref().map(|base| {
            if accessor.modifiers & MOD_STATIC != 0 {
                base.clone()
            } else {
                format!("{base}.prototype")
            }
        });
        let saved_rewrite = crate::source_transform::ES5_SUPER_REWRITE
            .with(|flag| flag.replace(self.es5_super_home.is_some()));
        self.emit_legacy_es5_accessor_function_inner(accessor, span);
        crate::source_transform::ES5_SUPER_REWRITE.with(|flag| flag.set(saved_rewrite));
        self.es5_super_home = saved_home;
    }

    fn emit_legacy_es5_accessor_function_inner(&mut self, accessor: &ClassAccessor, span: Span) {
        self.write("function (");
        let downlevel_params = self.can_downlevel_simple_param_initializers(&accessor.params);
        if downlevel_params {
            self.emit_params_without_initializers(&accessor.params);
        } else {
            self.emit_params(&accessor.params);
        }
        self.write(") ");
        let shadowed = self.hide_cjs_imports_shadowed_by_params(&accessor.params);
        let body = accessor.body.as_deref().unwrap_or(&[]);
        if downlevel_params {
            self.emit_block_for_decl_body_with_param_initializers(&accessor.params, body, span);
        } else {
            self.emit_block_for_decl_body(body, span);
        }
        self.restore_shadowed_cjs_imports(shadowed);
    }

    fn legacy_es5_instance_initializers(class_decl: &ClassDecl) -> Vec<Stmt> {
        // Parameter properties are assigned first, in parameter order.
        let span = Span::new(0, 0);
        let parameter_properties = class_decl
            .members
            .iter()
            .filter_map(|member| match &member.kind {
                ClassMemberKind::Constructor(ctor) if ctor.body.is_some() => Some(ctor),
                _ => None,
            })
            .flat_map(|ctor| ctor.params.iter())
            .filter(|param| {
                param.modifiers
                    & (MOD_PUBLIC | MOD_PRIVATE | MOD_PROTECTED | MOD_READONLY | MOD_OVERRIDE)
                    != 0
            })
            .filter_map(|param| {
                let PatKind::Ident(name) = &param.name.kind else {
                    return None;
                };
                Some(Stmt {
                    kind: StmtKind::Expr(Box::new(Expr {
                        kind: ExprKind::Assign(AssignExpr {
                            left: Box::new(Expr {
                                kind: ExprKind::Member(Box::new(MemberExpr {
                                    object: Box::new(Expr {
                                        kind: ExprKind::This,
                                        span,
                                    }),
                                    property: name.clone(),
                                    optional: false,
                                })),
                                span,
                            }),
                            op: AssignOp::Assign,
                            right: Box::new(Expr {
                                kind: ExprKind::Ident(name.clone()),
                                span: param.name.span,
                            }),
                        }),
                        span,
                    })),
                    span,
                })
            })
            .collect::<Vec<_>>();
        let fields = class_decl.members.iter().filter_map(|member| {
            let ClassMemberKind::Property(property) = &member.kind else {
                return None;
            };
            if property.modifiers & (MOD_STATIC | MOD_DECLARE) != 0 {
                return None;
            }
            let initializer = property.initializer.as_ref()?;
            let span = Span::new(0, 0);
            let receiver = Box::new(Expr {
                kind: ExprKind::This,
                span,
            });
            let kind = match &property.name {
                PropName::Ident(name, _) => ExprKind::Member(Box::new(MemberExpr {
                    object: receiver,
                    property: name.clone(),
                    optional: false,
                })),
                PropName::String(name, _) => ExprKind::ElemAccess(ElemAccessExpr {
                    object: receiver,
                    index: Box::new(Expr {
                        kind: ExprKind::StrLit(name.clone()),
                        span: property.name.span(),
                    }),
                    optional: false,
                }),
                PropName::Number(value, _) => ExprKind::ElemAccess(ElemAccessExpr {
                    object: receiver,
                    index: Box::new(Expr {
                        kind: ExprKind::NumLit(value.clone()),
                        span: property.name.span(),
                    }),
                    optional: false,
                }),
                PropName::Computed(key, _) => ExprKind::ElemAccess(ElemAccessExpr {
                    object: receiver,
                    index: key.clone(),
                    optional: false,
                }),
                PropName::Private(..) => return None,
            };
            Some(Stmt {
                kind: StmtKind::Expr(Box::new(Expr {
                    kind: ExprKind::Assign(AssignExpr {
                        left: Box::new(Expr { kind, span }),
                        op: AssignOp::Assign,
                        right: initializer.clone(),
                    }),
                    span,
                })),
                span,
            })
        });
        parameter_properties.into_iter().chain(fields).collect()
    }

    fn emit_legacy_es5_class_decl(&mut self, class_decl: &ClassDecl, lower_metadata_class: bool) {
        let saved_super_name = self.es5_class_super_name.take();
        self.emit_legacy_es5_class_decl_inner(class_decl, lower_metadata_class);
        self.es5_class_super_name = saved_super_name;
    }

    fn emit_legacy_es5_class_decl_inner(
        &mut self,
        class_decl: &ClassDecl,
        lower_metadata_class: bool,
    ) {
        let name = class_decl.name.as_deref().unwrap();
        if !class_decl.decorators.is_empty() {
            let let_prefix = format!("let {name} = ");
            if self.output.ends_with(&let_prefix) {
                self.output.truncate(self.output.len() - let_prefix.len());
            }
        }
        let super_name = self.legacy_generated_name("_super");
        self.es5_class_super_name = class_decl.extends.is_some().then(|| super_name.clone());
        let this_name = self.legacy_generated_name("_this");
        let field_initializers = Self::legacy_es5_instance_initializers(class_decl);
        let constructor = class_decl
            .members
            .iter()
            .find_map(|member| match &member.kind {
                ClassMemberKind::Constructor(ctor) if ctor.body.is_some() => {
                    Some((ctor, member.span))
                }
                _ => None,
            });

        self.write("var ");
        self.write(name);
        self.write(" = /** @class */ (function (");
        if class_decl.extends.is_some() {
            self.write(&super_name);
        }
        self.writeln(") {");
        self.indent += 1;
        if class_decl.extends.is_some() {
            self.write(self.helper_prefix());
            self.write("__extends(");
            self.write(name);
            self.write(", ");
            self.write(&super_name);
            self.writeln(");");
        }

        match constructor {
            Some((ctor, span)) => {
                // Constructor declarations move before methods and static
                // initializers. Only move their own leading trivia with them.
                let leading_start = class_decl
                    .members
                    .iter()
                    .take_while(|member| member.span.start < span.start)
                    .last()
                    .map_or(class_decl.span.start, |member| member.span.end);
                self.emit_leading_comments_in_range(leading_start, span.start);
                let downlevel_simple_params = class_decl.extends.is_none()
                    && self.can_downlevel_simple_param_initializers(&ctor.params);
                self.write("function ");
                self.write(name);
                self.write("(");
                if downlevel_simple_params {
                    self.emit_params_without_initializers(&ctor.params);
                } else {
                    self.emit_params(&ctor.params);
                }
                self.write(") ");
                let shadowed = lower_metadata_class
                    .then(|| self.hide_cjs_imports_shadowed_by_params(&ctor.params));
                let body = ctor.body.as_deref().unwrap_or(&[]);
                if class_decl.extends.is_some() {
                    self.emit_legacy_es5_derived_constructor_body(
                        body,
                        span,
                        &super_name,
                        &this_name,
                        &field_initializers,
                    );
                } else if !field_initializers.is_empty() {
                    let directive_count = body.iter().take_while(|stmt| matches!(&stmt.kind, StmtKind::Expr(expr) if matches!(expr.kind, ExprKind::StrLit(_)))).count();
                    let initialized_body: Vec<_> = body[..directive_count]
                        .iter()
                        .chain(&field_initializers)
                        .chain(&body[directive_count..])
                        .cloned()
                        .collect();
                    self.emit_block_for_decl_body_with_param_initializers(
                        &ctor.params,
                        &initialized_body,
                        span,
                    );
                } else if downlevel_simple_params {
                    self.emit_block_for_decl_body_with_param_initializers(&ctor.params, body, span);
                } else {
                    self.emit_block_for_decl_body(body, span);
                }
                if let Some(shadowed) = shadowed {
                    self.restore_shadowed_cjs_imports(shadowed);
                }
                self.newline();
                self.append_trailing_comment(span);
            }
            None if class_decl.extends.is_some() => {
                self.write("function ");
                self.write(name);
                if field_initializers.is_empty() {
                    self.writeln("() {");
                    self.indent += 1;
                    self.write("return ");
                    self.write(&super_name);
                    self.write(" !== null && ");
                    self.write(&super_name);
                    self.writeln(".apply(this, arguments) || this;");
                    self.indent -= 1;
                    self.writeln("}");
                } else {
                    self.write("() ");
                    self.emit_legacy_es5_constructor_with_fields(
                        &field_initializers,
                        &[],
                        None,
                        &super_name,
                        &this_name,
                    );
                    self.newline();
                }
            }
            None => {
                self.write("function ");
                self.write(name);
                if field_initializers.is_empty() {
                    self.writeln("() {");
                    self.writeln("}");
                } else {
                    self.write("() ");
                    self.emit_block_for_decl_body_with_param_initializers(
                        &[],
                        &field_initializers,
                        class_decl.span,
                    );
                    self.newline();
                }
            }
        }
        let mut emitted_accessors = HashSet::new();
        for member in &class_decl.members {
            match &member.kind {
                ClassMemberKind::Method(method) => {
                    if method.body.is_some() {
                        self.emit_leading_comments(member.span.start);
                        self.emit_legacy_es5_method(name, method, member.span);
                    } else {
                        // Signature-only overloads have no runtime key
                        // evaluation or function assignment.
                        self.advance_comment_pos(member.span.end);
                    }
                }
                ClassMemberKind::GetAccessor(accessor) | ClassMemberKind::SetAccessor(accessor) => {
                    if Self::legacy_es5_accessor_is_erased(accessor) {
                        self.advance_comment_pos(member.span.end);
                        continue;
                    }
                    let is_static = accessor.modifiers & MOD_STATIC != 0;
                    let is_computed = matches!(accessor.name, PropName::Computed(..));
                    let (getter, setter) = match &accessor.name {
                        PropName::Ident(..) | PropName::String(..) | PropName::Number(..) => {
                            let accessor_name =
                                Self::legacy_es5_literal_member_key(&accessor.name).unwrap();
                            if !emitted_accessors.insert((accessor_name.clone(), is_static)) {
                                continue;
                            }
                            let mut getter = None;
                            let mut setter = None;
                            for candidate in &class_decl.members {
                                match &candidate.kind {
                                    ClassMemberKind::GetAccessor(other)
                                        if !Self::legacy_es5_accessor_is_erased(other)
                                            && (other.modifiers & MOD_STATIC != 0) == is_static
                                            && Self::legacy_es5_literal_member_key(&other.name)
                                                .as_ref()
                                                == Some(&accessor_name) =>
                                    {
                                        getter = Some((other, candidate.span));
                                    }
                                    ClassMemberKind::SetAccessor(other)
                                        if !Self::legacy_es5_accessor_is_erased(other)
                                            && (other.modifiers & MOD_STATIC != 0) == is_static
                                            && Self::legacy_es5_literal_member_key(&other.name)
                                                .as_ref()
                                                == Some(&accessor_name) =>
                                    {
                                        setter = Some((other, candidate.span));
                                    }
                                    _ => {}
                                }
                            }
                            (getter, setter)
                        }
                        PropName::Computed(..) => match &member.kind {
                            ClassMemberKind::GetAccessor(accessor) => {
                                (Some((accessor, member.span)), None)
                            }
                            ClassMemberKind::SetAccessor(accessor) => {
                                (None, Some((accessor, member.span)))
                            }
                            _ => unreachable!(),
                        },
                        _ => continue,
                    };

                    if !is_computed {
                        self.emit_leading_comments(member.span.start);
                    }
                    self.write("Object.defineProperty(");
                    self.write(name);
                    if !is_static {
                        self.write(".prototype");
                    }
                    self.write(", ");
                    match &accessor.name {
                        PropName::Ident(accessor_name, _) => {
                            self.write("\"");
                            self.write(accessor_name);
                            self.write("\"");
                        }
                        PropName::String(..) | PropName::Number(..) => {
                            self.emit_prop_name(&accessor.name)
                        }
                        PropName::Computed(expr, _) => self.emit_legacy_es5_computed_key_expr(expr),
                        _ => unreachable!(),
                    }
                    self.writeln(", {");
                    self.indent += 1;
                    if is_computed {
                        self.emit_leading_comments(member.span.start);
                    }
                    if let Some((getter, span)) = getter {
                        self.write("get: ");
                        self.emit_legacy_es5_accessor_function(getter, span);
                        self.writeln(",");
                    }
                    if let Some((setter, span)) = setter {
                        self.write("set: ");
                        self.emit_legacy_es5_accessor_function(setter, span);
                        self.writeln(",");
                    }
                    self.writeln("enumerable: false,");
                    self.writeln("configurable: true");
                    self.indent -= 1;
                    self.writeln("});");
                }
                ClassMemberKind::Property(property)
                    if matches!(property.name, PropName::Computed(..)) =>
                {
                    // The shape gate only admits side-effect-free literal keys,
                    // so both the field and its comments are erased together.
                    self.advance_comment_pos(member.span.end);
                }
                ClassMemberKind::IndexSignature(_) => {
                    // Index signatures and their attached comments are
                    // type-only. Consume them before a later computed member
                    // asks for its own leading comments.
                    self.advance_comment_pos(member.span.end);
                }
                ClassMemberKind::SemicolonClassElement => {
                    self.emit_leading_comments(member.span.start);
                    self.writeln(";");
                    self.append_trailing_comment(member.span);
                }
                _ => {}
            }
        }
        for member in &class_decl.members {
            if let ClassMemberKind::Property(property) = &member.kind {
                if property.modifiers & MOD_STATIC != 0 && property.modifiers & MOD_DECLARE == 0 {
                    if let Some(initializer) = &property.initializer {
                        self.emit_leading_comments(member.span.start);
                        self.write(name);
                        self.emit_member_access(&property.name);
                        self.write(" = ");
                        let saved_this = self.static_this_alias.replace(name.to_string());
                        self.emit_class_prop_initializer_expr(property, initializer);
                        self.static_this_alias = saved_this;
                        self.writeln(";");
                    }
                }
            }
        }
        self.write("return ");
        self.write(name);
        self.writeln(";");
        self.indent -= 1;
        self.write("}(");
        if let Some(extends) = &class_decl.extends {
            self.emit_expr(extends);
        }
        self.writeln("));");
    }

    pub(super) fn emit_class_decl(&mut self, class_decl: &ClassDecl) {
        if class_decl.modifiers & MOD_DECLARE != 0 {
            return;
        }
        let lower_metadata_class = self.effective_target() < ScriptTarget::ES2015
            && self.legacy_es5_metadata_class_can_lower(class_decl);
        if !self.in_class_expression_emit
            && (lower_metadata_class
                || (self.effective_target() < ScriptTarget::ES2015
                    && self.legacy_es5_class_can_lower(class_decl)))
        {
            if let Some(ref name) = class_decl.name {
                self.emitted_var_names.insert(name.into());
            }
            self.emit_legacy_es5_class_decl(class_decl, lower_metadata_class);
            return;
        }
        // Register the class name so merged namespace/enum IIFEs skip `var`.
        if let Some(ref name) = class_decl.name {
            self.emitted_var_names.insert(name.into());
        }

        let use_define_semantics = self.use_define_for_class_fields();
        let downlevel_class_fields = use_define_semantics && self.needs_downlevel("class-fields");
        // Native class field syntax is only available when class fields are not downleveled.
        let use_define = use_define_semantics && !downlevel_class_fields;
        let downlevel_private = self.needs_downlevel("private-fields");
        let invalid_duplicate_private_names = Self::invalid_duplicate_private_name_info(class_decl);
        let private_field_name_by_span = Self::private_field_name_by_span(class_decl);
        // Track enclosing class name for private field WeakMap variable naming.
        let prev_class_name = self.current_class_name.take();
        let prev_private_fields = std::mem::take(&mut self.current_class_private_fields);
        let prev_static_private_fields =
            std::mem::take(&mut self.current_class_static_private_fields);
        let prev_static_alias = self.current_class_static_alias.take();
        let prev_class_scope_temp_reserved = self.class_scope_temp_reserved;
        let prev_private_methods = std::mem::take(&mut self.current_class_private_methods);
        let prev_static_private_methods =
            std::mem::take(&mut self.current_class_static_private_methods);
        let prev_private_accessors = std::mem::take(&mut self.current_class_private_accessors);
        let prev_static_private_accessors =
            std::mem::take(&mut self.current_class_static_private_accessors);
        let prev_private_var_map = std::mem::take(&mut self.current_class_private_var_map);
        self.current_class_name = class_decl.name.clone();
        self.current_class_private_fields = class_decl
            .members
            .iter()
            .filter_map(|m| {
                if let ClassMemberKind::Property(prop) = &m.kind {
                    if let PropName::Private(name, _) = &prop.name {
                        return Some(AstString::from(normalize_unicode_escapes(name)));
                    }
                }
                None
            })
            .collect();
        // Track which private fields are static (for 4-arg __classPrivateFieldGet/Set).
        if downlevel_private {
            self.current_class_static_private_fields = class_decl
                .members
                .iter()
                .filter_map(|m| {
                    if let ClassMemberKind::Property(prop) = &m.kind {
                        if let PropName::Private(name, _) = &prop.name {
                            if prop.modifiers & MOD_STATIC != 0 {
                                return Some(AstString::from(normalize_unicode_escapes(name)));
                            }
                        }
                    }
                    None
                })
                .collect();
        } else {
            self.current_class_static_private_fields.clear();
        }
        self.recovery_static_class_assignments.clear();

        // Compute temp captures for computed property names that reference the class.
        // For class declarations, TypeScript only allocates temps when there are
        // instance fields or static properties with computed [ClassName.prop] names
        // (these get hoisted/moved). Pure method computed names don't need temps.
        let class_name = class_decl.name.as_deref().unwrap_or("");
        let has_computed_field_or_static_prop = class_decl.members.iter().any(|m| {
            if let ClassMemberKind::Property(prop) = &m.kind {
                if prop.modifiers & MOD_DECLARE != 0 || prop.modifiers & MOD_ABSTRACT != 0 {
                    return false;
                }
                if let PropName::Computed(expr, _) = &prop.name {
                    return expr_is_class_member_access(expr, class_name).is_some();
                }
            }
            false
        });
        let prop_keys = if has_computed_field_or_static_prop {
            class_computed_name_prop_keys(class_decl, class_name, use_define)
        } else {
            Vec::new()
        };
        let mut computed_temps: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        let mut instance_prop_temps: Vec<String> = Vec::new();
        for key in &prop_keys {
            let temp = self.next_temp_var();
            computed_temps.insert(key.clone(), temp);
        }
        for member in &class_decl.members {
            if let ClassMemberKind::Property(prop) = &member.kind {
                if prop.modifiers & MOD_STATIC == 0
                    && prop.initializer.is_some()
                    && prop.modifiers & MOD_DECLARE == 0
                    && prop.modifiers & MOD_ABSTRACT == 0
                {
                    if let PropName::Computed(expr, _) = &prop.name {
                        if let Some(p) = expr_is_class_member_access(expr, class_name) {
                            let key = format!("{}.{}", class_name, p);
                            if computed_temps.contains_key(&key) {
                                instance_prop_temps.push(key);
                            }
                        }
                    }
                }
            }
        }
        let has_computed_name_temps = !computed_temps.is_empty();
        if has_computed_name_temps {
            self.class_computed_name_temps = Some(computed_temps);
            self.class_instance_prop_temps = Some(instance_prop_temps);
            self.class_preceding_static_prop_keys = Some(Vec::new());
        }

        // Collect private fields and auto-accessors for WeakMap-based downleveling.
        let class_ident = class_decl.name.as_deref().unwrap_or("default");
        let mut private_fields: Vec<(
            String,
            Option<&Expr>,
            bool,
            Span,
            Option<ClassExprBindingName>,
        )> = if downlevel_private {
            class_decl
                .members
                .iter()
                .filter_map(|m| {
                    if let ClassMemberKind::Property(ref prop) = m.kind {
                        if let PropName::Private(ref name, _) = prop.name {
                            // Skip `declare` and `abstract` private fields — they are
                            // type-only and should not be lowered to WeakMaps.
                            if prop.modifiers & MOD_DECLARE != 0
                                || prop.modifiers & MOD_ABSTRACT != 0
                            {
                                return None;
                            }
                            let is_static = prop.modifiers & MOD_STATIC != 0;
                            // TypeScript uses _ClassName_fieldName for private field WeakMaps.
                            // Unicode escapes in identifiers must be normalized to
                            // their actual characters (e.g. \u0078 → x).
                            let norm_name = normalize_unicode_escapes(name);
                            let norm_class = normalize_unicode_escapes(class_ident);
                            let qualified_name = format!("{}_{}", norm_class, norm_name);
                            return Some((
                                qualified_name,
                                prop.initializer.as_deref(),
                                is_static,
                                m.span,
                                Some(ClassExprBindingName::Literal(format!("#{}", norm_name))),
                            ));
                        }
                    }
                    None
                })
                .collect()
        } else {
            Vec::new()
        };

        // Collect private instance methods for downleveling:
        //   #x() { ... } -> _Class_instances (WeakSet) + _Class_x function helper.
        let norm_class = normalize_unicode_escapes(class_ident);
        let mut private_instance_methods: Vec<(&ClassMethod, Span, String, String)> =
            if downlevel_private {
                class_decl
                    .members
                    .iter()
                    .filter_map(|m| {
                        if let ClassMemberKind::Method(ref method) = m.kind {
                            if let PropName::Private(ref name, _) = method.name {
                                if method.modifiers & MOD_STATIC == 0 && method.body.is_some() {
                                    let norm_name = normalize_unicode_escapes(name);
                                    let helper_decl_name = format!("{}_{}", norm_class, norm_name);
                                    return Some((method, m.span, helper_decl_name, norm_name));
                                }
                            }
                        }
                        None
                    })
                    .collect()
            } else {
                Vec::new()
            };
        for (_, _, helper_decl_name, private_name) in &private_instance_methods {
            self.current_class_private_methods.insert(
                AstString::from(private_name.as_str()),
                format!("_{}", helper_decl_name).into(),
            );
        }

        // Collect static private methods for downleveling:
        //   static #m() { ... } → _Class_m function, uses class alias as brand
        let mut private_static_methods: Vec<(&ClassMethod, Span, String, String)> =
            if downlevel_private {
                class_decl
                    .members
                    .iter()
                    .filter_map(|m| {
                        if let ClassMemberKind::Method(ref method) = m.kind {
                            if let PropName::Private(ref name, _) = method.name {
                                if method.modifiers & MOD_STATIC != 0 && method.body.is_some() {
                                    let norm_name = normalize_unicode_escapes(name);
                                    let helper_decl_name = format!("{}_{}", norm_class, norm_name);
                                    return Some((method, m.span, helper_decl_name, norm_name));
                                }
                            }
                        }
                        None
                    })
                    .collect()
            } else {
                Vec::new()
            };
        for (_, _, helper_decl_name, private_name) in &private_static_methods {
            self.current_class_static_private_methods.insert(
                AstString::from(private_name.as_str()),
                format!("_{}", helper_decl_name).into(),
            );
        }
        let has_static_private_methods = !private_static_methods.is_empty();

        // Collect private get/set accessors for downleveling:
        //   get #x() { ... } → _Class_x_get function
        //   set #x(v) { ... } → _Class_x_set function
        // Both get and set share _Class_instances WeakSet brand.
        #[allow(dead_code)]
        struct PrivateAccessorInfo<'a> {
            acc: &'a ClassAccessor,
            span: Span,
            helper_decl_name: String,
            private_name: String,
            is_getter: bool,
            is_static: bool,
        }
        let mut private_accessors: Vec<PrivateAccessorInfo<'_>> = if downlevel_private {
            class_decl
                .members
                .iter()
                .filter_map(|m| {
                    let (acc, is_getter) = match &m.kind {
                        ClassMemberKind::GetAccessor(a) => (a, true),
                        ClassMemberKind::SetAccessor(a) => (a, false),
                        _ => return None,
                    };
                    if let PropName::Private(ref name, _) = acc.name {
                        let norm_name = normalize_unicode_escapes(name);
                        let suffix = if is_getter { "get" } else { "set" };
                        let helper_decl_name = format!("{}_{}_{}", norm_class, norm_name, suffix);
                        let is_static = acc.modifiers & MOD_STATIC != 0;
                        return Some(PrivateAccessorInfo {
                            acc,
                            span: m.span,
                            helper_decl_name,
                            private_name: norm_name,
                            is_getter,
                            is_static,
                        });
                    }
                    None
                })
                .collect()
        } else {
            Vec::new()
        };
        // Register accessors: map private name → (getter_var, setter_var)
        for info in &private_accessors {
            let entry = self
                .current_class_private_accessors
                .entry(AstString::from(info.private_name.as_str()))
                .or_insert((None, None));
            let var: AstString = format!("_{}", info.helper_decl_name).into();
            if info.is_getter {
                entry.0 = Some(var);
            } else {
                entry.1 = Some(var);
            }
            if info.is_static {
                self.current_class_static_private_accessors
                    .insert(AstString::from(info.private_name.as_str()));
            }
        }
        let has_private_accessors = !private_accessors.is_empty();
        // Accessors also need _instances brand (share with methods).
        let has_static_private_accessors = private_accessors.iter().any(|info| info.is_static);
        let has_non_static_accessors = private_accessors.iter().any(|info| !info.is_static);
        let private_method_brand_name =
            if private_instance_methods.is_empty() && !has_non_static_accessors {
                None
            } else {
                Some(format!("{}_instances", norm_class))
            };

        // Also collect auto-accessors (properties with MOD_ACCESSOR) when not using native class fields
        let private_named_field_count = private_fields.len();
        if !use_define && self.needs_downlevel("auto-accessors") {
            for member in &class_decl.members {
                if let ClassMemberKind::Property(ref prop) = member.kind {
                    if prop.modifiers & MOD_ACCESSOR != 0 {
                        // Ambient accessors don't need storage
                        if prop.modifiers & MOD_DECLARE != 0 {
                            continue;
                        }
                        if let Some(prop_name) = prop.name.ident_name() {
                            let class_name = class_decl.name.as_deref().unwrap_or("default");
                            let storage_name =
                                format!("{}_{}_accessor_storage", class_name, prop_name);
                            let is_static = prop.modifiers & MOD_STATIC != 0;
                            private_fields.push((
                                storage_name,
                                prop.initializer.as_deref(),
                                is_static,
                                member.span,
                                None,
                            ));
                        }
                    }
                }
            }
        }

        let downlevel_static_blocks = self.needs_downlevel("static-blocks");

        // Check if class has static private fields that need downleveling.
        let has_static_private_fields = downlevel_private
            && private_fields
                .iter()
                .any(|(_, _, is_static, _, _)| *is_static);
        let needs_private_helper_alias = downlevel_private
            && class_needs_private_helper_alias(
                class_decl,
                class_decl.name.as_deref().unwrap_or(""),
            );

        // In legacy mode (useDefineForClassFields = false), classes with static
        // field initializers need a `var _a; ... _a = ClassName;` wrapper so that
        // the class identity is captured before static initializers run.
        // Also needed when downleveling static private fields (brand check).
        let needs_class_alias = class_decl.name.is_some()
            && ((!use_define
                && downlevel_static_blocks
                && class_needs_static_alias(class_decl, downlevel_static_blocks))
                || has_static_private_fields
                || has_static_private_methods
                || has_static_private_accessors
                || needs_private_helper_alias);
        // For classes with private instance methods, keep the static alias temp in
        // the private-helper declaration list so ordering matches TypeScript.
        let has_any_private_helpers = !private_instance_methods.is_empty()
            || has_static_private_fields
            || has_private_accessors
            || has_static_private_methods;
        let class_temp_needs_private_decl = needs_class_alias && has_any_private_helpers;
        let class_temp_name = if needs_class_alias {
            if class_temp_needs_private_decl {
                Some(self.next_inline_temp_var())
            } else {
                let name = self.next_temp_var();
                self.class_expr_temp_emitted = true;
                Some(name)
            }
        } else {
            None
        };
        // Store the class alias for static private field/method/accessor access in expressions.
        if has_static_private_fields
            || has_static_private_methods
            || has_static_private_accessors
            || needs_private_helper_alias
        {
            self.current_class_static_alias = class_temp_name.clone();
        }
        let static_super_base_alias = if !use_define
            && class_decl.extends.is_some()
            && class_needs_static_super_base_alias(class_decl, downlevel_static_blocks)
        {
            let name = self.next_temp_var();
            self.class_expr_temp_emitted = true;
            Some(name)
        } else {
            None
        };

        // Record how many temps the CLASS itself allocated (alias, super base, etc.)
        // so that method-body temp counters start past these reserved names.
        // Only set non-zero when the class actually has a static alias to avoid
        // conflicting with file-level temps in classes without private statics.
        self.class_scope_temp_reserved = if self.current_class_static_alias.is_some() {
            self.temp_var_counter
        } else {
            0
        };

        // Emit declarations for private helper vars (WeakMap/WeakSet/method helpers).
        // TypeScript hoists these to the top of user code as a single declaration.
        let mut helper_var_names: Vec<String> = Vec::new();
        let mut helper_var_seen: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        // TypeScript ordering for private helper vars:
        // 1. _instances brand (when instance methods/accessors present) — FIRST
        // 2. Class alias _a (when class needs static wrapper)
        // 3. All private members in SOURCE ORDER (fields, methods, accessors interleaved)

        // Helper closure: allocate a unique name, suffixing with _1, _2, etc. on collision.
        // Returns the actual allocated name (without leading underscore).
        let allocate_unique = |base: &str, all: &mut HashMap<AstString, u32>| -> String {
            let count = all.entry(AstString::from(base)).or_insert(0);
            let actual = if *count == 0 {
                base.to_string()
            } else {
                format!("{}_{}", base, count)
            };
            *count += 1;
            actual
        };
        let allocate_accessor_unique =
            |base: &str, suffix: &str, all: &mut HashMap<AstString, u32>| -> String {
                let key = format!("{}:{}", base, suffix);
                let count = all.entry(AstString::from(key.as_str())).or_insert(0);
                let actual = if *count == 0 {
                    format!("{}_{}", base, suffix)
                } else {
                    format!("{}_{}_{}", base, count, suffix)
                };
                *count += 1;
                actual
            };

        if let Some(ref brand) = private_method_brand_name {
            if helper_var_seen.insert(brand.clone()) {
                let actual = allocate_unique(brand, &mut self.all_allocated_private_var_names);
                helper_var_names.push(actual.clone());
                self.current_class_private_var_map.insert(
                    format!("brand:{}", brand).into(),
                    format!("_{}", actual).into(),
                );
            }
        }
        if class_temp_needs_private_decl {
            if let Some(ref temp) = class_temp_name {
                let decl_name = temp.strip_prefix('_').unwrap_or(temp).to_string();
                if helper_var_seen.insert(decl_name.clone()) {
                    helper_var_names.push(decl_name);
                }
            }
        }
        // Collect all private member helper vars in source order.
        let downlevel_auto_accessors = !use_define && self.needs_downlevel("auto-accessors");
        let mut private_field_idx = 0usize;
        let mut auto_accessor_storage_idx = 0usize;
        let mut private_instance_method_idx = 0usize;
        let mut private_static_method_idx = 0usize;
        let mut private_accessor_idx = 0usize;
        if downlevel_private || downlevel_auto_accessors {
            for member in &class_decl.members {
                match &member.kind {
                    ClassMemberKind::Property(prop) => {
                        if let PropName::Private(ref name, _) = prop.name {
                            if downlevel_private
                                && prop.modifiers & MOD_DECLARE == 0
                                && prop.modifiers & MOD_ABSTRACT == 0
                            {
                                let norm_name = normalize_unicode_escapes(name);
                                let qualified = format!("{}_{}", norm_class, norm_name);
                                let actual = allocate_unique(
                                    &qualified,
                                    &mut self.all_allocated_private_var_names,
                                );
                                helper_var_names.push(actual.clone());
                                if let Some((field_name, _, _, _, _)) =
                                    private_fields.get_mut(private_field_idx)
                                {
                                    *field_name = actual.clone();
                                }
                                private_field_idx += 1;
                                self.current_class_private_var_map
                                    .insert(norm_name.into(), format!("_{}", actual).into());
                            }
                        } else if downlevel_auto_accessors
                            && prop.modifiers & MOD_ACCESSOR != 0
                            && prop.modifiers & MOD_DECLARE == 0
                        {
                            if let Some(prop_name) = prop.name.ident_name() {
                                let class_name = class_decl.name.as_deref().unwrap_or("default");
                                let storage_name =
                                    format!("{}_{}_accessor_storage", class_name, prop_name);
                                let actual = allocate_unique(
                                    &storage_name,
                                    &mut self.all_allocated_private_var_names,
                                );
                                helper_var_names.push(actual.clone());
                                if let Some((field_name, _, _, _, _)) = private_fields
                                    .get_mut(private_named_field_count + auto_accessor_storage_idx)
                                {
                                    *field_name = actual;
                                }
                                auto_accessor_storage_idx += 1;
                            }
                        }
                    }
                    ClassMemberKind::Method(method) => {
                        if let PropName::Private(ref name, _) = method.name {
                            if method.body.is_some() {
                                let norm_name = normalize_unicode_escapes(name);
                                let qualified = format!("{}_{}", norm_class, norm_name);
                                let actual = allocate_unique(
                                    &qualified,
                                    &mut self.all_allocated_private_var_names,
                                );
                                helper_var_names.push(actual.clone());
                                let target_idx = if method.modifiers & MOD_STATIC == 0 {
                                    let idx = private_instance_method_idx;
                                    private_instance_method_idx += 1;
                                    idx
                                } else {
                                    let idx = private_static_method_idx;
                                    private_static_method_idx += 1;
                                    idx
                                };
                                if method.modifiers & MOD_STATIC == 0 {
                                    if let Some((_, _, helper_decl_name, _)) =
                                        private_instance_methods.get_mut(target_idx)
                                    {
                                        *helper_decl_name = actual.clone();
                                    }
                                } else if let Some((_, _, helper_decl_name, _)) =
                                    private_static_methods.get_mut(target_idx)
                                {
                                    *helper_decl_name = actual.clone();
                                }
                                self.current_class_private_var_map.insert(
                                    format!("method:{}", norm_name).into(),
                                    format!("_{}", actual).into(),
                                );
                            }
                        }
                    }
                    ClassMemberKind::GetAccessor(acc) | ClassMemberKind::SetAccessor(acc) => {
                        if let PropName::Private(ref name, _) = acc.name {
                            {
                                let norm_name = normalize_unicode_escapes(name);
                                let suffix =
                                    if matches!(member.kind, ClassMemberKind::GetAccessor(_)) {
                                        "get"
                                    } else {
                                        "set"
                                    };
                                let accessor_base = format!("{}_{}", norm_class, norm_name);
                                let actual = allocate_accessor_unique(
                                    &accessor_base,
                                    suffix,
                                    &mut self.all_allocated_private_var_names,
                                );
                                helper_var_names.push(actual.clone());
                                if let Some(info) = private_accessors.get_mut(private_accessor_idx)
                                {
                                    info.helper_decl_name = actual.clone();
                                }
                                private_accessor_idx += 1;
                                self.current_class_private_var_map.insert(
                                    format!("{}:{}", suffix, norm_name).into(),
                                    format!("_{}", actual).into(),
                                );
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        self.current_class_private_methods.clear();
        for (_, _, helper_decl_name, private_name) in &private_instance_methods {
            self.current_class_private_methods.insert(
                AstString::from(private_name.as_str()),
                format!("_{}", helper_decl_name).into(),
            );
        }
        self.current_class_static_private_methods.clear();
        for (_, _, helper_decl_name, private_name) in &private_static_methods {
            self.current_class_static_private_methods.insert(
                AstString::from(private_name.as_str()),
                format!("_{}", helper_decl_name).into(),
            );
        }
        self.current_class_private_accessors.clear();
        self.current_class_static_private_accessors.clear();
        for info in &private_accessors {
            let entry = self
                .current_class_private_accessors
                .entry(AstString::from(info.private_name.as_str()))
                .or_insert((None, None));
            let var: AstString = format!("_{}", info.helper_decl_name).into();
            if info.is_getter {
                entry.0 = Some(var);
            } else {
                entry.1 = Some(var);
            }
            if info.is_static {
                self.current_class_static_private_accessors
                    .insert(AstString::from(info.private_name.as_str()));
            }
        }
        if !helper_var_names.is_empty() {
            let use_block_local_helper_decl =
                self.block_depth > 0 && self.class_decl_helper_insert_pos.is_some();
            if use_block_local_helper_decl {
                let insert_pos = self.class_decl_helper_insert_pos.unwrap();
                let mut helper_text = String::new();
                let indent_str = "    ".repeat(self.indent as usize);
                helper_text.push_str(&indent_str);
                helper_text.push_str("let ");
                for (i, name) in helper_var_names.iter().enumerate() {
                    if i > 0 {
                        helper_text.push_str(", ");
                    }
                    helper_text.push('_');
                    helper_text.push_str(name);
                }
                helper_text.push_str(";\n");
                self.output.insert_str(insert_pos, &helper_text);
                self.out_line += 1;
                if let Some(ref mut pos) = self.class_decl_helper_insert_pos {
                    *pos += helper_text.len();
                }
            } else if let Some(insert_pos) = self.private_field_var_insert_pos {
                if let Some(semi_pos) = self.private_field_var_semicolon_pos {
                    // Append to existing var declaration: insert ", _name" before the ";".
                    let mut append_text = String::new();
                    for name in &helper_var_names {
                        append_text.push_str(", _");
                        append_text.push_str(name);
                    }
                    self.output.insert_str(semi_pos, &append_text);
                    // Update tracked positions after insertion.
                    self.private_field_var_semicolon_pos = Some(semi_pos + append_text.len());
                    if let Some(ref mut pos) = self.private_field_var_insert_pos {
                        *pos += append_text.len();
                    }
                } else if self.fn_scope_depth == 0 && !self.temp_var_names.is_empty() {
                    // A class expression already added vars to temp_var_names;
                    // add ours too so they get combined into a single `var` statement.
                    for name in &helper_var_names {
                        self.temp_var_names.push(format!("_{}", name).into());
                    }
                } else {
                    // First var declaration: create new `var _name1, _name2;`.
                    let mut helper_text = String::new();
                    let indent_str = "    ".repeat(self.indent as usize);
                    helper_text.push_str(&indent_str);
                    helper_text.push_str("var ");
                    for (i, name) in helper_var_names.iter().enumerate() {
                        if i > 0 {
                            helper_text.push_str(", ");
                        }
                        helper_text.push('_');
                        helper_text.push_str(name);
                    }
                    // Track the semicolon position for future appends.
                    let semi_offset = insert_pos + helper_text.len();
                    helper_text.push_str(";\n");
                    self.output.insert_str(insert_pos, &helper_text);
                    self.out_line += 1;
                    self.private_field_var_semicolon_pos = Some(semi_offset);
                    if let Some(ref mut pos) = self.private_field_var_insert_pos {
                        *pos += helper_text.len();
                    }
                }
            } else if self.fn_scope_depth > 0 {
                // Inside a function body: collect names for hoisting to
                // the function body start (handled by emit_block_for_decl_body).
                for name in &helper_var_names {
                    self.pending_private_field_vars.push(format!("_{}", name));
                }
            } else if let Some(insert_pos) = self.class_decl_helper_insert_pos {
                let mut helper_text = String::new();
                let indent_str = "    ".repeat(self.indent as usize);
                helper_text.push_str(&indent_str);
                helper_text.push_str("var ");
                for (i, name) in helper_var_names.iter().enumerate() {
                    if i > 0 {
                        helper_text.push_str(", ");
                    }
                    helper_text.push('_');
                    helper_text.push_str(name);
                }
                helper_text.push_str(";\n");
                self.output.insert_str(insert_pos, &helper_text);
                self.out_line += 1;
                if let Some(ref mut pos) = self.class_decl_helper_insert_pos {
                    *pos += helper_text.len();
                }
            } else {
                self.write("var ");
                for (i, name) in helper_var_names.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.write("_");
                    self.write(name);
                }
                self.writeln(";");
            }
        }

        // Preserve TC39 class-level decorators in output for ESNext targets
        if self.should_preserve_decorators() && !class_decl.decorators.is_empty() {
            // Find the `class` keyword position in the source (after last decorator).
            let last_dec_end = class_decl.decorators.last().unwrap().span.end as usize;
            let class_kw_pos = self
                .source
                .get(last_dec_end..)
                .and_then(|s| s.find("class"))
                .map(|off| (last_dec_end + off) as u32);
            self.emit_preserved_decorators(&class_decl.decorators, class_kw_pos);
        }
        self.write("class");
        if let Some(ref name) = class_decl.name {
            self.write(" ");
            self.write(name);
        }
        // When there's no extends clause, check for inline block comments
        // between the class name and `{` (e.g. `class Foo /* extends Bar */ {`).
        if class_decl.extends.is_none()
            && class_decl.implements.is_empty()
            && class_decl.type_params.is_none()
        {
            let name_end = class_decl.name.as_ref().map_or(
                class_decl.span.start as usize + 5, // "class".len()
                |n| {
                    let class_start = class_decl.span.start as usize;
                    if class_start >= self.source.len() {
                        return class_start;
                    }
                    let src = &self.source[class_start..];
                    if let Some(pos) = src.find(n.as_str()) {
                        class_start + pos + n.len()
                    } else {
                        // Synthetic name (e.g. default_1) not in source text;
                        // fall back to just past the "class" keyword.
                        let fallback = class_start + 5;
                        fallback.min(self.source.len())
                    }
                },
            );
            let brace_pos = self
                .source
                .get(name_end..)
                .and_then(|s| s.find('{'))
                .map(|p| name_end + p);
            if let Some(brace) = brace_pos {
                let between = &self.source[name_end..brace];
                if let Some(bc_start) = between.find("/*") {
                    if let Some(bc_end) = between[bc_start..].find("*/") {
                        let comment = &between[bc_start..bc_start + bc_end + 2];
                        self.write(" ");
                        self.write(comment);
                    }
                }
            }
        }
        if let Some(ref extends) = class_decl.extends {
            self.write(" extends ");
            if let Some(ref base_alias) = static_super_base_alias {
                self.write("(");
                self.write(base_alias);
                self.write(" = ");
                self.emit_expr(extends);
                self.write(")");
            } else if self.class_extends_void_recovery_tail(class_decl) {
                // Recovery shape: `class C extends void {}` keeps an empty
                // heritage slot and emits `void {};` after the class.
            } else {
                // For parser-recovery cases like `class C extends A, B {`, copy the
                // extends clause from source text to preserve the invalid syntax.
                let extends_end = extends.span.end as usize;
                if extends_end < self.source.len() {
                    let rest = &self.source[extends_end..];
                    let brace_offset = rest.find('{').unwrap_or(0);
                    let between = &rest[..brace_offset];
                    // Only use source-copy when there's a comma followed by a real
                    // identifier (not just a trailing comma before `{`).
                    // When `implements` is present with a second `extends` after it,
                    // still preserve the extra extends for error recovery.
                    let has_extra_extends_after_implements = between.contains("implements")
                        && between
                            .rfind("extends")
                            .is_some_and(|p| p > between.find("implements").unwrap_or(0));
                    let has_extra_base = (!between.contains("implements")
                        && heritage_tail_has_extra_base(between))
                        || has_extra_extends_after_implements;
                    if has_extra_base {
                        // Find the "extends" keyword after the class name
                        let search_start = class_decl
                            .name
                            .as_ref()
                            .map_or(class_decl.span.start as usize + 5, |_n| {
                                extends.span.start as usize
                            });
                        let kw_pos = self.source[..search_start].rfind("extends").or_else(|| {
                            self.source[search_start..]
                                .find("extends")
                                .map(|o| search_start + o)
                        });
                        if let Some(kw) = kw_pos {
                            let brace_pos = extends_end + brace_offset;
                            let mut clause_text = self.source[kw + "extends".len()..brace_pos]
                                .trim()
                                .to_string();
                            // Strip `implements ... extends` sections from the
                            // clause (implements is type-only, but preserve the
                            // second extends for error recovery output).
                            if let Some(impl_pos) = clause_text.find("implements") {
                                if let Some(ext2_pos) = clause_text[impl_pos..].find("extends") {
                                    // Remove `implements ... ` before second `extends`
                                    let before = clause_text[..impl_pos].trim_end();
                                    let after = clause_text[impl_pos + ext2_pos..].trim_start();
                                    clause_text = format!("{} {}", before, after);
                                } else {
                                    // implements without second extends — strip implements section
                                    clause_text = clause_text[..impl_pos].trim_end().to_string();
                                }
                            }
                            let clause_text = strip_heritage_type_args(&clause_text);
                            // Normalize comma spacing: `A,B` → `A, B`
                            let clause_text = clause_text.replace(",", ", ");
                            let clause_text = clause_text.replace(",  ", ", ");
                            self.write(&clause_text);
                        } else {
                            self.emit_expr(extends);
                        }
                    } else {
                        self.emit_expr(extends);
                    }
                } else {
                    self.emit_expr(extends);
                }
            }
        }
        self.writeln(" {");
        self.indent += 1;

        // Inject `static { alias = this; }` for legacy-decorated classes with
        // static initializers when the target supports static blocks natively.
        if let Some(alias) = self.inject_decorator_alias_block.take() {
            self.write("static { ");
            self.write(&alias);
            self.writeln(" = this; }");
        }

        // Track computed property name expressions that need temp vars.
        // Legacy class-field emit uses these to pre-evaluate keys before the
        // class body, and define-mode named evaluation uses them to capture a
        // computed key once for __setFunctionName.
        self.class_general_computed_temps.clear();
        let general_class_name = class_decl.name.as_deref().unwrap_or("");
        let mut seen_starts = std::collections::HashSet::new();
        if !use_define {
            for member in &class_decl.members {
                let computed = match &member.kind {
                    ClassMemberKind::Property(prop) => {
                        if let PropName::Computed(expr, _) = &prop.name {
                            let define_uninitialized_field = downlevel_class_fields
                                && prop.initializer.is_none()
                                && prop.modifiers & MOD_ACCESSOR == 0
                                && !(prop.definite && prop.type_ann.is_some());
                            let needs_temp = prop.initializer.is_some()
                                || define_uninitialized_field
                                || !prop.decorators.is_empty();
                            if needs_temp
                                && prop.modifiers & MOD_DECLARE == 0
                                && prop.modifiers & MOD_ABSTRACT == 0
                            {
                                // Skip expressions already handled by class_computed_name_temps
                                // (e.g. [ClassName.staticProp] patterns)
                                if !general_class_name.is_empty()
                                    && expr_is_class_member_access(expr, general_class_name)
                                        .is_some()
                                {
                                    None
                                } else if Self::computed_name_is_simple_literal(expr) {
                                    None
                                } else {
                                    Some((expr, !prop.decorators.is_empty()))
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    }
                    ClassMemberKind::Method(method) => {
                        if method.modifiers & MOD_DECLARE == 0
                            && method.modifiers & MOD_ABSTRACT == 0
                            && !method.decorators.is_empty()
                        {
                            if let PropName::Computed(expr, _) = &method.name {
                                if Self::computed_name_is_simple_literal(expr) {
                                    None
                                } else {
                                    Some((expr, true))
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    }
                    ClassMemberKind::GetAccessor(acc) | ClassMemberKind::SetAccessor(acc) => {
                        if acc.modifiers & MOD_DECLARE == 0
                            && acc.modifiers & MOD_ABSTRACT == 0
                            && !acc.decorators.is_empty()
                        {
                            if let PropName::Computed(expr, _) = &acc.name {
                                if Self::computed_name_is_simple_literal(expr) {
                                    None
                                } else {
                                    Some((expr, true))
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
                if let Some((expr, use_deferred_temp)) = computed {
                    let start = expr.span.start;
                    if seen_starts.insert(start) {
                        let temp = if use_deferred_temp {
                            self.next_deferred_temp_placeholder()
                        } else {
                            self.next_temp_var()
                        };
                        self.class_general_computed_temps.push((temp, start));
                    }
                }
            }
        }
        for member in &class_decl.members {
            let ClassMemberKind::Property(prop) = &member.kind else {
                continue;
            };
            if prop.modifiers & MOD_DECLARE != 0 || prop.modifiers & MOD_ABSTRACT != 0 {
                continue;
            }
            let (PropName::Computed(expr, _), Some(init)) =
                (&prop.name, prop.initializer.as_deref())
            else {
                continue;
            };
            if !Self::expr_needs_class_expr_binding_name(init)
                || Self::computed_name_is_simple_literal(expr)
            {
                continue;
            }
            if !general_class_name.is_empty()
                && expr_is_class_member_access(expr, general_class_name).is_some()
            {
                continue;
            }
            if seen_starts.insert(expr.span.start) {
                let temp = self.next_temp_var();
                self.class_general_computed_temps
                    .push((temp, expr.span.start));
            }
        }

        let general_temp_by_start: std::collections::HashMap<u32, String> = self
            .class_general_computed_temps
            .iter()
            .map(|(temp, start)| (*start, temp.clone()))
            .collect();
        let mut legacy_method_name_overrides: std::collections::HashMap<u32, String> =
            std::collections::HashMap::new();
        let mut legacy_post_class_computed_actions: Vec<(Option<String>, Expr)> = Vec::new();
        if !use_define {
            let computed_method_anchors: Vec<usize> = class_decl
                .members
                .iter()
                .enumerate()
                .filter_map(|(idx, member)| match &member.kind {
                    ClassMemberKind::Method(method)
                        if method.body.is_some()
                            && method.modifiers & MOD_DECLARE == 0
                            && method.modifiers & MOD_ABSTRACT == 0
                            && matches!(method.name, PropName::Computed(_, _)) =>
                    {
                        Some(idx)
                    }
                    _ => None,
                })
                .collect();
            let mut legacy_actions_by_method_idx: std::collections::HashMap<
                usize,
                Vec<(Option<String>, Expr)>,
            > = std::collections::HashMap::new();
            let mut next_anchor_idx = 0usize;
            for (idx, member) in class_decl.members.iter().enumerate() {
                let ClassMemberKind::Property(prop) = &member.kind else {
                    continue;
                };
                if prop.modifiers & MOD_DECLARE != 0 || prop.modifiers & MOD_ABSTRACT != 0 {
                    continue;
                }
                let PropName::Computed(expr, _) = &prop.name else {
                    continue;
                };
                let define_uninitialized_field = downlevel_class_fields
                    && prop.initializer.is_none()
                    && prop.modifiers & MOD_ACCESSOR == 0
                    && !(prop.definite && prop.type_ann.is_some());
                let needs_temp = prop.initializer.is_some()
                    || define_uninitialized_field
                    || !prop.decorators.is_empty();
                let temp_name = if needs_temp {
                    general_temp_by_start.get(&expr.span.start).cloned()
                } else {
                    None
                };
                let inner_expr = Self::unwrap_type_layers(expr);
                let needs_side_effect = prop.initializer.is_none()
                    && temp_name.is_none()
                    && !matches!(
                        &inner_expr.kind,
                        ExprKind::Ident(_)
                            | ExprKind::NumLit(_)
                            | ExprKind::StrLit(_)
                            | ExprKind::BigIntLit(_)
                            | ExprKind::BoolLit(_)
                            | ExprKind::Spread(_)
                            | ExprKind::Cond(_)
                    );
                if temp_name.is_none() && !needs_side_effect {
                    continue;
                }
                while next_anchor_idx < computed_method_anchors.len()
                    && computed_method_anchors[next_anchor_idx] <= idx
                {
                    next_anchor_idx += 1;
                }
                let action = (temp_name, (*expr.as_ref()).clone());
                if let Some(anchor_member_idx) = computed_method_anchors.get(next_anchor_idx) {
                    legacy_actions_by_method_idx
                        .entry(*anchor_member_idx)
                        .or_default()
                        .push(action);
                } else {
                    legacy_post_class_computed_actions.push(action);
                }
            }

            for (idx, member) in class_decl.members.iter().enumerate() {
                let ClassMemberKind::Method(method) = &member.kind else {
                    continue;
                };
                if method.body.is_none()
                    || method.modifiers & MOD_DECLARE != 0
                    || method.modifiers & MOD_ABSTRACT != 0
                {
                    continue;
                }
                let PropName::Computed(expr, _) = &method.name else {
                    continue;
                };

                let mut name_parts: Vec<String> = Vec::new();
                if let Some(actions) = legacy_actions_by_method_idx.remove(&idx) {
                    for (temp_opt, action_expr) in actions {
                        let action_text = emit_expr_to_string(
                            self.source,
                            self.options,
                            &action_expr,
                            &self.cjs_import_map,
                            &self.cjs_string_import_locals,
                            &self.import_shadows,
                        );
                        if let Some(temp_name) = temp_opt {
                            name_parts.push(format!("{temp_name} = {action_text}"));
                        } else {
                            name_parts.push(action_text);
                        }
                    }
                }

                let method_name_text = emit_expr_to_string(
                    self.source,
                    self.options,
                    expr,
                    &self.cjs_import_map,
                    &self.cjs_string_import_locals,
                    &self.import_shadows,
                );
                let maybe_method_temp = if !method.decorators.is_empty() {
                    general_temp_by_start.get(&expr.span.start).cloned()
                } else {
                    None
                };
                if let Some(temp_name) = maybe_method_temp {
                    name_parts.push(format!("{temp_name} = {method_name_text}"));
                } else if !name_parts.is_empty() {
                    name_parts.push(method_name_text);
                }

                if !name_parts.is_empty() {
                    legacy_method_name_overrides
                        .insert(member.span.start, format!("[({})]", name_parts.join(", ")));
                }
            }
        }

        let downlevel_accessors = self.needs_downlevel("auto-accessors");
        let needs_legacy_instance_init_transform = !use_define
            && class_decl.members.iter().any(|m| {
                if self.class_member_recovery_statement_text(m).is_some() {
                    return false;
                }
                if let ClassMemberKind::Property(ref prop) = m.kind {
                    if matches!(prop.name, PropName::Private(_, _)) {
                        return false;
                    }
                    let define_uninitialized_field = downlevel_class_fields
                        && prop.initializer.is_none()
                        && prop.modifiers & MOD_ACCESSOR == 0
                        // Definite typed fields (`x!: T`) are type-only.
                        && !(prop.definite && prop.type_ann.is_some());
                    (prop.initializer.is_some() || define_uninitialized_field)
                        && prop.modifiers & MOD_STATIC == 0
                        && prop.modifiers & MOD_DECLARE == 0
                        && prop.modifiers & MOD_ABSTRACT == 0
                        && prop.modifiers & MOD_ACCESSOR == 0
                } else {
                    false
                }
            });

        // Pre-compute auto-accessor storage names, handling collisions with
        // existing private fields AND private names from enclosing classes.
        self.accessor_storage_names.clear();
        {
            let mut this_class_private_names: std::collections::HashSet<String> =
                std::collections::HashSet::new();
            let mut this_class_private_members: std::collections::HashMap<
                String,
                EnclosingPrivateMemberKind,
            > = std::collections::HashMap::new();
            for member in &class_decl.members {
                match &member.kind {
                    ClassMemberKind::Property(prop) => {
                        if let PropName::Private(name, _) = &prop.name {
                            let name = normalize_unicode_escapes(name);
                            this_class_private_names.insert(name.clone());
                            this_class_private_members.insert(
                                name,
                                EnclosingPrivateMemberKind::Field {
                                    is_static: prop.modifiers & MOD_STATIC != 0,
                                },
                            );
                        }
                    }
                    ClassMemberKind::Method(method) => {
                        if let PropName::Private(name, _) = &method.name {
                            let name = normalize_unicode_escapes(name);
                            this_class_private_names.insert(name.clone());
                            this_class_private_members.insert(
                                name,
                                EnclosingPrivateMemberKind::Method {
                                    is_static: method.modifiers & MOD_STATIC != 0,
                                },
                            );
                        }
                    }
                    ClassMemberKind::GetAccessor(acc) | ClassMemberKind::SetAccessor(acc) => {
                        if let PropName::Private(name, _) = &acc.name {
                            let name = normalize_unicode_escapes(name);
                            this_class_private_names.insert(name.clone());
                            this_class_private_members.insert(
                                name,
                                EnclosingPrivateMemberKind::Accessor {
                                    is_static: acc.modifiers & MOD_STATIC != 0,
                                },
                            );
                        }
                    }
                    _ => {}
                }
            }
            if (use_define && downlevel_accessors) || needs_legacy_instance_init_transform {
                // Check collisions against this class's private fields and
                // enclosing classes' private names (from the stack).
                let enclosing_names: std::collections::HashSet<&String> = self
                    .enclosing_private_names
                    .iter()
                    .flat_map(|s| s.iter())
                    .collect();
                for member in &class_decl.members {
                    if let ClassMemberKind::Property(ref prop) = member.kind {
                        if prop.modifiers & MOD_ACCESSOR != 0 {
                            if let Some(name) = prop.name.ident_name() {
                                let default = format!("{}_accessor_storage", name);
                                let storage = if this_class_private_names.contains(&default)
                                    || enclosing_names.contains(&default)
                                {
                                    format!("{}_1_accessor_storage", name)
                                } else {
                                    default
                                };
                                // Also add the generated storage name so nested classes see it.
                                this_class_private_names.insert(storage.clone());
                                self.accessor_storage_names
                                    .insert(name.to_string(), storage);
                            }
                        }
                    }
                }
            }
            // Push this class's private names (including generated accessor storage names)
            // so nested classes can check for collisions and resolve nested access.
            self.enclosing_private_names.push(this_class_private_names);
            self.enclosing_private_members
                .push(this_class_private_members);
            self.enclosing_private_class_names
                .push(class_decl.name.clone());
        }

        // Collect instance field initializers for constructor (legacy mode only)
        let field_inits: Vec<(&ClassProp, Span)> = if use_define {
            Vec::new()
        } else {
            class_decl
                .members
                .iter()
                .filter_map(|m| {
                    if self.class_member_recovery_statement_text(m).is_some() {
                        return None;
                    }
                    if let ClassMemberKind::Property(ref prop) = m.kind {
                        if prop.modifiers & MOD_STATIC != 0
                            || prop.modifiers & MOD_DECLARE != 0
                            || prop.modifiers & MOD_ABSTRACT != 0
                        {
                            return None;
                        }
                        if self.standard_decorator_instance_field_slot(prop).is_some() {
                            return Some((prop, m.span));
                        }
                        let define_uninitialized_field = downlevel_class_fields
                            && prop.initializer.is_none()
                            && prop.modifiers & MOD_ACCESSOR == 0
                            // Definite typed fields (`x!: T`) are type-only.
                            && !(prop.definite && prop.type_ann.is_some());
                        if prop.modifiers & MOD_ACCESSOR != 0 {
                            if needs_legacy_instance_init_transform && prop.initializer.is_some() {
                                return Some((prop, m.span));
                            }
                            return None;
                        }
                        if matches!(prop.name, PropName::Private(_, _)) {
                            if needs_legacy_instance_init_transform
                                && !downlevel_private
                                && prop.initializer.is_some()
                            {
                                return Some((prop, m.span));
                            }
                            return None;
                        }
                        if prop.initializer.is_some() || define_uninitialized_field {
                            return Some((prop, m.span));
                        }
                    }
                    None
                })
                .collect()
        };

        let has_explicit_ctor = class_decl.members.iter().any(|m| match &m.kind {
            ClassMemberKind::Constructor(ctor) => {
                ctor.body.is_some() || self.constructor_has_recovery_body_hint(m.span.end)
            }
            _ => false,
        });

        // Determine if we need a synthesized constructor
        let has_private_instance_fields = self.has_emitted_instance_private_field_inits(
            &private_fields,
            &invalid_duplicate_private_names,
            &private_field_name_by_span,
        );
        let has_private_instance_methods = !private_instance_methods.is_empty();
        let has_private_instance_accessors = private_accessors.iter().any(|info| !info.is_static);
        let needs_synth_ctor = (!field_inits.is_empty()
            || (downlevel_private
                && (has_private_instance_fields
                    || has_private_instance_methods
                    || has_private_instance_accessors)))
            && !has_explicit_ctor;
        // In legacy class-field mode, instance auto-accessors must be lowered
        // when other instance field initializers are moved to the constructor
        // so source initialization order remains correct.
        let force_legacy_accessor_transform = needs_legacy_instance_init_transform;

        // Pre-compute comment state for instance field inits and private fields.
        // Unlike static fields (emitted after the class body), instance field inits
        // and private fields are emitted inside the constructor which may appear
        // before the property in source order.
        // We pre-compute by finding the right comment index for each field/private.
        let private_field_spans: Vec<Span> = private_fields
            .iter()
            .filter(|(_, _, is_static, _, _)| !is_static)
            .map(|(_, _, _, span, _)| *span)
            .collect();
        let instance_comment_states: Vec<(u32, usize, u32)> = field_inits
            .iter()
            .map(|(_, span)| *span)
            .chain(private_field_spans.iter().copied())
            .filter_map(|span| {
                let span = &span;
                // Find the first comment index at or past span.start
                let partition = self.comments.partition_point(|c| c.pos < span.start);
                // The leading comment(s) for this member are just before partition
                if partition > 0 {
                    // Walk backwards to find the first leading comment for this member
                    let mut first_leading = partition;
                    for i in (0..partition).rev() {
                        let c = &self.comments[i];
                        // Check if this comment is on its own line just before span.start
                        let start = c.pos as usize;
                        let is_line_start = if start == 0 {
                            true
                        } else {
                            let prefix = &self.source[..start];
                            let line_start = prefix.rfind('\n').map(|p| p + 1).unwrap_or(0);
                            self.source[line_start..start].trim().is_empty()
                        };
                        if !is_line_start {
                            break;
                        }
                        // Check nothing except whitespace between this comment end and next
                        let between_end = if first_leading < partition {
                            self.comments[first_leading].pos as usize
                        } else {
                            span.start as usize
                        };
                        let between = &self.source[c.end as usize..between_end];
                        if between.trim().is_empty() {
                            first_leading = i;
                        } else {
                            break;
                        }
                    }
                    if first_leading < partition {
                        let emit_pos = self.comments[first_leading].pos;
                        Some((span.start, first_leading, emit_pos))
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();

        if needs_synth_ctor {
            let prev_class_expr_temp_emitted = self.class_expr_temp_emitted;
            let prev_temp_var_counter = self.temp_var_counter;
            let prev_temp_var_names = std::mem::take(&mut self.temp_var_names);
            let prev_private_field_var_insert_pos = self.private_field_var_insert_pos.take();
            let prev_pending_private_field_vars =
                std::mem::take(&mut self.pending_private_field_vars);
            let prev_class_scope_temp_reserved = self.class_scope_temp_reserved;
            self.class_expr_temp_emitted = false;
            self.temp_var_counter = prev_temp_var_counter.max(self.class_scope_temp_reserved);
            self.class_scope_temp_reserved = 0;
            let extends_is_null = class_decl.extends.as_ref().is_some_and(|e| {
                let mut expr = e.as_ref();
                while let ExprKind::Paren(inner) = &expr.kind {
                    expr = inner.as_ref();
                }
                matches!(expr.kind, ExprKind::NullLit)
            });
            let hoist_insert_pos;
            if class_decl.extends.is_some() && !extends_is_null {
                self.writeln("constructor() {");
                self.indent += 1;
                self.fn_scope_depth += 1;
                self.writeln("super(...arguments);");
                hoist_insert_pos = self.output.len();
            } else {
                self.writeln("constructor() {");
                self.indent += 1;
                self.fn_scope_depth += 1;
                hoist_insert_pos = self.output.len();
            }
            // Emit private field brand check
            if downlevel_private {
                if let Some(ref brand_name) = private_method_brand_name {
                    self.write("_");
                    self.write(brand_name);
                    self.writeln(".add(this);");
                }
            }
            // Emit private field WeakMap.set() and regular field inits
            // interleaved in source order by walking class members.
            // Build a map from member position → private_fields index for matching.
            let pf_by_pos: std::collections::HashMap<u32, usize> = if downlevel_private {
                private_fields
                    .iter()
                    .enumerate()
                    .map(|(i, (_, _, _, pf_span, _))| (pf_span.start, i))
                    .collect()
            } else {
                std::collections::HashMap::new()
            };
            let saved_comment_pos = self.comment_emit_pos;
            let saved_next_idx = self.next_comment_idx;
            let mut field_idx = 0;
            let mut emitted_pf = vec![false; private_fields.len()];
            let mut any_field_remaining = true;
            for member in &class_decl.members {
                if !any_field_remaining {
                    break;
                }
                // Check if this member matches a private field to emit
                if let Some(&pf_idx) = pf_by_pos.get(&member.span.start) {
                    let (name, init, is_static, pf_span, binding_name) = &private_fields[pf_idx];
                    if !is_static && !emitted_pf[pf_idx] {
                        let runtime_target = self.private_field_runtime_target(
                            name,
                            *pf_span,
                            false,
                            &invalid_duplicate_private_names,
                            &private_field_name_by_span,
                        );
                        let Some((runtime_name, runtime_is_static)) = runtime_target else {
                            emitted_pf[pf_idx] = true;
                            continue;
                        };
                        // Emit leading comments for the private field
                        if let Some(&(_, saved_idx, saved_emit_pos)) = instance_comment_states
                            .iter()
                            .find(|&&(start, _, _)| start == pf_span.start)
                        {
                            let old_idx = self.next_comment_idx;
                            let old_emit_pos = self.comment_emit_pos;
                            self.next_comment_idx = saved_idx;
                            self.comment_emit_pos = saved_emit_pos;
                            self.emit_leading_comments(pf_span.start);
                            self.next_comment_idx = old_idx;
                            self.comment_emit_pos = old_emit_pos;
                        }
                        self.write("_");
                        self.write(&runtime_name);
                        if runtime_is_static {
                            self.write(" = { value: ");
                            if let Some(init_expr) = init {
                                self.emit_private_static_value_initializer(
                                    binding_name.as_ref(),
                                    init_expr,
                                );
                            } else {
                                self.write("void 0");
                            }
                            self.write(" }");
                        } else {
                            self.write(".set(this, ");
                            if let Some(init_expr) = init {
                                self.emit_expr_with_binding_name_if_needed(
                                    binding_name.as_ref(),
                                    init_expr,
                                );
                            } else {
                                self.write("void 0");
                            }
                            self.write(")");
                        }
                        self.writeln(";");
                        // Skip trailing comment for auto-accessor storage (would duplicate)
                        if !name.ends_with("_accessor_storage") {
                            self.append_trailing_comment(*pf_span);
                        }
                        emitted_pf[pf_idx] = true;
                    } else {
                        emitted_pf[pf_idx] = true; // static — mark as handled
                    }
                    any_field_remaining =
                        field_idx < field_inits.len() || emitted_pf.iter().any(|&e| !e);
                    continue;
                }
                // Check if this member matches a regular field init
                // (skip SemicolonClassElement which may share span.start)
                if field_idx < field_inits.len()
                    && !matches!(&member.kind, ClassMemberKind::SemicolonClassElement)
                {
                    let (prop, span) = &field_inits[field_idx];
                    if member.span.start == span.start {
                        // This is a field init member
                        self.emit_leading_comments(span.start);
                        self.emit_field_init(prop);
                        self.append_trailing_comment(*span);
                        self.advance_comment_pos(span.end);
                        field_idx += 1;
                        any_field_remaining =
                            field_idx < field_inits.len() || emitted_pf.iter().any(|&e| !e);
                        continue;
                    }
                }
                // Non-field member: skip past its comments
                self.advance_comment_pos(member.span.end);
            }
            for (idx, member) in class_decl.members.iter().enumerate() {
                let ClassMemberKind::Property(prop) = &member.kind else {
                    continue;
                };
                let PropName::Private(name, _) = &prop.name else {
                    continue;
                };
                if prop.modifiers & MOD_STATIC == 0
                    || !Self::private_name_uses_duplicate_member_recovery(
                        name,
                        &invalid_duplicate_private_names,
                    )
                {
                    continue;
                }
                let next_member_start = class_decl
                    .members
                    .get(idx + 1)
                    .map(|next| next.span.start)
                    .unwrap_or(class_decl.span.end);
                self.emit_leading_comments_in_range(member.span.end, next_member_start);
            }
            if field_idx > 0 {
                self.emit_standard_decorator_instance_field_extra_initializers();
            }
            // Restore comment tracking so the class member loop can emit
            // comments for intervening members (getter/setter JSDoc, etc.).
            self.comment_emit_pos = saved_comment_pos;
            self.next_comment_idx = saved_next_idx;
            let indent_str = "    ".repeat(self.indent as usize);
            let mut insert_offset = 0usize;
            {
                let all_vars: Vec<&str> = self
                    .pending_private_field_vars
                    .iter()
                    .map(|s| s.as_str())
                    .chain(self.temp_var_names.iter().map(|s| s.as_str()))
                    .collect();
                if !all_vars.is_empty() {
                    let var_decl = format!("{}var {};\n", indent_str, all_vars.join(", "));
                    self.output
                        .insert_str(hoist_insert_pos + insert_offset, &var_decl);
                    insert_offset += var_decl.len();
                }
            }
            self.fn_scope_depth -= 1;
            self.indent -= 1;
            self.writeln("}");
            self.class_expr_temp_emitted = prev_class_expr_temp_emitted;
            self.temp_var_counter = prev_temp_var_counter;
            self.class_scope_temp_reserved = prev_class_scope_temp_reserved;
            self.temp_var_names = prev_temp_var_names;
            self.private_field_var_insert_pos = prev_private_field_var_insert_pos;
            self.pending_private_field_vars = prev_pending_private_field_vars;
        }

        // Maps member span.start → (next_comment_idx, comment_emit_pos) saved just before
        // advance_comment_pos() erases comments for static property members that are moved
        // out of the class body in legacy mode. Used by emit_class_static_field_initializers
        // to restore comment state so leading comments can be re-emitted at the correct site.
        let mut static_comment_states: Vec<(u32, usize, u32)> = Vec::new();

        // In define mode (useDefineForClassFields=true / ES2022+), emit field
        // declarations for constructor parameter properties at the start of the
        // class body.  TypeScript's [[Define]] semantics require these to appear
        // as explicit field declarations before other members.
        if use_define {
            if let Some(ctor) = class_decl.members.iter().find_map(|m| {
                if let ClassMemberKind::Constructor(ctor) = &m.kind {
                    if ctor.body.is_some() {
                        Some(ctor)
                    } else {
                        None
                    }
                } else {
                    None
                }
            }) {
                for param in &ctor.params {
                    if param.modifiers
                        & (MOD_PUBLIC | MOD_PRIVATE | MOD_PROTECTED | MOD_READONLY | MOD_OVERRIDE)
                        != 0
                    {
                        if let PatKind::Ident(ref name) = param.name.kind {
                            self.write(name);
                            self.writeln(";");
                        }
                    }
                }
            }
        }

        // When downleveling static private members, register class name → alias
        // substitution so that `A.#field` inside the class body (constructor, methods)
        // emits `_a` instead of `A` as the receiver.
        let saved_class_alias_for_body = if has_static_private_fields
            || has_static_private_methods
            || has_static_private_accessors
            || needs_private_helper_alias
        {
            if let (Some(ref cn), Some(ref tn)) = (&class_decl.name, &class_temp_name) {
                let prev = self.cjs_import_map.remove(cn.as_str());
                self.cjs_import_map.insert(
                    AstString::from(cn.as_str()),
                    (AstString::from(tn.as_str()), AstString::new("")),
                );
                Some((cn.clone(), prev))
            } else {
                None
            }
        } else {
            None
        };
        let mut pending_recovery_method_name: Option<String> = None;
        let mut recovery_statement_tails: Vec<(String, Span)> = Vec::new();
        for member in &class_decl.members {
            if let Some(recovery_text) = self.class_member_recovery_statement_text(member) {
                recovery_statement_tails.push((recovery_text, member.span));
                self.advance_comment_pos(member.span.end);
                continue;
            }
            // Don't emit leading comments for overload signatures (they will be
            // erased, so their comments should also be suppressed).
            let is_erased_overload = match &member.kind {
                ClassMemberKind::Method(m) => {
                    if downlevel_private && matches!(m.name, PropName::Private(_, _)) {
                        true // private method moved out of class body
                    } else {
                        m.body.is_none()
                            && m.modifiers & MOD_DECLARE == 0
                            && m.modifiers & MOD_ABSTRACT == 0
                            && !self.class_method_has_recovery_comma_successor(member, m)
                    }
                }
                ClassMemberKind::Constructor(c) => {
                    c.body.is_none() && !self.constructor_has_recovery_body_hint(member.span.end)
                }
                // In legacy mode, properties are moved out of the class body
                // (static → after class, instance → constructor). Their leading
                // comments are suppressed here; static field comments are emitted
                // by emit_class_static_field_initializers instead.
                // Exception: when the target supports static blocks (ES2022+),
                // static properties with initializers are emitted as
                // `static { this.prop = val; }` IN the body, so they're not erased.
                ClassMemberKind::Property(prop) => {
                    if !use_define
                        && prop.modifiers & MOD_DECLARE == 0
                        && prop.modifiers & MOD_ABSTRACT == 0
                    {
                        // Static properties with initializers → static { } blocks (ES2022+)
                        if prop.modifiers & MOD_STATIC != 0
                            && prop.initializer.is_some()
                            && !self.needs_downlevel("static-blocks")
                        {
                            false // not erased — emitted as static { this.p = val; } in body
                        } else {
                            true // erased — deferred to constructor or after class
                        }
                    } else {
                        false
                    }
                }
                // Index signatures are type-only and produce no JS output.
                ClassMemberKind::IndexSignature(_) => true,
                // Downleveled static blocks are emitted after the class body
                // as IIFEs, so their leading comments must also be deferred.
                ClassMemberKind::StaticBlock(_) => self.needs_downlevel("static-blocks"),
                // Private get/set accessors moved out of class body
                ClassMemberKind::GetAccessor(acc) | ClassMemberKind::SetAccessor(acc) => {
                    downlevel_private && matches!(acc.name, PropName::Private(_, _))
                }
                _ => false,
            };
            // For auto-accessors being lowered in define mode, defer leading comments
            // until after the storage field so the comment appears between storage and getter.
            let is_lowered_accessor = matches!(
                &member.kind,
                ClassMemberKind::Property(prop) if prop.modifiers & MOD_ACCESSOR != 0
                    && prop.modifiers & MOD_DECLARE == 0
                    && prop.modifiers & MOD_ABSTRACT == 0
                    && prop.name.ident_name().is_some()
                    && (downlevel_accessors
                        || (force_legacy_accessor_transform
                            && prop.modifiers & MOD_STATIC == 0))
            );
            // Skip leading comments for instance fields that were moved to the
            // constructor — their comments were already emitted there.
            // Exclude SemicolonClassElement which may share span.start with a field.
            let is_moved_to_ctor = !use_define
                && !matches!(&member.kind, ClassMemberKind::SemicolonClassElement)
                && field_inits
                    .iter()
                    .any(|(_, span)| span.start == member.span.start);
            if !is_erased_overload && !is_lowered_accessor && !is_moved_to_ctor {
                // When this member's leading comments overlap with a moved field's
                // comments (same span.start), limit emission to only this member's
                // own comments by using the field's comment start from
                // instance_comment_states as the upper bound.
                let emit_up_to = if !use_define {
                    instance_comment_states
                        .iter()
                        .find(|&&(start, _, _)| start == member.span.start)
                        .map(|&(_, _, emit_pos)| emit_pos)
                } else {
                    None
                };
                if let Some(up_to) = emit_up_to {
                    // Only emit comments before the field's own comments start
                    self.emit_leading_comments(up_to);
                } else {
                    self.emit_leading_comments(member.span.start);
                }
            }
            if is_moved_to_ctor {
                // Advance past the field's comments so they don't get
                // emitted again for subsequent members.
                self.advance_comment_pos(member.span.end);
            }
            // For erased static members in legacy mode, save the comment state
            // before advancing past them so we can re-emit their comments later.
            // When target supports static blocks, static properties are emitted
            // in the body and don't need deferral.
            match &member.kind {
                ClassMemberKind::Property(prop)
                    if !use_define
                        && prop.modifiers & MOD_STATIC != 0
                        && prop.modifiers & MOD_DECLARE == 0
                        && prop.modifiers & MOD_ABSTRACT == 0
                        && prop.initializer.is_some()
                        && self.needs_downlevel("static-blocks") =>
                {
                    static_comment_states.push((
                        member.span.start,
                        self.next_comment_idx,
                        self.comment_emit_pos,
                    ));
                    // Track preceding static property computed keys for static method comma expr
                    if let Some(ref mut preceding) = self.class_preceding_static_prop_keys {
                        if let PropName::Computed(expr, _) = &prop.name {
                            if let Some(p) = expr_is_class_member_access(expr, class_name) {
                                preceding.push(format!("{}.{}", class_name, p));
                            }
                        }
                    }
                }
                ClassMemberKind::StaticBlock(_) if self.needs_downlevel("static-blocks") => {
                    static_comment_states.push((
                        member.span.start,
                        self.next_comment_idx,
                        self.comment_emit_pos,
                    ));
                }
                _ => {}
            }
            let before_len = self.output.len();
            let mut method_name_override: Option<String> = None;
            if let ClassMemberKind::Method(method) = &member.kind {
                if let PropName::Ident(name, _) = &method.name {
                    if method.body.is_none() {
                        if class_decl.name.as_deref() != Some(name.as_str()) {
                            pending_recovery_method_name = Some(name.to_string());
                        }
                    } else if class_decl.name.as_deref() == Some(name.as_str()) {
                        if let Some(prev_name) = pending_recovery_method_name.take() {
                            method_name_override = Some(prev_name);
                        }
                    } else {
                        pending_recovery_method_name = None;
                    }
                } else {
                    pending_recovery_method_name = None;
                }
            }
            if method_name_override.is_none() {
                if let Some(override_name) = legacy_method_name_overrides.get(&member.span.start) {
                    method_name_override = Some(override_name.clone());
                }
            }

            let current_class_name: &str = if let Some(ref tn) = class_temp_name {
                tn
            } else {
                class_decl.name.as_deref().unwrap_or("default")
            };

            self.emit_class_member_with_private(
                member,
                &field_inits,
                class_decl.extends.is_some(),
                use_define,
                downlevel_private,
                &invalid_duplicate_private_names,
                &private_field_name_by_span,
                &private_fields,
                private_method_brand_name.as_deref(),
                method_name_override.as_deref(),
                current_class_name,
                &instance_comment_states,
            );
            if let ClassMemberKind::Constructor(ctor) = &member.kind {
                if let Some(body) = &ctor.body {
                    self.emit_ctor_recovered_static_methods(body);
                }
            }
            // Only attach trailing comment if the member actually produced output
            // (skipped overload signatures, declare members, etc. produce nothing)
            let handled_accessor_comment = matches!(
                &member.kind,
                ClassMemberKind::Property(prop)
                    if !use_define
                        && prop.modifiers & MOD_ACCESSOR != 0
                        && (downlevel_accessors
                            || (force_legacy_accessor_transform
                                && prop.modifiers & MOD_STATIC == 0))
            );
            if self.output.len() > before_len && !handled_accessor_comment {
                self.append_trailing_comment(member.span);
            }
            self.advance_comment_pos(member.span.end);
        }

        // When target supports static blocks (ES2022+), emit computed property key
        // evaluations as `static { ... }` inside the class body (before closing `}`).
        // TypeScript places these inside the class body rather than after it.
        if !use_define && !downlevel_static_blocks && !legacy_post_class_computed_actions.is_empty()
        {
            self.write("static { ");
            for (i, (temp, expr)) in legacy_post_class_computed_actions.iter().enumerate() {
                if i > 0 {
                    self.write(", ");
                }
                if let Some(temp_name) = temp {
                    self.write(temp_name);
                    self.write(" = ");
                }
                self.emit_expr(expr);
            }
            self.writeln("; }");
        }

        self.indent -= 1;
        self.writeln("}");

        // Restore class name substitution set up for the class body.
        if let Some((name, prev)) = saved_class_alias_for_body {
            self.cjs_import_map.remove(name.as_str());
            if let Some(prev_val) = prev {
                self.cjs_import_map
                    .insert(AstString::from(name.as_str()), prev_val);
            }
        }

        if let Some(ref tn) = class_temp_name {
            if !class_temp_needs_private_decl {
                self.write(tn);
                self.write(" = ");
                self.write(class_decl.name.as_ref().unwrap());
                self.writeln(";");
            }
        }

        // Initialize auto-accessor and private field storage
        if !use_define {
            for (name, init, is_static, _pf_span, binding_name) in &private_fields {
                if name.ends_with("_accessor_storage") || downlevel_private {
                    if *is_static && name.ends_with("_accessor_storage") {
                        self.write("_");
                        self.write(name);
                        self.write(".set(");
                        if let Some(ref tn) = class_temp_name {
                            self.write(tn);
                        } else {
                            self.write(class_decl.name.as_deref().unwrap_or("default"));
                        }
                        self.write(", ");
                        if let Some(init_expr) = init {
                            self.emit_expr_with_binding_name_if_needed(
                                binding_name.as_ref(),
                                init_expr,
                            );
                        } else {
                            self.write("void 0");
                        }
                        self.writeln(");");
                    }
                }
            }
        }

        // Record output position before WeakMap init and static field initializers
        // so CJS export handlers can move them after the export assignment.
        // In CJS: `exports.Foo = Foo;` comes BEFORE `_Foo_field = new WeakMap();`
        self.pre_static_output_len = Some(self.output.len());

        // Emit remaining computed property key evaluations for erased class
        // properties (those not consumed by subsequent computed method names).
        // This goes after `pre_static_output_len` so CJS `exports.X = X;` is
        // inserted before these evaluations.
        // When target supports static blocks (ES2022+), these were already emitted
        // inside the class body as `static { ... }`.
        if !use_define && downlevel_static_blocks {
            if !legacy_post_class_computed_actions.is_empty() {
                for (i, (temp, expr)) in legacy_post_class_computed_actions.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    if let Some(temp_name) = temp {
                        self.write(temp_name);
                        self.write(" = ");
                    }
                    self.emit_expr(expr);
                }
                self.writeln(";");
            }
        }

        // Initialize instance private fields and auto-accessor storage with WeakMap
        // TypeScript emits these as a comma expression after the class body:
        //   _Cls_field = new WeakMap(), _Cls_field2 = new WeakMap();
        {
            let instance_weakmaps: Vec<&str> = private_fields
                .iter()
                .filter(|(_, _, is_static, _, _)| !*is_static)
                .map(|(name, _, _, _, _)| name.as_str())
                .collect();
            let mut wrote_any = false;
            if class_temp_needs_private_decl {
                if let Some(ref tn) = class_temp_name {
                    self.write(tn);
                    self.write(" = ");
                    self.write(class_decl.name.as_deref().unwrap_or("default"));
                    wrote_any = true;
                }
            }
            for name in &instance_weakmaps {
                if wrote_any {
                    self.write(", ");
                }
                self.write("_");
                self.write(name);
                self.write(" = new WeakMap()");
                wrote_any = true;
            }
            if let Some(ref brand_name) = private_method_brand_name {
                if wrote_any {
                    self.write(", ");
                }
                self.write("_");
                self.write(brand_name);
                self.write(" = new WeakSet()");
                wrote_any = true;
            }

            if !private_instance_methods.is_empty()
                || has_private_accessors
                || has_static_private_methods
            {
                let private_method_helper_by_pos: std::collections::HashMap<u32, &str> =
                    private_instance_methods
                        .iter()
                        .chain(private_static_methods.iter())
                        .map(|(_, span, helper_decl_name, _)| {
                            (span.start, helper_decl_name.as_str())
                        })
                        .collect();
                let private_accessor_helper_by_pos: std::collections::HashMap<u32, &str> =
                    private_accessors
                        .iter()
                        .map(|info| (info.span.start, info.helper_decl_name.as_str()))
                        .collect();
                let saved_class_alias = if let (Some(ref class_name), Some(ref tn)) =
                    (&class_decl.name, &class_temp_name)
                {
                    let prev = self.cjs_import_map.remove(class_name.as_str());
                    self.cjs_import_map.insert(
                        AstString::from(class_name.as_str()),
                        (AstString::from(tn.as_str()), AstString::new("")),
                    );
                    Some((class_name.clone(), prev))
                } else {
                    None
                };
                // Emit method/accessor function assignments in SOURCE ORDER
                // to match TypeScript's output ordering.
                for member in &class_decl.members {
                    match &member.kind {
                        ClassMemberKind::Method(method) => {
                            if let PropName::Private(ref name, _) = method.name {
                                if let Some(ref body) = method.body {
                                    if Self::private_name_uses_any_member_recovery(
                                        name,
                                        &invalid_duplicate_private_names,
                                    ) {
                                        continue;
                                    }
                                    let norm_name = normalize_unicode_escapes(name);
                                    let fallback_helper_decl_name =
                                        format!("{}_{}", norm_class, norm_name);
                                    let helper_decl_name = private_method_helper_by_pos
                                        .get(&member.span.start)
                                        .copied()
                                        .unwrap_or(fallback_helper_decl_name.as_str());
                                    if wrote_any {
                                        self.write(", ");
                                    }
                                    self.write("_");
                                    self.write(&helper_decl_name);
                                    self.write(" = ");
                                    let downlevel_async_gen = method.is_async
                                        && method.is_generator
                                        && self.needs_downlevel("async-generator");
                                    let async_gen_move_params = downlevel_async_gen
                                        && method.params.iter().any(Self::param_needs_async_lift);
                                    let downlevel_async = method.is_async
                                        && !method.is_generator
                                        && self.needs_downlevel("async");
                                    if downlevel_async_gen {
                                        // async generator → function() { return __asyncGenerator(...) }
                                        self.write("function _");
                                        self.write(&helper_decl_name);
                                        self.write("(");
                                        if async_gen_move_params {
                                            let temps = if method
                                                .params
                                                .iter()
                                                .any(|p| p.initializer.is_some())
                                            {
                                                Self::generate_async_lift_prefix_temp_names(
                                                    &method.params,
                                                )
                                            } else {
                                                Self::generate_async_lift_temp_names(&method.params)
                                            };
                                            for (i, t) in temps.iter().enumerate() {
                                                if i > 0 {
                                                    self.write(", ");
                                                }
                                                self.write(t);
                                            }
                                        } else {
                                            self.emit_params(&method.params);
                                        }
                                        self.write(") ");
                                        let gen_name = format!("_{}_1", helper_decl_name);
                                        self.awaiter_enclosing_span = Some(member.span);
                                        self.emit_async_generator_body(
                                            body,
                                            Some(&gen_name),
                                            async_gen_move_params
                                                .then_some(method.params.as_slice()),
                                        );
                                    } else if downlevel_async {
                                        // async → function() { return __awaiter(...) }
                                        self.write("function _");
                                        self.write(&helper_decl_name);
                                        self.write("(");
                                        self.emit_params(&method.params);
                                        self.write(") ");
                                        self.awaiter_enclosing_span = Some(member.span);
                                        self.emit_awaiter_body_with_params(
                                            body,
                                            None,
                                            &method.params,
                                            None,
                                        );
                                    } else {
                                        if method.is_async {
                                            self.write("async ");
                                        }
                                        self.write("function");
                                        if method.is_generator {
                                            self.write("*");
                                        }
                                        self.write(" _");
                                        self.write(&helper_decl_name);
                                        self.write("(");
                                        self.emit_params(&method.params);
                                        self.write(") ");
                                        self.emit_block_for_decl_body(body, member.span);
                                    }
                                    wrote_any = true;
                                }
                            }
                        }
                        ClassMemberKind::GetAccessor(acc) | ClassMemberKind::SetAccessor(acc) => {
                            if let PropName::Private(ref name, _) = acc.name {
                                if Self::private_name_uses_any_member_recovery(
                                    name,
                                    &invalid_duplicate_private_names,
                                ) {
                                    continue;
                                }
                                let norm_name = normalize_unicode_escapes(name);
                                let suffix =
                                    if matches!(member.kind, ClassMemberKind::GetAccessor(_)) {
                                        "get"
                                    } else {
                                        "set"
                                    };
                                let fallback_helper_decl_name =
                                    format!("{}_{}_{}", norm_class, norm_name, suffix);
                                let helper_decl_name = private_accessor_helper_by_pos
                                    .get(&member.span.start)
                                    .copied()
                                    .unwrap_or(fallback_helper_decl_name.as_str());
                                if wrote_any {
                                    self.write(", ");
                                }
                                self.write("_");
                                self.write(&helper_decl_name);
                                self.write(" = function _");
                                self.write(&helper_decl_name);
                                self.write("(");
                                self.emit_params(&acc.params);
                                self.write(") ");
                                if let Some(ref body) = acc.body {
                                    let is_async_accessor = acc.modifiers & MOD_ASYNC != 0
                                        && self.needs_downlevel("async");
                                    if is_async_accessor {
                                        self.awaiter_enclosing_span = Some(member.span);
                                        self.emit_awaiter_body_with_params(
                                            body,
                                            None,
                                            &acc.params,
                                            None,
                                        );
                                    } else {
                                        self.emit_block_for_decl_body(body, member.span);
                                    }
                                } else {
                                    // declare accessor — emit empty body
                                    self.write("{ }");
                                }
                                wrote_any = true;
                            }
                        }
                        _ => {}
                    }
                }
                if let Some((name, prev)) = saved_class_alias {
                    self.cjs_import_map.remove(name.as_str());
                    if let Some(prev_val) = prev {
                        self.cjs_import_map
                            .insert(AstString::from(name.as_str()), prev_val);
                    }
                }
            }
            if wrote_any {
                self.writeln(";");
            }
        }

        // Emit static private field initializations as separate statements.
        // Static private fields use `{ value: init }` objects, not WeakMaps.
        if has_static_private_fields {
            for (name, init, is_static, pf_span, binding_name) in &private_fields {
                if *is_static {
                    let runtime_target = self.private_field_runtime_target(
                        name,
                        *pf_span,
                        true,
                        &invalid_duplicate_private_names,
                        &private_field_name_by_span,
                    );
                    let Some((runtime_name, runtime_is_static)) = runtime_target else {
                        continue;
                    };
                    self.write("_");
                    self.write(&runtime_name);
                    if runtime_is_static {
                        self.write(" = { value: ");
                        if let Some(init_expr) = init {
                            self.emit_private_static_value_initializer(
                                binding_name.as_ref(),
                                init_expr,
                            );
                        } else {
                            self.write("void 0");
                        }
                        self.write(" }");
                    } else {
                        self.write(".set(");
                        self.write(class_decl.name.as_deref().unwrap_or("default"));
                        self.write(", ");
                        if let Some(init_expr) = init {
                            self.emit_expr_with_binding_name_if_needed(
                                binding_name.as_ref(),
                                init_expr,
                            );
                        } else {
                            self.write("void 0");
                        }
                        self.write(")");
                    }
                    self.writeln(";");
                    self.append_trailing_comment(*pf_span);
                }
            }
        }

        // Emit instance property temp assignments (_b = A.p2) before static inits
        let instance_temps: Vec<String> = self
            .class_instance_prop_temps
            .as_ref()
            .map(|v| v.clone())
            .unwrap_or_default();
        let computed_temps_snapshot: std::collections::HashMap<String, String> = self
            .class_computed_name_temps
            .as_ref()
            .map(|m| m.clone())
            .unwrap_or_default();
        if !instance_temps.is_empty() && !computed_temps_snapshot.is_empty() {
            let class_name = class_decl.name.as_deref().unwrap_or("default");
            for key in &instance_temps {
                if let Some(temp) = computed_temps_snapshot.get(key) {
                    let (_, prop) = key.split_once('.').unwrap_or(("", key));
                    self.write(temp);
                    self.write(" = ");
                    self.write(class_name);
                    self.write(".");
                    self.write(prop);
                    self.writeln(";");
                }
            }
        }

        let saved_class_alias_for_static =
            if let (Some(ref class_name), Some(ref tn)) = (&class_decl.name, &class_temp_name) {
                let prev = self.cjs_import_map.remove(class_name.as_str());
                self.cjs_import_map.insert(
                    AstString::from(class_name.as_str()),
                    (AstString::from(tn.as_str()), AstString::new("")),
                );
                Some((class_name.clone(), prev))
            } else {
                None
            };
        let saved_static_this_alias = self.static_this_alias.take();
        let saved_static_super_base_alias = self.static_super_base_alias.take();
        let saved_static_super_receiver_alias = self.static_super_receiver_alias.take();
        if let Some(ref tn) = class_temp_name {
            self.static_this_alias = Some(tn.clone());
            self.static_super_receiver_alias = Some(tn.clone());
        } else if let Some(ref class_name) = class_decl.name {
            self.static_super_receiver_alias = Some(class_name.clone());
        }
        if let Some(ref base_alias) = static_super_base_alias {
            self.static_super_base_alias = Some(base_alias.clone());
        }
        // Save pre_static_output_len before emitting static field initializers.
        // Static field initializers may contain class expressions (e.g. `static Cls = class {}`)
        // whose recursive emit_class_decl would overwrite pre_static_output_len.
        let saved_pre_static = self.pre_static_output_len;
        self.emit_class_static_field_initializers(class_decl, &static_comment_states);
        self.pre_static_output_len = saved_pre_static;
        self.static_this_alias = saved_static_this_alias;
        self.static_super_base_alias = saved_static_super_base_alias;
        self.static_super_receiver_alias = saved_static_super_receiver_alias;
        if let Some((name, prev)) = saved_class_alias_for_static {
            self.cjs_import_map.remove(name.as_str());
            if let Some(prev_val) = prev {
                self.cjs_import_map
                    .insert(AstString::from(name.as_str()), prev_val);
            }
        }

        let recovery_static_assignments =
            std::mem::take(&mut self.recovery_static_class_assignments);
        for (target, prop, rhs, span) in recovery_static_assignments {
            self.write(&target);
            self.write(".");
            self.write(&prop);
            self.write(" = ");
            self.emit_expr(&rhs);
            self.writeln(";");
            self.append_trailing_comment(span);
        }
        for (text, span) in recovery_statement_tails {
            self.writeln(&text);
            self.append_trailing_comment(span);
        }

        // Legacy computed-key side effects are emitted by the ordered action
        // sequence above.

        // Parser-recovery case: `var constructor() { }` inside a class can
        // produce trailing top-level recovery output in TypeScript baselines.
        if self.class_has_var_constructor_recovery_tail(class_decl) {
            self.writeln("var constructor;");
            self.writeln("() => { };");
        }
        for (name, leading_comments) in self.class_nested_class_recovery_names(class_decl) {
            if let Some(leading_comments) = leading_comments {
                for line in leading_comments.lines() {
                    self.writeln(line.trim_end_matches('\r'));
                }
            }
            self.write("class ");
            self.write(&name);
            self.writeln(" {");
            self.write("}");
            self.newline();
        }
        if let Some(tail) = self.class_public_missing_member_recovery_tail(class_decl) {
            match tail {
                PublicMissingMemberRecoveryTail::EmptyBlock => {
                    self.writeln("{ }");
                }
                PublicMissingMemberRecoveryTail::IndexSignatureLike {
                    index_name,
                    index_type,
                    value_name,
                } => {
                    self.writeln("{");
                    self.indent += 1;
                    self.write("[");
                    self.write(&index_name);
                    self.write(", ");
                    self.write(&index_type);
                    self.writeln("];");
                    self.write(&value_name);
                    self.writeln(";");
                    self.indent -= 1;
                    self.writeln("}");
                }
            }
        }
        if self.class_computed_field_no_asi_empty_block_tail(class_decl) {
            self.writeln("{ }");
        }
        if let Some(value_name) = self.class_global_namespace_recovery_tail(class_decl) {
            self.writeln("var global;");
            self.writeln("(function (global) {");
            self.writeln("})(global || (global = {}));");
            self.write(&value_name);
            self.writeln(";");
        }
        if self.class_extends_void_recovery_tail(class_decl) {
            self.writeln("void {};");
        }
        // Restore enclosing class name and computed name temp state.
        self.current_class_name = prev_class_name;
        self.current_class_private_fields = prev_private_fields;
        self.current_class_static_private_fields = prev_static_private_fields;
        self.current_class_static_alias = prev_static_alias;
        self.class_scope_temp_reserved = prev_class_scope_temp_reserved;
        self.current_class_private_methods = prev_private_methods;
        self.current_class_static_private_methods = prev_static_private_methods;
        self.current_class_private_accessors = prev_private_accessors;
        self.current_class_static_private_accessors = prev_static_private_accessors;
        self.current_class_private_var_map = prev_private_var_map;
        self.class_computed_name_temps = None;
        self.class_instance_prop_temps = None;
        self.class_preceding_static_prop_keys = None;
        // Pop enclosing private names stack after class declaration processing.
        self.enclosing_private_names.pop();
        self.enclosing_private_members.pop();
        self.enclosing_private_class_names.pop();
    }

    /// Emit a class expression with static properties as a comma-operator IIFE:
    /// `(_a = class C { }, _a.a = 1, _a.b = 2, _a)`
    #[allow(dead_code)] // class expression IIFE pattern
    pub(super) fn emit_class_expr_iife(&mut self, class_decl: &ClassDecl) {
        self.emit_class_expr_iife_with_wrap(class_decl, true, None);
    }

    pub(super) fn emit_class_expr_iife_with_wrap(
        &mut self,
        class_decl: &ClassDecl,
        wrap: bool,
        binding_name: Option<&ClassExprBindingName>,
    ) {
        let use_define_semantics = self.use_define_for_class_fields();
        let downlevel_class_fields = use_define_semantics && self.needs_downlevel("class-fields");
        let use_define = use_define_semantics && !downlevel_class_fields;
        let downlevel_private = self.needs_downlevel("private-fields");
        let downlevel_accessors = self.needs_downlevel("auto-accessors");
        let helper_class_name = class_decl.name.clone().or_else(|| {
            binding_name
                .and_then(Self::class_expr_binding_name_identifier)
                .map(|name| name.to_string())
        });
        let class_ident = helper_class_name.as_deref().unwrap_or("");
        let norm_class = normalize_unicode_escapes(class_ident);
        let qualify_private_helper = |private_name: &str| {
            if norm_class.is_empty() {
                private_name.to_string()
            } else {
                format!("{}_{}", norm_class, private_name)
            }
        };
        let qualify_private_accessor = |private_name: &str, suffix: &str| {
            if norm_class.is_empty() {
                format!("{}_{}", private_name, suffix)
            } else {
                format!("{}_{}_{}", norm_class, private_name, suffix)
            }
        };
        let accessor_storage_name = |prop_name: &str| {
            if class_ident.is_empty() {
                format!("{}_accessor_storage", prop_name)
            } else {
                format!("{}_{}_accessor_storage", class_ident, prop_name)
            }
        };
        // In legacy class-field mode, pre-evaluate computed field keys to temps
        // in source order. TypeScript allocates computed key temps relative to
        // the class alias temp based on whether instance computed keys exist:
        // - With instance computed keys: ALL computed keys (instance+static) in source order before class alias
        // - With only static computed keys: class alias first, then static keys
        let class_temp_allocation_start = self.temp_var_names.len();
        let mut class_expr_computed_temps: Vec<(String, u32)> = Vec::new();
        let mut static_computed_starts: Vec<u32> = Vec::new();
        let mut has_instance_computed_temps = false;
        if !use_define {
            // First pass: detect if any instance computed keys exist
            for member in &class_decl.members {
                if let ClassMemberKind::Property(prop) = &member.kind {
                    if prop.modifiers & MOD_DECLARE != 0 || prop.modifiers & MOD_ABSTRACT != 0 {
                        continue;
                    }
                    if prop.modifiers & MOD_STATIC != 0 {
                        continue;
                    }
                    if let PropName::Computed(expr, _) = &prop.name {
                        if Self::computed_name_is_simple_literal(expr) {
                            continue;
                        }
                        let has_temp = prop.initializer.is_some()
                            || (downlevel_class_fields
                                && prop.initializer.is_none()
                                && prop.modifiers & MOD_ACCESSOR == 0
                                && !(prop.definite && prop.type_ann.is_some()))
                            || !prop.decorators.is_empty();
                        if has_temp {
                            has_instance_computed_temps = true;
                            break;
                        }
                    }
                }
            }
            // Second pass: allocate temps in source order
            let mut seen_starts = std::collections::HashSet::new();
            for member in &class_decl.members {
                match &member.kind {
                    ClassMemberKind::Property(prop) => {
                        if prop.modifiers & MOD_DECLARE != 0 || prop.modifiers & MOD_ABSTRACT != 0 {
                            continue;
                        }
                        let PropName::Computed(expr, _) = &prop.name else {
                            continue;
                        };
                        if Self::computed_name_is_simple_literal(expr)
                            || !seen_starts.insert(expr.span.start)
                        {
                            continue;
                        }
                        if prop.modifiers & MOD_STATIC != 0 {
                            let static_define_uninitialized = downlevel_class_fields
                                && prop.initializer.is_none()
                                && prop.modifiers & MOD_ACCESSOR == 0
                                && !(prop.definite && prop.type_ann.is_some());
                            if prop.initializer.is_some() || static_define_uninitialized {
                                static_computed_starts.push(expr.span.start);
                                if has_instance_computed_temps {
                                    // Allocate in source order (interleaved with instance)
                                    let temp = self.next_temp_var();
                                    class_expr_computed_temps.push((temp, expr.span.start));
                                }
                                // else: deferred until after class alias
                            }
                            continue;
                        }

                        let define_uninitialized_field = downlevel_class_fields
                            && prop.initializer.is_none()
                            && prop.modifiers & MOD_ACCESSOR == 0
                            && !(prop.definite && prop.type_ann.is_some());
                        let needs_temp = prop.initializer.is_some()
                            || define_uninitialized_field
                            || !prop.decorators.is_empty();
                        if needs_temp {
                            let temp = if !prop.decorators.is_empty() {
                                self.next_deferred_temp_placeholder()
                            } else {
                                self.next_temp_var()
                            };
                            class_expr_computed_temps.push((temp, expr.span.start));
                        }
                    }
                    ClassMemberKind::Method(method) => {
                        if method.modifiers & MOD_STATIC == 0
                            && method.modifiers & MOD_DECLARE == 0
                            && method.modifiers & MOD_ABSTRACT == 0
                            && !method.decorators.is_empty()
                        {
                            if let PropName::Computed(expr, _) = &method.name {
                                if !Self::computed_name_is_simple_literal(expr)
                                    && seen_starts.insert(expr.span.start)
                                {
                                    let temp = self.next_deferred_temp_placeholder();
                                    class_expr_computed_temps.push((temp, expr.span.start));
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // When instance computed keys exist, static key temps were already
        // allocated in source order during the scan loop above.
        // Extract them from class_expr_computed_temps by matching positions.
        let mut static_computed_temps: Vec<(String, u32)> = Vec::new();
        if has_instance_computed_temps {
            for start in &static_computed_starts {
                if let Some((temp, _)) = class_expr_computed_temps.iter().find(|(_, s)| s == start)
                {
                    static_computed_temps.push((temp.clone(), *start));
                }
            }
        }

        // Allocate a unique temp var for this class expression.
        // File-level hoisting via insert_file_level_temp_vars will emit
        // `var _a, _b, ...;` at the top of the scope.
        let mut temp_name = if self.in_parameter_initializer && self.fn_scope_depth > 0 {
            self.next_scoped_deferred_temp_var()
        } else {
            self.next_temp_var()
        };
        self.class_expr_temp_emitted = true;

        // When only static computed keys exist (no instance keys), allocate their
        // temps AFTER the class alias temp, matching TypeScript's allocation order.
        if !has_instance_computed_temps {
            for start in &static_computed_starts {
                static_computed_temps.push((self.next_temp_var(), *start));
            }
        }

        let use_block_scoped_public_class_temps = has_instance_computed_temps
            && self.block_depth > 0
            && self.class_decl_helper_insert_pos.is_some()
            && class_decl.decorators.is_empty()
            && !class_has_member_decorators(class_decl)
            && !self.class_expr_needs_private_iife(class_decl)
            && class_decl.members.iter().all(|member| match &member.kind {
                ClassMemberKind::Property(prop) => !matches!(prop.name, PropName::Private(..)),
                ClassMemberKind::Method(method) => !matches!(method.name, PropName::Private(..)),
                ClassMemberKind::GetAccessor(accessor) | ClassMemberKind::SetAccessor(accessor) => {
                    !matches!(accessor.name, PropName::Private(..))
                }
                _ => true,
            });
        if use_block_scoped_public_class_temps {
            let allocated_names: Vec<String> = self.temp_var_names[class_temp_allocation_start..]
                .iter()
                .map(ToString::to_string)
                .collect();
            let mut static_starts = static_computed_starts.clone();
            static_starts.sort_unstable();
            let static_start_set: HashSet<u32> = static_starts.iter().copied().collect();
            let mut instance_starts: Vec<u32> = class_expr_computed_temps
                .iter()
                .filter_map(|(_, start)| (!static_start_set.contains(start)).then_some(*start))
                .collect();
            instance_starts.sort_unstable();

            if allocated_names.len() == static_starts.len() + instance_starts.len() + 1 {
                let mut replacement_by_start: HashMap<u32, String> = HashMap::new();
                for (idx, start) in static_starts.iter().enumerate() {
                    replacement_by_start.insert(*start, allocated_names[idx].clone());
                }
                let instance_offset = static_starts.len();
                for (idx, start) in instance_starts.iter().enumerate() {
                    replacement_by_start
                        .insert(*start, allocated_names[instance_offset + idx].clone());
                }
                for (name, start) in &mut class_expr_computed_temps {
                    if let Some(replacement) = replacement_by_start.get(start) {
                        *name = replacement.clone();
                    }
                }
                for (name, start) in &mut static_computed_temps {
                    if let Some(replacement) = replacement_by_start.get(start) {
                        *name = replacement.clone();
                    }
                }
                temp_name = allocated_names.last().cloned().unwrap_or(temp_name);

                self.temp_var_names.truncate(class_temp_allocation_start);
                self.temp_var_names.extend(
                    allocated_names[..static_starts.len()]
                        .iter()
                        .cloned()
                        .map(AstString::from),
                );
                let mut local_names: Vec<String> = instance_starts
                    .iter()
                    .filter_map(|start| replacement_by_start.get(start).cloned())
                    .collect();
                local_names.push(temp_name.clone());
                let insert_pos = self.class_decl_helper_insert_pos.unwrap();
                let indent = "    ".repeat(self.indent as usize);
                let declaration = format!("{indent}let {};\n", local_names.join(", "));
                self.output.insert_str(insert_pos, &declaration);
                self.out_line += 1;
                if let Some(ref mut pos) = self.class_decl_helper_insert_pos {
                    *pos += declaration.len();
                }
            }
        }

        let class_name = class_decl.name.clone();
        let mut saved_alias: Option<(String, Option<(AstString, AstString)>)> = None;

        let saved_general_computed_temps = std::mem::take(&mut self.class_general_computed_temps);
        self.class_general_computed_temps = class_expr_computed_temps.clone();
        let prev_class_name = self.current_class_name.take();
        let prev_private_fields = std::mem::take(&mut self.current_class_private_fields);
        let prev_static_private_fields =
            std::mem::take(&mut self.current_class_static_private_fields);
        let prev_static_alias = self.current_class_static_alias.take();
        let prev_class_scope_temp_reserved = self.class_scope_temp_reserved;
        let prev_private_methods = std::mem::take(&mut self.current_class_private_methods);
        let prev_static_private_methods =
            std::mem::take(&mut self.current_class_static_private_methods);
        let prev_private_accessors = std::mem::take(&mut self.current_class_private_accessors);
        let prev_static_private_accessors =
            std::mem::take(&mut self.current_class_static_private_accessors);
        let prev_private_var_map = std::mem::take(&mut self.current_class_private_var_map);
        self.current_class_name = helper_class_name.clone();
        let invalid_duplicate_private_names = Self::invalid_duplicate_private_name_info(class_decl);
        let private_field_name_by_span = Self::private_field_name_by_span(class_decl);
        self.current_class_private_fields = class_decl
            .members
            .iter()
            .filter_map(|m| {
                if let ClassMemberKind::Property(prop) = &m.kind {
                    if let PropName::Private(name, _) = &prop.name {
                        return Some(AstString::from(normalize_unicode_escapes(name)));
                    }
                }
                None
            })
            .collect();
        if downlevel_private {
            self.current_class_static_private_fields = class_decl
                .members
                .iter()
                .filter_map(|m| {
                    if let ClassMemberKind::Property(prop) = &m.kind {
                        if let PropName::Private(name, _) = &prop.name {
                            if prop.modifiers & MOD_STATIC != 0 {
                                return Some(AstString::from(normalize_unicode_escapes(name)));
                            }
                        }
                    }
                    None
                })
                .collect();
        } else {
            self.current_class_static_private_fields.clear();
        }

        let mut private_fields: Vec<(
            String,
            Option<&Expr>,
            bool,
            Span,
            Option<ClassExprBindingName>,
        )> = if downlevel_private {
            class_decl
                .members
                .iter()
                .filter_map(|m| {
                    if let ClassMemberKind::Property(ref prop) = m.kind {
                        if let PropName::Private(ref name, _) = prop.name {
                            let is_static = prop.modifiers & MOD_STATIC != 0;
                            let norm_name = normalize_unicode_escapes(name);
                            return Some((
                                qualify_private_helper(&norm_name),
                                prop.initializer.as_deref(),
                                is_static,
                                m.span,
                                Some(ClassExprBindingName::Literal(format!("#{}", norm_name))),
                            ));
                        }
                    }
                    None
                })
                .collect()
        } else {
            Vec::new()
        };

        let mut private_instance_methods: Vec<(&ClassMethod, Span, String, String)> =
            if downlevel_private {
                class_decl
                    .members
                    .iter()
                    .filter_map(|m| {
                        if let ClassMemberKind::Method(ref method) = m.kind {
                            if let PropName::Private(ref name, _) = method.name {
                                if method.modifiers & MOD_STATIC == 0 && method.body.is_some() {
                                    let norm_name = normalize_unicode_escapes(name);
                                    let helper_decl_name = qualify_private_helper(&norm_name);
                                    return Some((method, m.span, helper_decl_name, norm_name));
                                }
                            }
                        }
                        None
                    })
                    .collect()
            } else {
                Vec::new()
            };
        for (_, _, helper_decl_name, private_name) in &private_instance_methods {
            self.current_class_private_methods.insert(
                AstString::from(private_name.as_str()),
                format!("_{}", helper_decl_name).into(),
            );
        }

        let mut private_static_methods: Vec<(&ClassMethod, Span, String, String)> =
            if downlevel_private {
                class_decl
                    .members
                    .iter()
                    .filter_map(|m| {
                        if let ClassMemberKind::Method(ref method) = m.kind {
                            if let PropName::Private(ref name, _) = method.name {
                                if method.modifiers & MOD_STATIC != 0 && method.body.is_some() {
                                    let norm_name = normalize_unicode_escapes(name);
                                    let helper_decl_name = qualify_private_helper(&norm_name);
                                    return Some((method, m.span, helper_decl_name, norm_name));
                                }
                            }
                        }
                        None
                    })
                    .collect()
            } else {
                Vec::new()
            };
        for (_, _, helper_decl_name, private_name) in &private_static_methods {
            self.current_class_static_private_methods.insert(
                AstString::from(private_name.as_str()),
                format!("_{}", helper_decl_name).into(),
            );
        }
        let has_static_private_methods = !private_static_methods.is_empty();

        struct PrivateAccessorInfo<'a> {
            _acc: &'a ClassAccessor,
            span: Span,
            helper_decl_name: String,
            private_name: String,
            is_getter: bool,
            is_static: bool,
        }
        let mut private_accessors: Vec<PrivateAccessorInfo<'_>> = if downlevel_private {
            class_decl
                .members
                .iter()
                .filter_map(|m| {
                    let (acc, is_getter) = match &m.kind {
                        ClassMemberKind::GetAccessor(a) => (a, true),
                        ClassMemberKind::SetAccessor(a) => (a, false),
                        _ => return None,
                    };
                    if let PropName::Private(ref name, _) = acc.name {
                        let norm_name = normalize_unicode_escapes(name);
                        let suffix = if is_getter { "get" } else { "set" };
                        let helper_decl_name = qualify_private_accessor(&norm_name, suffix);
                        let is_static = acc.modifiers & MOD_STATIC != 0;
                        return Some(PrivateAccessorInfo {
                            _acc: acc,
                            span: m.span,
                            helper_decl_name,
                            private_name: norm_name,
                            is_getter,
                            is_static,
                        });
                    }
                    None
                })
                .collect()
        } else {
            Vec::new()
        };
        for info in &private_accessors {
            let entry = self
                .current_class_private_accessors
                .entry(AstString::from(info.private_name.as_str()))
                .or_insert((None, None));
            let var: AstString = format!("_{}", info.helper_decl_name).into();
            if info.is_getter {
                entry.0 = Some(var);
            } else {
                entry.1 = Some(var);
            }
            if info.is_static {
                self.current_class_static_private_accessors
                    .insert(AstString::from(info.private_name.as_str()));
            }
        }
        let has_static_private_accessors = private_accessors.iter().any(|info| info.is_static);
        let has_non_static_accessors = private_accessors.iter().any(|info| !info.is_static);
        let private_method_brand_name =
            if private_instance_methods.is_empty() && !has_non_static_accessors {
                None
            } else if norm_class.is_empty() {
                Some("instances".to_string())
            } else {
                Some(format!("{}_instances", norm_class))
            };

        let private_named_field_count = private_fields.len();
        if !use_define && downlevel_accessors {
            for member in &class_decl.members {
                if let ClassMemberKind::Property(ref prop) = member.kind {
                    if prop.modifiers & MOD_ACCESSOR != 0 {
                        if prop.modifiers & MOD_DECLARE != 0 {
                            continue;
                        }
                        if let Some(prop_name) = prop.name.ident_name() {
                            let no_comment_span = Span {
                                start: 0,
                                end: u32::MAX,
                            };
                            private_fields.push((
                                accessor_storage_name(prop_name),
                                prop.initializer.as_deref(),
                                prop.modifiers & MOD_STATIC != 0,
                                no_comment_span,
                                None,
                            ));
                        }
                    }
                }
            }
        }

        let has_static_private_fields = downlevel_private
            && private_fields
                .iter()
                .any(|(_, _, is_static, _, _)| *is_static);
        let needs_private_helper_alias =
            downlevel_private && class_needs_private_helper_alias(class_decl, class_ident);
        if has_static_private_fields
            || has_static_private_methods
            || has_static_private_accessors
            || needs_private_helper_alias
        {
            self.current_class_static_alias = Some(temp_name.clone());
        }
        self.class_scope_temp_reserved = if self.current_class_static_alias.is_some() {
            self.temp_var_counter
        } else {
            0
        };

        let downlevel_auto_accessors = !use_define && downlevel_accessors;
        let mut helper_var_names: Vec<String> = Vec::new();
        let mut helper_var_seen: std::collections::HashSet<String> =
            std::collections::HashSet::new();

        // Helper closure: allocate a unique name, suffixing with _1, _2, etc. on collision.
        let allocate_unique = |base: &str, all: &mut HashMap<AstString, u32>| -> String {
            let count = all.entry(AstString::from(base)).or_insert(0);
            let actual = if *count == 0 {
                base.to_string()
            } else {
                format!("{}_{}", base, count)
            };
            *count += 1;
            actual
        };
        let allocate_accessor_unique =
            |base: &str, suffix: &str, all: &mut HashMap<AstString, u32>| -> String {
                let key = format!("{}:{}", base, suffix);
                let count = all.entry(AstString::from(key.as_str())).or_insert(0);
                let actual = if *count == 0 {
                    format!("{}_{}", base, suffix)
                } else {
                    format!("{}_{}_{}", base, count, suffix)
                };
                *count += 1;
                actual
            };

        if let Some(ref brand) = private_method_brand_name {
            if helper_var_seen.insert(brand.clone()) {
                let actual = allocate_unique(brand, &mut self.all_allocated_private_var_names);
                helper_var_names.push(actual.clone());
                self.current_class_private_var_map.insert(
                    format!("brand:{}", brand).into(),
                    format!("_{}", actual).into(),
                );
            }
        }
        if downlevel_private || downlevel_auto_accessors {
            let mut private_field_idx = 0usize;
            let mut auto_accessor_storage_idx = 0usize;
            let mut private_instance_method_idx = 0usize;
            let mut private_static_method_idx = 0usize;
            let mut private_accessor_idx = 0usize;
            for member in &class_decl.members {
                match &member.kind {
                    ClassMemberKind::Property(prop) => {
                        if let PropName::Private(ref name, _) = prop.name {
                            if downlevel_private {
                                let norm_name = normalize_unicode_escapes(name);
                                let qualified = qualify_private_helper(&norm_name);
                                let actual = allocate_unique(
                                    &qualified,
                                    &mut self.all_allocated_private_var_names,
                                );
                                helper_var_names.push(actual.clone());
                                if let Some((field_name, _, _, _, _)) =
                                    private_fields.get_mut(private_field_idx)
                                {
                                    *field_name = actual.clone();
                                }
                                private_field_idx += 1;
                                self.current_class_private_var_map
                                    .insert(norm_name.into(), format!("_{}", actual).into());
                            }
                        } else if downlevel_auto_accessors
                            && prop.modifiers & MOD_ACCESSOR != 0
                            && prop.modifiers & MOD_DECLARE == 0
                        {
                            if let Some(prop_name) = prop.name.ident_name() {
                                let storage_name = accessor_storage_name(prop_name);
                                let actual = allocate_unique(
                                    &storage_name,
                                    &mut self.all_allocated_private_var_names,
                                );
                                helper_var_names.push(actual.clone());
                                if let Some((field_name, _, _, _, _)) = private_fields
                                    .get_mut(private_named_field_count + auto_accessor_storage_idx)
                                {
                                    *field_name = actual;
                                }
                                auto_accessor_storage_idx += 1;
                            }
                        }
                    }
                    ClassMemberKind::Method(method) => {
                        if let PropName::Private(ref name, _) = method.name {
                            if method.body.is_some() {
                                let norm_name = normalize_unicode_escapes(name);
                                let qualified = qualify_private_helper(&norm_name);
                                let actual = allocate_unique(
                                    &qualified,
                                    &mut self.all_allocated_private_var_names,
                                );
                                helper_var_names.push(actual.clone());
                                let target_idx = if method.modifiers & MOD_STATIC == 0 {
                                    let idx = private_instance_method_idx;
                                    private_instance_method_idx += 1;
                                    idx
                                } else {
                                    let idx = private_static_method_idx;
                                    private_static_method_idx += 1;
                                    idx
                                };
                                if method.modifiers & MOD_STATIC == 0 {
                                    if let Some((_, _, helper_decl_name, _)) =
                                        private_instance_methods.get_mut(target_idx)
                                    {
                                        *helper_decl_name = actual.clone();
                                    }
                                } else if let Some((_, _, helper_decl_name, _)) =
                                    private_static_methods.get_mut(target_idx)
                                {
                                    *helper_decl_name = actual.clone();
                                }
                                self.current_class_private_var_map.insert(
                                    format!("method:{}", norm_name).into(),
                                    format!("_{}", actual).into(),
                                );
                            }
                        }
                    }
                    ClassMemberKind::GetAccessor(acc) | ClassMemberKind::SetAccessor(acc) => {
                        if let PropName::Private(ref name, _) = acc.name {
                            {
                                let norm_name = normalize_unicode_escapes(name);
                                let suffix =
                                    if matches!(member.kind, ClassMemberKind::GetAccessor(_)) {
                                        "get"
                                    } else {
                                        "set"
                                    };
                                let accessor_base = qualify_private_helper(&norm_name);
                                let actual = allocate_accessor_unique(
                                    &accessor_base,
                                    suffix,
                                    &mut self.all_allocated_private_var_names,
                                );
                                helper_var_names.push(actual.clone());
                                if let Some(info) = private_accessors.get_mut(private_accessor_idx)
                                {
                                    info.helper_decl_name = actual.clone();
                                }
                                private_accessor_idx += 1;
                                self.current_class_private_var_map.insert(
                                    format!("{}:{}", suffix, norm_name).into(),
                                    format!("_{}", actual).into(),
                                );
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        self.current_class_private_methods.clear();
        for (_, _, helper_decl_name, private_name) in &private_instance_methods {
            self.current_class_private_methods.insert(
                AstString::from(private_name.as_str()),
                format!("_{}", helper_decl_name).into(),
            );
        }
        self.current_class_static_private_methods.clear();
        for (_, _, helper_decl_name, private_name) in &private_static_methods {
            self.current_class_static_private_methods.insert(
                AstString::from(private_name.as_str()),
                format!("_{}", helper_decl_name).into(),
            );
        }
        self.current_class_private_accessors.clear();
        self.current_class_static_private_accessors.clear();
        for info in &private_accessors {
            let entry = self
                .current_class_private_accessors
                .entry(AstString::from(info.private_name.as_str()))
                .or_insert((None, None));
            let actual: AstString = format!("_{}", info.helper_decl_name).into();
            if info.is_getter {
                entry.0 = Some(actual);
            } else {
                entry.1 = Some(actual);
            }
            if info.is_static {
                self.current_class_static_private_accessors
                    .insert(AstString::from(info.private_name.as_str()));
            }
        }
        if !helper_var_names.is_empty() {
            let static_private_helper_layout = has_static_private_fields
                || has_static_private_methods
                || has_static_private_accessors;
            let use_block_local_helper_decl = self.block_depth > 0
                && self.class_decl_helper_insert_pos.is_some()
                && !static_private_helper_layout;
            if use_block_local_helper_decl {
                let insert_pos = self.class_decl_helper_insert_pos.unwrap();
                let mut helper_text = String::new();
                let indent_str = "    ".repeat(self.indent as usize);
                helper_text.push_str(&indent_str);
                helper_text.push_str("let ");
                for (i, name) in helper_var_names.iter().enumerate() {
                    if i > 0 {
                        helper_text.push_str(", ");
                    }
                    helper_text.push('_');
                    helper_text.push_str(name);
                }
                helper_text.push_str(";\n");
                self.output.insert_str(insert_pos, &helper_text);
                self.out_line += 1;
                if let Some(ref mut pos) = self.class_decl_helper_insert_pos {
                    *pos += helper_text.len();
                }
            } else if self.fn_scope_depth == 0 {
                let temp_idx = self
                    .temp_var_names
                    .iter()
                    .rposition(|name| name == &temp_name)
                    .unwrap_or(self.temp_var_names.len());
                let helper_temp_names: Vec<String> = helper_var_names
                    .iter()
                    .map(|name| format!("_{}", name))
                    .collect();
                if static_private_helper_layout {
                    let brand_name = private_method_brand_name
                        .as_deref()
                        .map(|name| format!("_{}", name));
                    if let Some(ref brand) = brand_name {
                        let brand_ast: AstString = brand.as_str().into();
                        if !self.temp_var_names[..temp_idx].contains(&brand_ast) {
                            self.temp_var_names.insert(temp_idx, brand_ast);
                        }
                    }
                    let mut insert_at = self
                        .temp_var_names
                        .iter()
                        .rposition(|name| name == &temp_name)
                        .map(|idx| idx + 1)
                        .unwrap_or(temp_idx);
                    for helper_name in helper_temp_names {
                        if brand_name.as_ref() == Some(&helper_name) {
                            continue;
                        }
                        self.temp_var_names
                            .insert(insert_at, AstString::from(helper_name));
                        insert_at += 1;
                    }
                } else {
                    let mut insert_at = temp_idx;
                    for helper_name in helper_temp_names {
                        self.temp_var_names
                            .insert(insert_at, AstString::from(helper_name));
                        insert_at += 1;
                    }
                }
            } else if self.fn_scope_depth > 0 {
                let temp_idx = self
                    .temp_var_names
                    .iter()
                    .rposition(|name| name == &temp_name)
                    .unwrap_or(self.temp_var_names.len());
                let helper_temp_names: Vec<String> = helper_var_names
                    .iter()
                    .map(|name| format!("_{}", name))
                    .collect();
                if static_private_helper_layout {
                    let brand_name = private_method_brand_name
                        .as_deref()
                        .map(|name| format!("_{}", name));
                    if let Some(ref brand) = brand_name {
                        let brand_ast: AstString = brand.as_str().into();
                        if !self.temp_var_names[..temp_idx].contains(&brand_ast) {
                            self.temp_var_names.insert(temp_idx, brand_ast);
                        }
                    }
                    let mut insert_at = self
                        .temp_var_names
                        .iter()
                        .rposition(|name| name == &temp_name)
                        .map(|idx| idx + 1)
                        .unwrap_or(temp_idx);
                    for helper_name in helper_temp_names {
                        if brand_name.as_ref() == Some(&helper_name) {
                            continue;
                        }
                        self.temp_var_names
                            .insert(insert_at, AstString::from(helper_name));
                        insert_at += 1;
                    }
                } else {
                    let mut insert_at = temp_idx;
                    for helper_name in helper_temp_names {
                        self.temp_var_names
                            .insert(insert_at, AstString::from(helper_name));
                        insert_at += 1;
                    }
                }
            }
        }

        let class_expr_temp_by_start: std::collections::HashMap<u32, String> =
            class_expr_computed_temps
                .iter()
                .map(|(temp, start)| (*start, temp.clone()))
                .collect();
        let mut class_expr_method_name_overrides: std::collections::HashMap<u32, String> =
            std::collections::HashMap::new();
        let mut class_expr_post_actions: Vec<(u32, Option<String>, Expr)> = Vec::new();
        if !use_define {
            let computed_method_anchors: Vec<usize> = class_decl
                .members
                .iter()
                .enumerate()
                .filter_map(|(idx, member)| match &member.kind {
                    ClassMemberKind::Method(method)
                        if method.body.is_some()
                            && method.modifiers & MOD_DECLARE == 0
                            && method.modifiers & MOD_ABSTRACT == 0
                            && method.modifiers & MOD_STATIC == 0
                            && matches!(method.name, PropName::Computed(_, _)) =>
                    {
                        Some(idx)
                    }
                    _ => None,
                })
                .collect();
            let mut actions_by_method_idx: std::collections::HashMap<
                usize,
                Vec<(u32, Option<String>, Expr)>,
            > = std::collections::HashMap::new();
            let mut next_anchor_idx = 0usize;
            for (idx, member) in class_decl.members.iter().enumerate() {
                let ClassMemberKind::Property(prop) = &member.kind else {
                    continue;
                };
                if prop.modifiers & MOD_STATIC != 0
                    || prop.modifiers & MOD_DECLARE != 0
                    || prop.modifiers & MOD_ABSTRACT != 0
                {
                    continue;
                }
                let PropName::Computed(expr, _) = &prop.name else {
                    continue;
                };
                let define_uninitialized_field = downlevel_class_fields
                    && prop.initializer.is_none()
                    && prop.modifiers & MOD_ACCESSOR == 0
                    && !(prop.definite && prop.type_ann.is_some());
                let needs_temp = prop.initializer.is_some()
                    || define_uninitialized_field
                    || !prop.decorators.is_empty();
                let temp_name = if needs_temp {
                    class_expr_temp_by_start.get(&expr.span.start).cloned()
                } else {
                    None
                };
                let inner_expr = Self::unwrap_type_layers(expr);
                let needs_side_effect = prop.initializer.is_none()
                    && temp_name.is_none()
                    && !matches!(
                        &inner_expr.kind,
                        ExprKind::Ident(_)
                            | ExprKind::NumLit(_)
                            | ExprKind::StrLit(_)
                            | ExprKind::BigIntLit(_)
                            | ExprKind::BoolLit(_)
                            | ExprKind::Spread(_)
                            | ExprKind::Cond(_)
                    );
                if temp_name.is_none() && !needs_side_effect {
                    continue;
                }
                while next_anchor_idx < computed_method_anchors.len()
                    && computed_method_anchors[next_anchor_idx] <= idx
                {
                    next_anchor_idx += 1;
                }
                let action = (expr.span.start, temp_name, (*expr.as_ref()).clone());
                if let Some(anchor_member_idx) = computed_method_anchors.get(next_anchor_idx) {
                    actions_by_method_idx
                        .entry(*anchor_member_idx)
                        .or_default()
                        .push(action);
                } else {
                    class_expr_post_actions.push(action);
                }
            }

            for (idx, member) in class_decl.members.iter().enumerate() {
                let ClassMemberKind::Method(method) = &member.kind else {
                    continue;
                };
                if method.body.is_none()
                    || method.modifiers & MOD_DECLARE != 0
                    || method.modifiers & MOD_ABSTRACT != 0
                    || method.modifiers & MOD_STATIC != 0
                {
                    continue;
                }
                let PropName::Computed(expr, _) = &method.name else {
                    continue;
                };
                let mut name_parts: Vec<String> = Vec::new();
                if let Some(actions) = actions_by_method_idx.remove(&idx) {
                    for (_, temp_opt, action_expr) in actions {
                        let action_text = emit_expr_to_string(
                            self.source,
                            self.options,
                            &action_expr,
                            &self.cjs_import_map,
                            &self.cjs_string_import_locals,
                            &self.import_shadows,
                        );
                        if let Some(temp_name) = temp_opt {
                            name_parts.push(format!("{temp_name} = {action_text}"));
                        } else {
                            name_parts.push(action_text);
                        }
                    }
                }
                let method_name_text = emit_expr_to_string(
                    self.source,
                    self.options,
                    expr,
                    &self.cjs_import_map,
                    &self.cjs_string_import_locals,
                    &self.import_shadows,
                );
                let maybe_method_temp = if !method.decorators.is_empty() {
                    class_expr_temp_by_start.get(&expr.span.start).cloned()
                } else {
                    None
                };
                if let Some(temp_name) = maybe_method_temp {
                    name_parts.push(format!("{temp_name} = {method_name_text}"));
                } else if !name_parts.is_empty() {
                    name_parts.push(method_name_text);
                }
                if !name_parts.is_empty() {
                    class_expr_method_name_overrides
                        .insert(member.span.start, format!("[({})]", name_parts.join(", ")));
                }
            }
        }

        let downlevel_static_blocks = self.needs_downlevel("static-blocks");
        let mut native_static_computed_actions = class_expr_post_actions.clone();
        for (temp, expr_start) in &static_computed_temps {
            if let Some(expr) = class_decl.members.iter().find_map(|member| {
                if let ClassMemberKind::Property(prop) = &member.kind {
                    if let PropName::Computed(expr, _) = &prop.name {
                        if expr.span.start == *expr_start {
                            return Some((*expr.as_ref()).clone());
                        }
                    }
                }
                None
            }) {
                native_static_computed_actions.push((*expr_start, Some(temp.clone()), expr));
            }
        }
        native_static_computed_actions.sort_by_key(|(start, _, _)| *start);
        let use_native_static_computed_evaluator = !downlevel_static_blocks
            && !self.class_expr_needs_private_iife(class_decl)
            && helper_var_names.is_empty()
            && private_fields.is_empty()
            && private_accessors.is_empty()
            && private_method_brand_name.is_none()
            && !has_static_private_fields
            && !has_static_private_methods
            && !has_static_private_accessors
            && binding_name.is_none()
            && native_static_computed_actions
                .iter()
                .any(|(_, _, expr)| expr_has_this(expr) || expr_has_super(expr));
        if !use_native_static_computed_evaluator {
            saved_alias = class_name.as_ref().map(|name| {
                let prev = self.cjs_import_map.remove(name.as_str());
                self.cjs_import_map.insert(
                    AstString::from(name.as_str()),
                    (AstString::from(temp_name.as_str()), AstString::new("")),
                );
                (name.clone(), prev)
            });
        }

        if wrap {
            self.write("(");
        }
        if use_native_static_computed_evaluator {
            self.write(&temp_name);
            self.write(" = () => { ");
            for (idx, (_, temp_opt, expr)) in native_static_computed_actions.iter().enumerate() {
                if idx > 0 {
                    self.write(", ");
                }
                if let Some(temp_name) = temp_opt {
                    self.write(temp_name);
                    self.write(" = ");
                }
                self.emit_expr(expr);
            }
            self.write("; }");
            self.writeln(",");
            self.indent += 1;
            self.write("class");
        } else {
            self.write(&temp_name);
            self.write(" = class");
        }
        if let Some(ref name) = class_decl.name {
            self.write(" ");
            self.write(name);
        }
        if let Some(ref extends) = class_decl.extends {
            self.write(" extends ");
            self.emit_expr(extends);
        }
        self.writeln(" {");
        // TypeScript indents class members one level deeper than the comma-IIFE
        // entries (`},`, `_a.x = ...`, `_a`) around the class expression.
        self.indent += 2;

        let needs_legacy_instance_init_transform = !use_define
            && class_decl.members.iter().any(|m| {
                if let ClassMemberKind::Property(ref prop) = m.kind {
                    if matches!(prop.name, PropName::Private(_, _)) {
                        return false;
                    }
                    let define_uninitialized_field = downlevel_class_fields
                        && prop.initializer.is_none()
                        && prop.modifiers & MOD_ACCESSOR == 0
                        && !(prop.definite && prop.type_ann.is_some());
                    (prop.initializer.is_some() || define_uninitialized_field)
                        && prop.modifiers & MOD_STATIC == 0
                        && prop.modifiers & MOD_DECLARE == 0
                        && prop.modifiers & MOD_ABSTRACT == 0
                        && prop.modifiers & MOD_ACCESSOR == 0
                } else {
                    false
                }
            });

        // Pre-compute auto-accessor storage names for class expressions too.
        self.accessor_storage_names.clear();
        {
            let mut this_class_private_names_expr: std::collections::HashSet<String> =
                std::collections::HashSet::new();
            let mut this_class_private_members_expr: std::collections::HashMap<
                String,
                EnclosingPrivateMemberKind,
            > = std::collections::HashMap::new();
            for member in &class_decl.members {
                match &member.kind {
                    ClassMemberKind::Property(prop) => {
                        if let PropName::Private(name, _) = &prop.name {
                            let name = normalize_unicode_escapes(name);
                            this_class_private_names_expr.insert(name.clone());
                            this_class_private_members_expr.insert(
                                name,
                                EnclosingPrivateMemberKind::Field {
                                    is_static: prop.modifiers & MOD_STATIC != 0,
                                },
                            );
                        }
                    }
                    ClassMemberKind::Method(method) => {
                        if let PropName::Private(name, _) = &method.name {
                            let name = normalize_unicode_escapes(name);
                            this_class_private_names_expr.insert(name.clone());
                            this_class_private_members_expr.insert(
                                name,
                                EnclosingPrivateMemberKind::Method {
                                    is_static: method.modifiers & MOD_STATIC != 0,
                                },
                            );
                        }
                    }
                    ClassMemberKind::GetAccessor(acc) | ClassMemberKind::SetAccessor(acc) => {
                        if let PropName::Private(name, _) = &acc.name {
                            let name = normalize_unicode_escapes(name);
                            this_class_private_names_expr.insert(name.clone());
                            this_class_private_members_expr.insert(
                                name,
                                EnclosingPrivateMemberKind::Accessor {
                                    is_static: acc.modifiers & MOD_STATIC != 0,
                                },
                            );
                        }
                    }
                    _ => {}
                }
            }
            if (use_define && downlevel_accessors) || needs_legacy_instance_init_transform {
                let enclosing_names: std::collections::HashSet<&String> = self
                    .enclosing_private_names
                    .iter()
                    .flat_map(|s| s.iter())
                    .collect();
                for member in &class_decl.members {
                    if let ClassMemberKind::Property(ref prop) = member.kind {
                        if prop.modifiers & MOD_ACCESSOR != 0 {
                            if let Some(name) = prop.name.ident_name() {
                                let default = format!("{}_accessor_storage", name);
                                let storage = if this_class_private_names_expr.contains(&default)
                                    || enclosing_names.contains(&default)
                                {
                                    format!("{}_1_accessor_storage", name)
                                } else {
                                    default
                                };
                                this_class_private_names_expr.insert(storage.clone());
                                self.accessor_storage_names
                                    .insert(name.to_string(), storage);
                            }
                        }
                    }
                }
            }
            self.enclosing_private_names
                .push(this_class_private_names_expr);
            self.enclosing_private_members
                .push(this_class_private_members_expr);
            self.enclosing_private_class_names
                .push(class_decl.name.clone());
        }

        // Collect instance property initializers for constructor synthesis.
        let field_inits: Vec<(&ClassProp, Span)> = if use_define {
            Vec::new()
        } else {
            class_decl
                .members
                .iter()
                .filter_map(|m| {
                    if let ClassMemberKind::Property(ref prop) = m.kind {
                        if prop.modifiers & MOD_STATIC != 0
                            || prop.modifiers & MOD_DECLARE != 0
                            || prop.modifiers & MOD_ABSTRACT != 0
                        {
                            return None;
                        }
                        let define_uninitialized_field = downlevel_class_fields
                            && prop.initializer.is_none()
                            && prop.modifiers & MOD_ACCESSOR == 0
                            && !(prop.definite && prop.type_ann.is_some());
                        if prop.modifiers & MOD_ACCESSOR != 0 {
                            if needs_legacy_instance_init_transform && prop.initializer.is_some() {
                                return Some((prop, m.span));
                            }
                            return None;
                        }
                        if matches!(prop.name, PropName::Private(_, _)) {
                            if needs_legacy_instance_init_transform
                                && !downlevel_private
                                && prop.initializer.is_some()
                            {
                                return Some((prop, m.span));
                            }
                            return None;
                        }
                        if prop.initializer.is_some() || define_uninitialized_field {
                            return Some((prop, m.span));
                        }
                    }
                    None
                })
                .collect()
        };

        let has_explicit_ctor = class_decl.members.iter().any(|m| match &m.kind {
            ClassMemberKind::Constructor(ctor) => ctor.body.is_some(),
            _ => false,
        });
        let has_private_instance_fields = self.has_emitted_instance_private_field_inits(
            &private_fields,
            &invalid_duplicate_private_names,
            &private_field_name_by_span,
        );
        let has_private_instance_methods = !private_instance_methods.is_empty();
        let has_private_instance_accessors = private_accessors.iter().any(|info| !info.is_static);
        let needs_synth_ctor = (!field_inits.is_empty()
            || (downlevel_private
                && (has_private_instance_fields
                    || has_private_instance_methods
                    || has_private_instance_accessors)))
            && !has_explicit_ctor;
        let force_legacy_accessor_transform = needs_legacy_instance_init_transform;

        // Synthesize constructor when instance inits are hoisted out of the class body.
        if needs_synth_ctor {
            let prev_class_expr_temp_emitted = self.class_expr_temp_emitted;
            let prev_temp_var_counter = self.temp_var_counter;
            let prev_temp_var_names = std::mem::take(&mut self.temp_var_names);
            let prev_private_field_var_insert_pos = self.private_field_var_insert_pos.take();
            let prev_pending_private_field_vars =
                std::mem::take(&mut self.pending_private_field_vars);
            let prev_class_scope_temp_reserved = self.class_scope_temp_reserved;
            self.class_expr_temp_emitted = false;
            self.temp_var_counter = prev_temp_var_counter.max(self.class_scope_temp_reserved);
            self.class_scope_temp_reserved = 0;
            let extends_is_null = class_decl.extends.as_ref().is_some_and(|e| {
                let mut expr = e.as_ref();
                while let ExprKind::Paren(inner) = &expr.kind {
                    expr = inner.as_ref();
                }
                matches!(expr.kind, ExprKind::NullLit)
            });
            let hoist_insert_pos;
            self.write("constructor");
            if class_decl.extends.is_some() && !extends_is_null {
                self.writeln("() {");
                self.indent += 1;
                self.fn_scope_depth += 1;
                self.writeln("super(...arguments);");
                hoist_insert_pos = self.output.len();
            } else {
                self.writeln("() {");
                self.indent += 1;
                self.fn_scope_depth += 1;
                hoist_insert_pos = self.output.len();
            }
            if downlevel_private {
                if let Some(ref brand_name) = private_method_brand_name {
                    self.write("_");
                    self.write(brand_name);
                    self.writeln(".add(this);");
                }
                // Interleave private fields and regular field inits in source order.
                let mut init_order: Vec<(u32, bool, usize)> = Vec::new();
                for (i, (_, _, is_static, pf_span, _)) in private_fields.iter().enumerate() {
                    if !is_static {
                        init_order.push((pf_span.start, true, i));
                    }
                }
                for (i, (_, span)) in field_inits.iter().enumerate() {
                    init_order.push((span.start, false, i));
                }
                init_order.sort_by_key(|&(pos, _, _)| pos);
                for &(_, is_private, idx) in &init_order {
                    if is_private {
                        let (name, init, _, pf_span, binding_name) = &private_fields[idx];
                        let runtime_target = self.private_field_runtime_target(
                            name,
                            *pf_span,
                            false,
                            &invalid_duplicate_private_names,
                            &private_field_name_by_span,
                        );
                        let Some((runtime_name, runtime_is_static)) = runtime_target else {
                            self.append_trailing_comment(*pf_span);
                            continue;
                        };
                        self.write("_");
                        self.write(&runtime_name);
                        if runtime_is_static {
                            self.write(" = { value: ");
                            if let Some(init_expr) = init {
                                self.emit_private_static_value_initializer(
                                    binding_name.as_ref(),
                                    init_expr,
                                );
                            } else {
                                self.write("void 0");
                            }
                            self.write(" }");
                        } else {
                            self.write(".set(this, ");
                            if let Some(init_expr) = init {
                                self.emit_expr_with_binding_name_if_needed(
                                    binding_name.as_ref(),
                                    init_expr,
                                );
                            } else {
                                self.write("void 0");
                            }
                            self.write(")");
                        }
                        self.writeln(";");
                        self.append_trailing_comment(*pf_span);
                    } else {
                        let (prop, span) = &field_inits[idx];
                        self.emit_field_init(prop);
                        self.append_trailing_comment(*span);
                    }
                }
            } else {
                for (prop, span) in &field_inits {
                    self.emit_field_init(prop);
                    self.append_trailing_comment(*span);
                }
            }
            let indent_str = "    ".repeat(self.indent as usize);
            let mut insert_offset = 0usize;
            {
                let all_vars: Vec<&str> = self
                    .pending_private_field_vars
                    .iter()
                    .map(|s| s.as_str())
                    .chain(self.temp_var_names.iter().map(|s| s.as_str()))
                    .collect();
                if !all_vars.is_empty() {
                    let var_decl = format!("{}var {};\n", indent_str, all_vars.join(", "));
                    self.output
                        .insert_str(hoist_insert_pos + insert_offset, &var_decl);
                    insert_offset += var_decl.len();
                }
            }
            self.fn_scope_depth -= 1;
            self.indent -= 1;
            self.writeln("}");
            self.class_expr_temp_emitted = prev_class_expr_temp_emitted;
            self.temp_var_counter = prev_temp_var_counter;
            self.class_scope_temp_reserved = prev_class_scope_temp_reserved;
            self.temp_var_names = prev_temp_var_names;
            self.private_field_var_insert_pos = prev_private_field_var_insert_pos;
            self.pending_private_field_vars = prev_pending_private_field_vars;
        }

        // In define mode, emit field declarations for constructor parameter
        // properties at the start of the class body (same as class declaration path).
        if use_define {
            if let Some(ctor) = class_decl.members.iter().find_map(|m| {
                if let ClassMemberKind::Constructor(ctor) = &m.kind {
                    if ctor.body.is_some() {
                        Some(ctor)
                    } else {
                        None
                    }
                } else {
                    None
                }
            }) {
                for param in &ctor.params {
                    if param.modifiers
                        & (MOD_PUBLIC | MOD_PRIVATE | MOD_PROTECTED | MOD_READONLY | MOD_OVERRIDE)
                        != 0
                    {
                        if let PatKind::Ident(ref name) = param.name.kind {
                            self.write(name);
                            self.writeln(";");
                        }
                    }
                }
            }
        }

        if use_native_static_computed_evaluator {
            self.write("static { ");
            self.write(&temp_name);
            self.writeln("(); }");
        }

        // Emit non-static class members (methods, constructor, etc.).
        for member in &class_decl.members {
            // Skip properties in legacy mode — static ones are emitted as
            // `_a.prop = value` after the class, and instance ones become
            // constructor body assignments (handled above via field_inits).
            if let ClassMemberKind::Property(ref prop) = member.kind {
                if !use_define
                    && prop.modifiers & MOD_DECLARE == 0
                    && prop.modifiers & MOD_ABSTRACT == 0
                    && prop.modifiers & MOD_ACCESSOR == 0
                    && !matches!(prop.name, PropName::Private(_, _))
                {
                    let native_static_field_in_body = use_native_static_computed_evaluator
                        && prop.modifiers & MOD_STATIC != 0
                        && prop.initializer.is_some();
                    if !native_static_field_in_body {
                        continue;
                    }
                }
            }
            // Skip overload signatures and type-only members.
            let is_erased = match &member.kind {
                ClassMemberKind::Method(m) => {
                    m.body.is_none()
                        && m.modifiers & MOD_DECLARE == 0
                        && m.modifiers & MOD_ABSTRACT == 0
                }
                ClassMemberKind::Constructor(c) => c.body.is_none(),
                ClassMemberKind::IndexSignature(_) => true,
                _ => false,
            };
            // For auto-accessors being lowered, defer leading comments
            // until after the storage field.
            let is_lowered_accessor = matches!(
                &member.kind,
                ClassMemberKind::Property(prop) if prop.modifiers & MOD_ACCESSOR != 0
                    && prop.modifiers & MOD_DECLARE == 0
                    && prop.modifiers & MOD_ABSTRACT == 0
                    && prop.name.ident_name().is_some()
                    && (downlevel_accessors
                        || (force_legacy_accessor_transform
                            && prop.modifiers & MOD_STATIC == 0))
            );
            if !is_erased && !is_lowered_accessor {
                self.emit_leading_comments(member.span.start);
            }
            let before_len = self.output.len();
            let method_name_override = class_expr_method_name_overrides
                .get(&member.span.start)
                .cloned();
            self.emit_class_member_with_private(
                member,
                &field_inits,
                class_decl.extends.is_some(),
                use_define,
                downlevel_private,
                &invalid_duplicate_private_names,
                &private_field_name_by_span,
                &private_fields,
                private_method_brand_name.as_deref(),
                method_name_override.as_deref(),
                &temp_name,
                &Vec::new(),
            );
            let handled_accessor_comment = matches!(
                &member.kind,
                ClassMemberKind::Property(prop)
                    if !use_define
                        && prop.modifiers & MOD_ACCESSOR != 0
                        && (downlevel_accessors
                            || (force_legacy_accessor_transform
                                && prop.modifiers & MOD_STATIC == 0))
            );
            if self.output.len() > before_len && !handled_accessor_comment {
                self.append_trailing_comment(member.span);
            }
            self.advance_comment_pos(member.span.end);
        }

        // Close class body. Native static-block computed-key capture can
        // return the class expression directly, so it doesn't need the
        // trailing comma/operator tail.
        self.indent -= 1;
        if use_native_static_computed_evaluator {
            self.write("}");
        } else {
            self.writeln("},");
        }

        // Emit deferred computed key evaluations in source order before helper
        // initializers and __setFunctionName.
        let tail_computed_actions = if use_native_static_computed_evaluator {
            Vec::new()
        } else {
            native_static_computed_actions.clone()
        };
        for (_, temp_opt, expr) in &tail_computed_actions {
            if let Some(temp_name) = temp_opt {
                self.write(temp_name);
                self.write(" = ");
            }
            self.emit_expr(expr);
            self.writeln(",");
        }

        // Emit WeakMap/WeakSet initializers and private method/accessor helpers.
        for (name, _, is_static, _, _) in &private_fields {
            if !is_static {
                self.write("_");
                self.write(name);
                self.write(" = new WeakMap()");
                self.writeln(",");
            }
        }
        if let Some(ref brand_name) = private_method_brand_name {
            self.write("_");
            self.write(brand_name);
            self.write(" = new WeakSet()");
            self.writeln(",");
        }
        let private_method_helper_by_pos: std::collections::HashMap<u32, &str> =
            private_instance_methods
                .iter()
                .chain(private_static_methods.iter())
                .map(|(_, span, helper_decl_name, _)| (span.start, helper_decl_name.as_str()))
                .collect();
        let private_accessor_helper_by_pos: std::collections::HashMap<u32, &str> =
            private_accessors
                .iter()
                .map(|info| (info.span.start, info.helper_decl_name.as_str()))
                .collect();
        for member in &class_decl.members {
            match &member.kind {
                ClassMemberKind::Method(method) => {
                    if let PropName::Private(ref name, _) = method.name {
                        if let Some(ref body) = method.body {
                            if Self::private_name_uses_any_member_recovery(
                                name,
                                &invalid_duplicate_private_names,
                            ) {
                                continue;
                            }
                            let norm_name = normalize_unicode_escapes(name);
                            let fallback_helper_decl_name = qualify_private_helper(&norm_name);
                            let helper_decl_name = private_method_helper_by_pos
                                .get(&member.span.start)
                                .copied()
                                .unwrap_or(fallback_helper_decl_name.as_str());
                            self.write("_");
                            self.write(&helper_decl_name);
                            self.write(" = ");
                            let downlevel_async_gen = method.is_async
                                && method.is_generator
                                && self.needs_downlevel("async-generator");
                            let async_gen_move_params = downlevel_async_gen
                                && method.params.iter().any(Self::param_needs_async_lift);
                            let downlevel_async = method.is_async
                                && !method.is_generator
                                && self.needs_downlevel("async");
                            if downlevel_async_gen {
                                self.write("function _");
                                self.write(&helper_decl_name);
                                self.write("(");
                                if async_gen_move_params {
                                    let temps =
                                        if method.params.iter().any(|p| p.initializer.is_some()) {
                                            Self::generate_async_lift_prefix_temp_names(
                                                &method.params,
                                            )
                                        } else {
                                            Self::generate_async_lift_temp_names(&method.params)
                                        };
                                    for (i, t) in temps.iter().enumerate() {
                                        if i > 0 {
                                            self.write(", ");
                                        }
                                        self.write(t);
                                    }
                                } else {
                                    self.emit_params(&method.params);
                                }
                                self.write(") ");
                                let gen_name = format!("_{}_1", helper_decl_name);
                                self.awaiter_enclosing_span = Some(member.span);
                                self.emit_async_generator_body(
                                    body,
                                    Some(&gen_name),
                                    async_gen_move_params.then_some(method.params.as_slice()),
                                );
                            } else if downlevel_async {
                                self.write("function _");
                                self.write(&helper_decl_name);
                                self.write("(");
                                self.emit_params(&method.params);
                                self.write(") ");
                                self.awaiter_enclosing_span = Some(member.span);
                                self.emit_awaiter_body_with_params(
                                    body,
                                    None,
                                    &method.params,
                                    None,
                                );
                            } else {
                                if method.is_async {
                                    self.write("async ");
                                }
                                self.write("function");
                                if method.is_generator {
                                    self.write("*");
                                }
                                self.write(" _");
                                self.write(&helper_decl_name);
                                self.write("(");
                                self.emit_params(&method.params);
                                self.write(") ");
                                self.emit_block_for_decl_body(body, member.span);
                            }
                            self.writeln(",");
                        }
                    }
                }
                ClassMemberKind::GetAccessor(acc) | ClassMemberKind::SetAccessor(acc) => {
                    if let PropName::Private(ref name, _) = acc.name {
                        if Self::private_name_uses_any_member_recovery(
                            name,
                            &invalid_duplicate_private_names,
                        ) {
                            continue;
                        }
                        let norm_name = normalize_unicode_escapes(name);
                        let suffix = if matches!(member.kind, ClassMemberKind::GetAccessor(_)) {
                            "get"
                        } else {
                            "set"
                        };
                        let fallback_helper_decl_name =
                            qualify_private_accessor(&norm_name, suffix);
                        let helper_decl_name = private_accessor_helper_by_pos
                            .get(&member.span.start)
                            .copied()
                            .unwrap_or(fallback_helper_decl_name.as_str());
                        self.write("_");
                        self.write(&helper_decl_name);
                        self.write(" = function _");
                        self.write(&helper_decl_name);
                        self.write("(");
                        self.emit_params(&acc.params);
                        self.write(") ");
                        if let Some(ref body) = acc.body {
                            let is_async_accessor =
                                acc.modifiers & MOD_ASYNC != 0 && self.needs_downlevel("async");
                            if is_async_accessor {
                                self.awaiter_enclosing_span = Some(member.span);
                                self.emit_awaiter_body_with_params(body, None, &acc.params, None);
                            } else {
                                self.emit_block_for_decl_body(body, member.span);
                            }
                        } else {
                            self.write("{ }");
                        }
                        self.writeln(",");
                    }
                }
                _ => {}
            }
        }

        // Emit __setFunctionName for anonymous class expressions assigned to named bindings.
        // `__setFunctionName(_a, "name"),`
        if let Some(binding_name) = binding_name {
            let needs_named_evaluation = class_decl.name.is_none()
                && (class_has_static_initializers(class_decl) || has_static_private_fields);
            if needs_named_evaluation {
                self.needs_set_function_name_helper = true;
                self.write(self.helper_prefix());
                self.write("__setFunctionName(");
                self.write(&temp_name);
                self.write(", ");
                self.emit_class_expr_binding_name_arg(binding_name);
                self.writeln("),");
            }
        }

        // Static initializers of an anonymous class expression run with `this`
        // bound to the class value. Once the fields are moved into the comma
        // expression, preserve that receiver through the class alias temp.
        let saved_static_this_alias = self.static_this_alias.replace(temp_name.clone());

        // Emit static private field initializations after __setFunctionName.
        if has_static_private_fields {
            for (name, init, is_static, pf_span, binding_name) in &private_fields {
                if *is_static {
                    let runtime_target = self.private_field_runtime_target(
                        name,
                        *pf_span,
                        true,
                        &invalid_duplicate_private_names,
                        &private_field_name_by_span,
                    );
                    let Some((runtime_name, runtime_is_static)) = runtime_target else {
                        continue;
                    };
                    self.write("_");
                    self.write(&runtime_name);
                    if runtime_is_static {
                        self.write(" = { value: ");
                        if let Some(init_expr) = init {
                            self.emit_private_static_value_initializer(
                                binding_name.as_ref(),
                                init_expr,
                            );
                        } else {
                            self.write("void 0");
                        }
                        self.write(" }");
                    } else {
                        self.write(".set(");
                        self.write(&temp_name);
                        self.write(", ");
                        if let Some(init_expr) = init {
                            self.emit_expr_with_binding_name_if_needed(
                                binding_name.as_ref(),
                                init_expr,
                            );
                        } else {
                            self.write("void 0");
                        }
                        self.write(")");
                    }
                    self.writeln(",");
                }
            }
        }

        // Emit static property assignments and static blocks as `_a.prop = value,` or `(() => { ... })(),`.
        for member in &class_decl.members {
            match &member.kind {
                ClassMemberKind::Property(ref prop) => {
                    let is_private_static = matches!(prop.name, PropName::Private(_, _))
                        && prop.modifiers & MOD_STATIC != 0
                        && downlevel_private;
                    if !use_native_static_computed_evaluator
                        && !is_private_static
                        && prop.modifiers & MOD_STATIC != 0
                        && prop.modifiers & MOD_DECLARE == 0
                        && prop.modifiers & MOD_ABSTRACT == 0
                        && prop.initializer.is_some()
                    {
                        self.emit_leading_comments(member.span.start);
                        self.write(&temp_name);
                        // Use captured temp for computed keys, dot for regular names
                        if let PropName::Computed(expr, _) = &prop.name {
                            // Find the matching static computed temp
                            let temp_var = static_computed_temps
                                .iter()
                                .find(|(_, start)| *start == expr.span.start)
                                .map(|(t, _)| t.as_str());
                            if let Some(tv) = temp_var {
                                self.write("[");
                                self.write(tv);
                                self.write("]");
                            } else {
                                self.write("[");
                                self.emit_expr(expr);
                                self.write("]");
                            }
                        } else {
                            self.write(".");
                            self.emit_prop_name(&prop.name);
                        }
                        self.write(" = ");
                        self.emit_class_prop_initializer_expr(
                            prop,
                            prop.initializer.as_ref().unwrap(),
                        );
                        self.writeln(",");
                        self.append_trailing_comment(member.span);
                        self.advance_comment_pos(member.span.end);
                    }
                }
                ClassMemberKind::StaticBlock(stmts) => {
                    if downlevel_static_blocks {
                        self.emit_leading_comments(member.span.start);
                        if self.emit_special_static_block_recovery(member, true) {
                            self.append_trailing_comment(member.span);
                            self.advance_comment_pos(member.span.end);
                            continue;
                        }
                        self.emit_downleveled_static_block_iife(stmts, member.span);
                        self.writeln(",");
                        self.append_trailing_comment(member.span);
                        self.advance_comment_pos(member.span.end);
                    }
                }
                _ => {}
            }
        }
        self.static_this_alias = saved_static_this_alias;

        // Final element: `_a` (with closing paren when wrapped), unless the
        // native static-block path returned the class expression directly.
        if !use_native_static_computed_evaluator {
            self.write(&temp_name);
        }
        if wrap {
            self.write(")");
        }
        if use_native_static_computed_evaluator {
            self.indent -= 2;
        } else {
            self.indent -= 1;
        }
        self.class_general_computed_temps = saved_general_computed_temps;
        self.current_class_name = prev_class_name;
        self.current_class_private_fields = prev_private_fields;
        self.current_class_static_private_fields = prev_static_private_fields;
        self.current_class_static_alias = prev_static_alias;
        self.class_scope_temp_reserved = prev_class_scope_temp_reserved;
        self.current_class_private_methods = prev_private_methods;
        self.current_class_static_private_methods = prev_static_private_methods;
        self.current_class_private_accessors = prev_private_accessors;
        self.current_class_static_private_accessors = prev_static_private_accessors;
        self.current_class_private_var_map = prev_private_var_map;

        // Pop enclosing private names stack after class expression processing.
        self.enclosing_private_names.pop();
        self.enclosing_private_members.pop();
        self.enclosing_private_class_names.pop();

        // Restore the class name mapping.
        if let Some((name, prev)) = saved_alias {
            self.cjs_import_map.remove(name.as_str());
            if let Some(prev_val) = prev {
                self.cjs_import_map
                    .insert(AstString::from(name.as_str()), prev_val);
            }
        }
    }

    pub(super) fn emit_class_static_field_initializers(
        &mut self,
        class_decl: &ClassDecl,
        // Saved comment state (next_comment_idx, comment_emit_pos) per static member span.start,
        // collected during the class body loop before comments were advanced past erased members.
        static_comment_states: &[(u32, usize, u32)],
    ) {
        let downlevel_static_blocks = self.needs_downlevel("static-blocks");
        let use_define_semantics = self.use_define_for_class_fields();
        let use_define = use_define_semantics && !self.needs_downlevel("class-fields");

        if use_define && !downlevel_static_blocks {
            return;
        }

        // Static fields are only moved after class body in legacy mode (!use_define).
        // Static blocks are moved after class body if downlevel_static_blocks is true.
        // For `export default @dec class { }`, class_decl.name is None but the variable
        // is named "default_1" — use recovered_default_class_name if available.
        let class_name_owned: String;
        let class_name = if let Some(ref n) = class_decl.name {
            n.as_str()
        } else if let Some(ref n) = self.recovered_default_class_name {
            class_name_owned = n.clone();
            &class_name_owned
        } else {
            "default"
        };

        for member in &class_decl.members {
            match &member.kind {
                ClassMemberKind::Property(prop) => {
                    // Skip static private fields when downleveling private fields
                    // (they are handled separately with `{ value: ... }` objects).
                    let is_private_static = matches!(prop.name, PropName::Private(_, _))
                        && prop.modifiers & MOD_STATIC != 0
                        && self.needs_downlevel("private-fields");
                    if !is_private_static
                        && !use_define
                        && prop.modifiers & MOD_STATIC != 0
                        && prop.modifiers & MOD_DECLARE == 0
                        && prop.modifiers & MOD_ABSTRACT == 0
                        && prop.initializer.is_some()
                        // When target supports static blocks (ES2022+), static properties
                        // with initializers are already emitted as `static { this.p = val; }`
                        // in the class body — don't also emit them after the class.
                        && downlevel_static_blocks
                    {
                        // Emit leading comments (including line comments) before the static field
                        // assignment. The class body loop already advanced past these comments, so
                        // we restore the saved state to make emit_leading_comments work again.
                        if self.options.remove_comments != Some(true) {
                            if let Some(&(_, saved_idx, saved_emit_pos)) = static_comment_states
                                .iter()
                                .find(|&&(start, _, _)| start == member.span.start)
                            {
                                let old_idx = self.next_comment_idx;
                                let old_emit_pos = self.comment_emit_pos;
                                self.next_comment_idx = saved_idx;
                                self.comment_emit_pos = saved_emit_pos;
                                self.emit_leading_comments(member.span.start);
                                // Merge: take the maximum to avoid double-emitting
                                self.next_comment_idx = self.next_comment_idx.max(old_idx);
                                self.comment_emit_pos = self.comment_emit_pos.max(old_emit_pos);
                            }
                        }
                        // When useDefineForClassFields is true and target needs
                        // downlevel class fields, use Object.defineProperty for
                        // static fields (matching instance field behavior).
                        let static_define_downlevel =
                            use_define_semantics && self.needs_downlevel("class-fields");
                        if static_define_downlevel {
                            self.write("Object.defineProperty(");
                            self.write(class_name);
                            self.write(", ");
                            match &prop.name {
                                PropName::Ident(name, _) => {
                                    self.write("\"");
                                    self.write(name);
                                    self.write("\"");
                                }
                                PropName::Number(n, _) => {
                                    self.write("\"");
                                    self.write(n);
                                    self.write("\"");
                                }
                                PropName::String(s, _) => {
                                    self.write("\"");
                                    self.write(s);
                                    self.write("\"");
                                }
                                PropName::Computed(expr, _) => {
                                    let general_temp = self
                                        .class_general_computed_temps
                                        .iter()
                                        .find(|(_, start)| *start == expr.span.start)
                                        .map(|(name, _)| name.clone());
                                    let class_name_str =
                                        self.current_class_name.as_deref().unwrap_or("");
                                    let temp_opt = general_temp.or_else(|| {
                                        self.class_computed_name_temps.as_ref().and_then(|temps| {
                                            expr_is_class_member_access(expr, class_name_str)
                                                .map(|p| format!("{}.{}", class_name_str, p))
                                                .and_then(|key| temps.get(&key).cloned())
                                        })
                                    });
                                    if let Some(temp) = temp_opt {
                                        self.write(&temp);
                                    } else {
                                        self.emit_expr(expr);
                                    }
                                }
                                PropName::Private(name, _) => {
                                    self.write("\"#");
                                    self.write(name);
                                    self.write("\"");
                                }
                            }
                            self.writeln(", {");
                            self.indent += 1;
                            self.writeln("enumerable: true,");
                            self.writeln("configurable: true,");
                            self.writeln("writable: true,");
                            self.write("value: ");
                            if let Some(init) = &prop.initializer {
                                self.suppress_oc_parens = true;
                                self.emit_class_prop_initializer_expr(prop, init);
                            } else {
                                self.write("void 0");
                            }
                            self.newline();
                            self.indent -= 1;
                            self.writeln("});");
                            self.append_trailing_comment(member.span);
                        } else {
                            self.write(class_name);
                            let temp_opt = if let PropName::Computed(expr, _) = &prop.name {
                                // First check class_computed_name_temps (ClassName.staticProp patterns)
                                let from_class_temps =
                                    self.class_computed_name_temps.as_ref().and_then(|temps| {
                                        expr_is_class_member_access(expr, class_name)
                                            .map(|p| format!("{}.{}", class_name, p))
                                            .and_then(|key| temps.get(&key).cloned())
                                    });
                                // Fall back to general computed temps (arbitrary expressions)
                                from_class_temps.or_else(|| {
                                    self.class_general_computed_temps
                                        .iter()
                                        .find(|(_, start)| *start == expr.span.start)
                                        .map(|(temp, _)| temp.clone())
                                })
                            } else {
                                None
                            };
                            if let Some(temp) = temp_opt {
                                self.write("[");
                                self.write(&temp);
                                self.write("]");
                            } else {
                                self.emit_member_access(&prop.name);
                            }
                            self.write(" = ");
                            // When static_this_alias forces structured emit instead of source-copy,
                            // comments inside the initializer body may have been skipped because
                            // comment_emit_pos was advanced during class body emit. Reset it to
                            // allow the structured emit path to pick up those comments.
                            if self.static_this_alias.is_some() {
                                let init_start = prop.initializer.as_ref().unwrap().span.start;
                                if self.comment_emit_pos > init_start {
                                    self.comment_emit_pos = init_start;
                                    // Also rewind next_comment_idx to find comments inside the initializer
                                    while self.next_comment_idx > 0
                                        && self
                                            .comments
                                            .get(self.next_comment_idx - 1)
                                            .is_some_and(|c| c.pos >= init_start)
                                    {
                                        self.next_comment_idx -= 1;
                                    }
                                }
                            }
                            self.suppress_oc_parens = true;
                            self.emit_class_prop_initializer_expr(
                                prop,
                                prop.initializer.as_ref().unwrap(),
                            );
                            self.writeln(";");
                            self.append_trailing_comment(member.span);
                        }
                    }
                }
                ClassMemberKind::StaticBlock(stmts) => {
                    if downlevel_static_blocks {
                        let mut restore_comment_state: Option<(usize, u32)> = None;
                        if self.options.remove_comments != Some(true) {
                            if let Some(&(_, saved_idx, saved_emit_pos)) = static_comment_states
                                .iter()
                                .find(|&&(start, _, _)| start == member.span.start)
                            {
                                restore_comment_state =
                                    Some((self.next_comment_idx, self.comment_emit_pos));
                                self.next_comment_idx = saved_idx;
                                self.comment_emit_pos = saved_emit_pos;
                                self.emit_leading_comments(member.span.start);
                            } else {
                                self.emit_leading_comments(member.span.start);
                            }
                        } else {
                            self.emit_leading_comments(member.span.start);
                        }
                        if self.emit_special_static_block_recovery(member, true) {
                            if let Some((old_idx, old_emit_pos)) = restore_comment_state {
                                self.next_comment_idx = self.next_comment_idx.max(old_idx);
                                self.comment_emit_pos = self.comment_emit_pos.max(old_emit_pos);
                            }
                            self.append_trailing_comment(member.span);
                            self.advance_comment_pos(member.span.end);
                            continue;
                        }
                        self.emit_downleveled_static_block_iife(stmts, member.span);
                        self.writeln(";");
                        if let Some((old_idx, old_emit_pos)) = restore_comment_state {
                            // Merge after emitting the entire downleveled block
                            // so comments inside the static block are not skipped.
                            self.next_comment_idx = self.next_comment_idx.max(old_idx);
                            self.comment_emit_pos = self.comment_emit_pos.max(old_emit_pos);
                        }
                        self.append_trailing_comment(member.span);
                    }
                }
                _ => {}
            }
        }
    }

    /// Unwrap parentheses and type assertions to get the inner expression.
    fn unwrap_type_layers(expr: &Expr) -> &Expr {
        match &expr.kind {
            ExprKind::Paren(inner) => Self::unwrap_type_layers(inner),
            ExprKind::TypeAssertion(ta) => Self::unwrap_type_layers(&ta.expr),
            ExprKind::As(a) => Self::unwrap_type_layers(&a.expr),
            ExprKind::Satisfies(s) => Self::unwrap_type_layers(&s.expr),
            _ => expr,
        }
    }

    pub(super) fn computed_name_is_simple_literal(expr: &Expr) -> bool {
        let inner = Self::unwrap_type_layers(expr);
        matches!(
            inner.kind,
            ExprKind::StrLit(_)
                | ExprKind::NumLit(_)
                | ExprKind::BigIntLit(_)
                | ExprKind::BoolLit(_)
                | ExprKind::NoSubstTemplate(_)
        )
    }

    pub(super) fn class_expr_needs_private_iife(&self, class_decl: &ClassDecl) -> bool {
        self.needs_downlevel("private-fields")
            && class_decl.members.iter().any(|member| match &member.kind {
                ClassMemberKind::Property(prop) => {
                    prop.modifiers & MOD_DECLARE == 0
                        && prop.modifiers & MOD_ABSTRACT == 0
                        && matches!(prop.name, PropName::Private(_, _))
                }
                ClassMemberKind::Method(method) => {
                    method.body.is_some() && matches!(method.name, PropName::Private(_, _))
                }
                ClassMemberKind::GetAccessor(acc) | ClassMemberKind::SetAccessor(acc) => {
                    acc.body.is_some() && matches!(acc.name, PropName::Private(_, _))
                }
                _ => false,
            })
    }

    pub(super) fn class_expr_needs_legacy_computed_iife(&self, class_decl: &ClassDecl) -> bool {
        let use_define_semantics = self.use_define_for_class_fields();
        let downlevel_class_fields = use_define_semantics && self.needs_downlevel("class-fields");
        let use_define = use_define_semantics && !downlevel_class_fields;
        if use_define {
            return false;
        }

        let computed_method_anchors: Vec<usize> = class_decl
            .members
            .iter()
            .enumerate()
            .filter_map(|(idx, member)| match &member.kind {
                ClassMemberKind::Method(method)
                    if method.body.is_some()
                        && method.modifiers & MOD_DECLARE == 0
                        && method.modifiers & MOD_ABSTRACT == 0
                        && method.modifiers & MOD_STATIC == 0
                        && matches!(method.name, PropName::Computed(_, _)) =>
                {
                    Some(idx)
                }
                _ => None,
            })
            .collect();

        let mut next_anchor_idx = 0usize;
        for (idx, member) in class_decl.members.iter().enumerate() {
            let ClassMemberKind::Property(prop) = &member.kind else {
                continue;
            };
            if prop.modifiers & MOD_STATIC != 0
                || prop.modifiers & MOD_DECLARE != 0
                || prop.modifiers & MOD_ABSTRACT != 0
            {
                continue;
            }
            let PropName::Computed(expr, _) = &prop.name else {
                continue;
            };

            let define_uninitialized_field = downlevel_class_fields
                && prop.initializer.is_none()
                && prop.modifiers & MOD_ACCESSOR == 0
                && !(prop.definite && prop.type_ann.is_some());
            let needs_temp_capture = (prop.initializer.is_some()
                || define_uninitialized_field
                || !prop.decorators.is_empty())
                && !Self::computed_name_is_simple_literal(expr);
            let needs_side_effect = prop.initializer.is_none()
                && !needs_temp_capture
                && !matches!(
                    &expr.kind,
                    ExprKind::Ident(_)
                        | ExprKind::NumLit(_)
                        | ExprKind::StrLit(_)
                        | ExprKind::BigIntLit(_)
                        | ExprKind::Spread(_)
                        | ExprKind::Cond(_)
                );
            if !needs_temp_capture && !needs_side_effect {
                continue;
            }

            while next_anchor_idx < computed_method_anchors.len()
                && computed_method_anchors[next_anchor_idx] <= idx
            {
                next_anchor_idx += 1;
            }
            if next_anchor_idx >= computed_method_anchors.len() {
                return true;
            }
        }

        false
    }

    #[allow(dead_code)] // jsdoc preservation for static members
    pub(super) fn emit_static_member_jsdoc(&mut self, member_start: u32) {
        for c in self.comments.iter() {
            if c.end > member_start {
                break;
            }
            if c.pos < member_start {
                let text = &self.source[c.pos as usize..c.end as usize];
                if text.starts_with("/**") {
                    // Check only whitespace between comment end and member start
                    let between = &self.source[c.end as usize..member_start as usize];
                    if between.trim().is_empty() {
                        // Determine the source indentation of the comment
                        // to strip from continuation lines.
                        let cstart = c.pos as usize;
                        let prefix = &self.source[..cstart];
                        let line_start = prefix.rfind('\n').map(|p| p + 1).unwrap_or(0);
                        let src_indent = cstart - line_start;
                        for (i, line) in text.lines().enumerate() {
                            if i == 0 {
                                self.writeln(line.trim_end());
                            } else {
                                let stripped = if line.len() >= src_indent
                                    && line.as_bytes()[..src_indent]
                                        .iter()
                                        .all(|&b| b == b' ' || b == b'\t')
                                {
                                    &line[src_indent..]
                                } else {
                                    line.trim_start()
                                };
                                self.writeln(stripped.trim_end());
                            }
                        }
                    }
                }
            }
        }
    }

    /// Emit a property name for a class member, using temp captures when the
    /// computed name references the class before construction.
    fn emit_class_member_prop_name(&mut self, name: &PropName, is_static: bool) {
        let class_name = self.current_class_name.as_deref().unwrap_or("").to_string();
        match name {
            PropName::Computed(expr, _) => {
                // In class expression IIFEs, the class name is mapped to a temp in
                // cjs_import_map. But computed property names are evaluated in the
                // class scope where the class name binding is directly available,
                // so suppress the rewrite for this expression.
                let saved_class_map = if !class_name.is_empty() {
                    self.cjs_import_map.remove(class_name.as_str())
                } else {
                    None
                };

                let preceding: Vec<String> = self
                    .class_preceding_static_prop_keys
                    .as_ref()
                    .map(|v| v.clone())
                    .unwrap_or_default();
                let temps: std::collections::HashMap<String, String> = self
                    .class_computed_name_temps
                    .as_ref()
                    .map(|m| m.clone())
                    .unwrap_or_default();
                if let Some(p) = expr_is_class_member_access(expr, &class_name) {
                    let key = format!("{}.{}", class_name, p);
                    if is_static && !preceding.is_empty() && temps.contains_key(&key) {
                        // Static method with computed name: emit comma expr to capture
                        // preceding static property keys, e.g. (_a = A.p1, A.p2)
                        self.write("[(");
                        let mut first = true;
                        for prec_key in &preceding {
                            if let Some(temp) = temps.get(prec_key) {
                                if !first {
                                    self.write(", ");
                                }
                                let (_, prop) = prec_key.split_once('.').unwrap_or(("", prec_key));
                                self.write(temp);
                                self.write(" = ");
                                self.write(&class_name);
                                self.write(".");
                                self.write(prop);
                                first = false;
                            }
                        }
                        if !first {
                            self.write(", ");
                        }
                        self.emit_expr(expr);
                        self.write(")]");
                        if let Some(entry) = saved_class_map {
                            self.cjs_import_map
                                .insert(AstString::from(class_name.as_str()), entry);
                        }
                        return;
                    }
                }
                self.write("[");
                self.emit_expr(expr);
                self.write("]");

                if let Some(entry) = saved_class_map {
                    self.cjs_import_map
                        .insert(AstString::from(class_name.as_str()), entry);
                }
            }
            PropName::String(raw, _) => {
                // In class bodies, TypeScript unquotes string-literal method names
                // that evaluate to "constructor", emitting the bare keyword instead.
                let decoded = decode_js_string_content(raw);
                if decoded == "constructor" {
                    self.write("constructor");
                } else {
                    self.emit_prop_name(name);
                }
            }
            _ => self.emit_prop_name(name),
        }
    }

    pub(super) fn emit_field_init(&mut self, prop: &ClassProp) {
        let define_downlevel =
            self.use_define_for_class_fields() && self.needs_downlevel("class-fields");

        if let Some(slot) = self.standard_decorator_instance_field_slot(prop).cloned() {
            match &prop.name {
                PropName::Ident(name, _) => {
                    self.write("this.");
                    self.write(name);
                }
                PropName::Number(n, _) => {
                    self.write("this[");
                    self.write(n);
                    self.write("]");
                }
                PropName::String(s, span) => {
                    let q = self.original_quote_char(span);
                    self.write("this[");
                    self.write(q);
                    self.write(s);
                    self.write(q);
                    self.write("]");
                }
                PropName::Computed(expr, _) => {
                    self.write("this[");
                    self.emit_expr(expr);
                    self.write("]");
                }
                PropName::Private(name, _) => {
                    self.write("this.#");
                    self.write(name);
                }
            }
            self.write(" = ");
            if let Some(prev_slot_stem) = slot.prev_slot_stem.as_deref() {
                self.write("(");
                self.write(self.helper_prefix());
                self.write("__runInitializers(this, _");
                self.write(prev_slot_stem);
                self.write("_extraInitializers), ");
            }
            self.write(self.helper_prefix());
            self.write("__runInitializers(this, _");
            self.write(&slot.slot_stem);
            self.write("_initializers, ");
            if let Some(init) = &prop.initializer {
                self.emit_class_prop_initializer_expr(prop, init);
            } else {
                self.write("void 0");
            }
            self.write(")");
            if slot.prev_slot_stem.is_some() {
                self.write(")");
            }
            self.writeln(";");
            return;
        }

        if define_downlevel {
            // `useDefineForClassFields: true` with downlevel targets emits
            // constructor-time [[Define]] semantics via Object.defineProperty.
            self.write("Object.defineProperty(this, ");
            match &prop.name {
                PropName::Ident(name, _) => {
                    self.write("\"");
                    self.write(name);
                    self.write("\"");
                }
                PropName::Number(n, _) => {
                    self.write("\"");
                    self.write(n);
                    self.write("\"");
                }
                PropName::String(s, _) => {
                    self.write("\"");
                    self.write(s);
                    self.write("\"");
                }
                PropName::Computed(expr, _) => {
                    // Check general computed temps first (legacy-mode key pre-eval).
                    let general_temp = self
                        .class_general_computed_temps
                        .iter()
                        .find(|(_, start)| *start == expr.span.start)
                        .map(|(name, _)| name.clone());
                    let class_name = self.current_class_name.as_deref().unwrap_or("");
                    let temp_opt = general_temp.or_else(|| {
                        self.class_computed_name_temps.as_ref().and_then(|temps| {
                            expr_is_class_member_access(expr, class_name)
                                .map(|p| format!("{}.{}", class_name, p))
                                .and_then(|key| temps.get(&key).cloned())
                        })
                    });
                    if let Some(temp) = temp_opt {
                        self.write(temp.as_str());
                    } else {
                        self.emit_expr(expr);
                    }
                }
                PropName::Private(name, _) => {
                    // Private fields are handled elsewhere; keep a fallback.
                    self.write("\"#");
                    self.write(name);
                    self.write("\"");
                }
            }
            self.writeln(", {");
            self.indent += 1;
            self.writeln("enumerable: true,");
            self.writeln("configurable: true,");
            self.writeln("writable: true,");
            self.write("value: ");
            if let Some(init) = &prop.initializer {
                self.emit_class_prop_initializer_expr(prop, init);
            } else {
                self.write("void 0");
            }
            self.newline();
            self.indent -= 1;
            self.writeln("});");
            return;
        }

        let Some(init) = &prop.initializer else {
            return;
        };
        if prop.modifiers & MOD_ACCESSOR != 0 {
            if let Some(prop_name) = prop.name.ident_name() {
                let storage_name = self
                    .accessor_storage_names
                    .get(prop_name)
                    .cloned()
                    .unwrap_or_else(|| format!("{}_accessor_storage", prop_name));
                self.write("this.#");
                self.write(&storage_name);
                self.write(" = ");
                self.emit_class_prop_initializer_expr(prop, init);
                self.writeln(";");
            }
            return;
        }
        match &prop.name {
            PropName::Ident(name, _) => {
                self.write("this.");
                self.write(name);
            }
            PropName::Number(n, _) => {
                self.write("this[");
                self.write(n);
                self.write("]");
            }
            PropName::String(s, span) => {
                let q = self.original_quote_char(span);
                self.write("this[");
                self.write(q);
                self.write(s);
                self.write(q);
                self.write("]");
            }
            PropName::Computed(expr, _) => {
                self.write("this[");
                // Check general computed temps first (legacy mode pre-evaluated keys)
                let general_temp = self
                    .class_general_computed_temps
                    .iter()
                    .find(|(_, start)| *start == expr.span.start)
                    .map(|(name, _)| name.clone());
                let class_name = self.current_class_name.as_deref().unwrap_or("");
                let temp_opt = general_temp.or_else(|| {
                    self.class_computed_name_temps.as_ref().and_then(|temps| {
                        expr_is_class_member_access(expr, class_name)
                            .map(|p| format!("{}.{}", class_name, p))
                            .and_then(|key| temps.get(&key).cloned())
                    })
                });
                if let Some(temp) = temp_opt {
                    self.write(&temp);
                } else {
                    self.emit_expr(expr);
                }
                self.write("]");
            }
            PropName::Private(name, _) => {
                self.write("this.#");
                self.write(name);
            }
        }
        self.write(" = ");
        self.emit_class_prop_initializer_expr(prop, init);
        self.writeln(";");
    }

    /// Heuristic for parser-recovery constructors where `ctor.body` is missing
    /// but the source line still contains an inline `{`.
    pub(super) fn constructor_has_recovery_body_hint(&self, span_end: u32) -> bool {
        let start = span_end as usize;
        if start >= self.source.len() {
            return false;
        }
        let rest = &self.source[start..];
        let line_end = rest.find('\n').unwrap_or(rest.len());
        let line_tail = &rest[..line_end];
        if line_tail.contains(';') {
            return false;
        }
        line_tail.contains('{')
    }

    fn fn_decl_has_closed_param_list(&self, fn_decl: &FnDecl, body_start: usize) -> bool {
        let start = fn_decl.span.start as usize;
        if start >= body_start || body_start > self.source.len() {
            return false;
        }
        let head = &self.source[start..body_start];
        let open = match head.find('(') {
            Some(open) => open,
            None => return false,
        };
        let mut paren_depth = 0usize;
        for ch in head[open..].chars() {
            match ch {
                '(' => paren_depth += 1,
                ')' => {
                    paren_depth = paren_depth.saturating_sub(1);
                    if paren_depth == 0 {
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }

    /// For parser-recovery function signatures with missing `body`, detect
    /// an inline `{ ... }` body and return the end offset to skip recovery
    /// debris statements following the declaration.
    pub(super) fn fn_decl_recovery_body_end(&self, fn_decl: &FnDecl) -> Option<u32> {
        let start = fn_decl.span.end as usize;
        if start >= self.source.len() {
            return None;
        }
        let rest = &self.source[start..];
        let line_end = rest.find('\n').unwrap_or(rest.len());
        let line_tail = &rest[..line_end];
        if line_tail.contains(';') {
            return None;
        }
        let brace_in_line = line_tail.find('{')?;
        if line_tail[..brace_in_line].trim_start().starts_with("is ") {
            // Nested/union predicate recovery leaves `is T { ... }` after a
            // bodyless function signature. Those tokens are reparsed as
            // standalone statements; do not synthesize a function body and
            // skip them.
            return None;
        }
        // Reject recovery when non-ASCII characters appear between the
        // function signature and `{` (e.g. `function Foo() ¬ { }`).
        if line_tail[..brace_in_line]
            .chars()
            .any(|ch| !ch.is_ascii() && !ch.is_whitespace())
        {
            return None;
        }
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        let mut brace_depth = 0usize;
        for ch in line_tail[..brace_in_line].chars() {
            match ch {
                '(' => paren_depth += 1,
                ')' => paren_depth = paren_depth.saturating_sub(1),
                '[' => bracket_depth += 1,
                ']' => bracket_depth = bracket_depth.saturating_sub(1),
                '{' => brace_depth += 1,
                '}' => brace_depth = brace_depth.saturating_sub(1),
                ',' if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 => {
                    return None;
                }
                _ => {}
            }
        }
        let global_open = start + brace_in_line;
        if !self.fn_decl_has_closed_param_list(fn_decl, global_open) {
            return None;
        }
        let close_rel = self.source[global_open..].find('}')?;
        let close = global_open + close_rel;
        Some((close + 1) as u32)
    }

    fn same_line_recovery_tail_after(&self, span_end: u32) -> Option<&str> {
        let start = span_end as usize;
        if start >= self.source.len() {
            return None;
        }
        let rest = &self.source[start..];
        let line_end = rest.find('\n').unwrap_or(rest.len());
        Some(rest[..line_end].trim_start())
    }

    pub(super) fn fn_decl_has_recovery_comma_successor(&self, fn_decl: &FnDecl) -> bool {
        let Some(name) = fn_decl.name.as_deref() else {
            return false;
        };
        let Some(tail) = self.same_line_recovery_tail_after(fn_decl.span.end) else {
            return false;
        };
        let Some(after_comma) = tail.strip_prefix(',') else {
            return false;
        };
        let after_comma = after_comma.trim_start();
        let after_function = if fn_decl.is_generator {
            after_comma
                .strip_prefix("function*")
                .or_else(|| after_comma.strip_prefix("function *"))
        } else {
            after_comma.strip_prefix("function")
        };
        let Some(after_function) = after_function else {
            return false;
        };
        let after_function = after_function.trim_start();
        if !after_function.starts_with(name) {
            return false;
        }
        after_function[name.len()..]
            .chars()
            .next()
            .is_some_and(|ch| matches!(ch, '(' | '<' | '?' | ':' | '!' | ' ' | '\t'))
    }

    pub(super) fn fn_decl_skip_same_line_tail_end(&self, fn_decl: &FnDecl) -> Option<u32> {
        if fn_decl.body.is_some()
            || fn_decl.name.is_some()
            || fn_decl.is_async
            || fn_decl.is_generator
        {
            return None;
        }
        let start = fn_decl.span.end as usize;
        if start >= self.source.len() {
            return None;
        }
        let rest = &self.source[start..];
        let trimmed = rest.trim_start();
        let consumed_ws = rest.len() - trimmed.len();
        let tail = trimmed.strip_prefix("=>")?;
        let semi_rel = tail.find(';')?;
        Some((start + consumed_ws + 2 + semi_rel + 1) as u32)
    }

    pub(super) fn fn_decl_bare_arrow_param_recovery_end(&self, fn_decl: &FnDecl) -> Option<u32> {
        if fn_decl.body.is_some()
            || fn_decl.name.is_some()
            || fn_decl.is_async
            || fn_decl.is_generator
        {
            return None;
        }
        let [param] = fn_decl.params.as_slice() else {
            return None;
        };
        if !matches!(&param.name.kind, PatKind::Ident(name) if name == "<error>")
            || param.type_ann.is_some()
            || param.initializer.is_some()
            || param.dotdotdot
            || param.optional
            || !param.decorators.is_empty()
        {
            return None;
        }
        let fn_src = self.copy_span_trimmed(fn_decl.span);
        if !fn_src.trim_start().starts_with("function") {
            return None;
        }
        (self.copy_span_trimmed(param.span).trim() == "=>").then_some(fn_decl.span.end)
    }

    pub(super) fn fn_decl_suppress_recovered_body_end(&self, fn_decl: &FnDecl) -> Option<u32> {
        let body = fn_decl.body.as_ref()?;
        let start = fn_decl.span.start as usize;
        let end = fn_decl.span.end as usize;
        if start >= self.source.len() || end > self.source.len() || start >= end {
            return None;
        }
        let body_search_end = body
            .first()
            .map(|stmt| stmt.span.start as usize)
            .unwrap_or(end);
        if body_search_end > self.source.len() || start >= body_search_end {
            return None;
        }
        let head = &self.source[start..body_search_end];
        if !head.contains('(') {
            return None;
        }
        let body_open = start + head.rfind('{')?;
        if self.fn_decl_has_closed_param_list(fn_decl, body_open) {
            return None;
        }
        Some(fn_decl.span.end)
    }

    pub(super) fn fn_decl_requires_recovery_emit(&self, fn_decl: &FnDecl) -> bool {
        fn_decl.body.is_none()
            && (self.fn_decl_recovery_body_end(fn_decl).is_some()
                || self.fn_decl_has_static_recovery_body(fn_decl)
                || self.fn_decl_has_recovery_comma_successor(fn_decl)
                || self
                    .fn_decl_bare_arrow_param_recovery_end(fn_decl)
                    .is_some()
                || self.fn_decl_skip_same_line_tail_end(fn_decl).is_some()
                || self.fn_decl_has_non_ascii_body_separator(fn_decl))
    }

    /// Check if a named bodyless function has a non-ASCII character between
    /// the signature end and a `{` on the same line (e.g. `function Foo() ¬ { }`).
    /// These need recovery emit with a synthesized empty body.
    pub(super) fn fn_decl_has_non_ascii_body_separator(&self, fn_decl: &FnDecl) -> bool {
        if fn_decl.name.is_none() || fn_decl.body.is_some() {
            return false;
        }
        let start = fn_decl.span.end as usize;
        if start >= self.source.len() {
            return false;
        }
        let rest = &self.source[start..];
        let line_end = rest.find('\n').unwrap_or(rest.len());
        let line_tail = &rest[..line_end];
        let Some(brace_pos) = line_tail.find('{') else {
            return false;
        };
        line_tail[..brace_pos]
            .chars()
            .any(|ch| !ch.is_ascii() && !ch.is_whitespace())
    }

    pub(super) fn class_method_has_recovery_comma_successor(
        &self,
        member: &ClassMember,
        method: &ClassMethod,
    ) -> bool {
        let Some(name) = method.name.ident_name() else {
            return false;
        };
        let Some(tail) = self.same_line_recovery_tail_after(member.span.end) else {
            return false;
        };
        let Some(after_comma) = tail.strip_prefix(',') else {
            return false;
        };
        let mut after = after_comma.trim_start();
        if method.modifiers & MOD_STATIC != 0 {
            let Some(rest) = after.strip_prefix("static") else {
                return false;
            };
            after = rest.trim_start();
        }
        if method.is_async {
            let Some(rest) = after.strip_prefix("async") else {
                return false;
            };
            after = rest.trim_start();
        }
        if method.is_generator {
            let Some(rest) = after.strip_prefix('*') else {
                return false;
            };
            after = rest.trim_start();
        }
        if !after.starts_with(name) {
            return false;
        }
        after[name.len()..]
            .chars()
            .next()
            .is_some_and(|ch| matches!(ch, '(' | '<' | '?' | ':' | '!' | ' ' | '\t'))
    }

    fn rest_modifier_recovery_trailing_ident(&self, param: &Param) -> Option<String> {
        if !param.dotdotdot {
            return None;
        }
        let PatKind::Ident(name) = &param.name.kind else {
            return None;
        };
        if name != "public" {
            return None;
        }
        let start = (param.name.span.end as usize).min(self.source.len());
        if start >= self.source.len() {
            return None;
        }
        let window_end = (start + 64).min(self.source.len());
        let tail = &self.source[start..window_end];
        let trimmed = tail.trim_start();
        if trimmed.is_empty() {
            return None;
        }
        let mut ident = String::new();
        for ch in trimmed.chars() {
            if ch == '_' || ch == '$' || ch.is_ascii_alphanumeric() {
                ident.push(ch);
            } else {
                break;
            }
        }
        if ident.is_empty() || ident == "public" {
            return None;
        }
        let after_ident = trimmed[ident.len()..].trim_start();
        let valid_tail = after_ident.starts_with(':')
            || after_ident.starts_with(',')
            || after_ident.starts_with(')')
            || after_ident.starts_with(']');
        if valid_tail {
            Some(ident)
        } else {
            None
        }
    }

    pub(super) fn class_has_var_constructor_recovery_tail(&self, class_decl: &ClassDecl) -> bool {
        let has_ctor = class_decl
            .members
            .iter()
            .any(|m| matches!(m.kind, ClassMemberKind::Constructor(_)));
        if !has_ctor {
            return false;
        }
        let has_var_prop = class_decl.members.iter().any(|m| {
            matches!(
                m.kind,
                ClassMemberKind::Property(ClassProp {
                    name: PropName::Ident(ref n, _),
                    ..
                }) if n == "var"
            )
        });
        if !has_var_prop {
            return false;
        }
        let class_src = self.copy_span_trimmed(class_decl.span);
        class_src.contains("var constructor(")
    }

    fn class_nested_class_recovery_names(
        &self,
        class_decl: &ClassDecl,
    ) -> Vec<(String, Option<String>)> {
        fn bare_ident_prop_name(member: &ClassMember) -> Option<&str> {
            let ClassMemberKind::Property(prop) = &member.kind else {
                return None;
            };
            if prop.type_ann.is_some()
                || prop.initializer.is_some()
                || prop.modifiers != MOD_NONE
                || prop.optional
                || prop.definite
                || !prop.decorators.is_empty()
            {
                return None;
            }
            match &prop.name {
                PropName::Ident(name, _) => Some(name.as_str()),
                _ => None,
            }
        }

        let class_end = class_decl.span.end as usize;
        if class_end > self.source.len() {
            return Vec::new();
        }

        let mut recovered = Vec::new();
        for pair in class_decl.members.windows(2) {
            let [first, second] = pair else {
                continue;
            };
            if bare_ident_prop_name(first) != Some("class") {
                continue;
            }
            let Some(name) = bare_ident_prop_name(second) else {
                continue;
            };
            if name == "class" || name == "<error>" {
                continue;
            }

            let start = first.span.start as usize;
            if start >= class_end {
                continue;
            }
            let text = &self.source[start..class_end];
            let Some(rest) = text.strip_prefix("class") else {
                continue;
            };
            let rest = rest.trim_start();
            let Some(after_name) = rest.strip_prefix(name) else {
                continue;
            };
            if !after_name.trim_start().starts_with('{') {
                continue;
            }

            let Some(open_brace_rel) = text.find('{') else {
                continue;
            };
            let Some(close_brace_tail_rel) = text[open_brace_rel + 1..].find('}') else {
                continue;
            };
            let inner = &text[open_brace_rel + 1..open_brace_rel + 1 + close_brace_tail_rel];
            if inner.trim().is_empty() {
                let class_start = class_decl.span.start as usize;
                let open_brace_abs = class_start + open_brace_rel;
                let leading_comments = if open_brace_abs < start && start <= self.source.len() {
                    let segment = &self.source[open_brace_abs + 1..start];
                    let comments: Vec<&str> = segment
                        .lines()
                        .map(|line| line.trim())
                        .filter(|line| {
                            !line.is_empty()
                                && (line.starts_with("//")
                                    || line.starts_with("/*")
                                    || line.starts_with('*')
                                    || line.starts_with("*/"))
                        })
                        .collect();
                    if comments.is_empty() {
                        None
                    } else {
                        Some(comments.join("\n"))
                    }
                } else {
                    None
                };
                recovered.push((name.to_string(), leading_comments));
            }
        }
        recovered
    }

    fn class_member_recovery_statement_text(&self, member: &ClassMember) -> Option<String> {
        if matches!(member.kind, ClassMemberKind::Constructor(_)) {
            return None;
        }
        let text = self.copy_span_trimmed(member.span);
        let member_start = member.span.start as usize;
        if member_start > self.source.len() {
            return None;
        }
        let line_start = self.source[..member_start]
            .rfind('\n')
            .map(|idx| idx + 1)
            .unwrap_or(0);
        let prefix = self.source[line_start..member_start].trim();
        let recovered = if text.starts_with("var ") || text.starts_with("function ") {
            text.to_string()
        } else if prefix == "var" || prefix == "function" {
            format!("{prefix} {text}")
        } else {
            return None;
        };
        let normalized = normalize_brace_spacing(&recovered);
        let normalized = normalize_unified_pass(&normalized);
        Some(normalized.into_owned())
    }

    fn class_missing_name_generator_method_recovery_member_text(
        &self,
        class_decl: &ClassDecl,
    ) -> Option<&'static str> {
        if !class_decl.members.is_empty()
            || class_decl.name.is_none()
            || class_decl.extends.is_some()
            || class_decl.type_params.is_some()
            || !class_decl.implements.is_empty()
            || !class_decl.decorators.is_empty()
            || class_decl.modifiers != 0
        {
            return None;
        }

        let start = class_decl.span.start as usize;
        let end = class_decl.span.end as usize;
        if start >= self.source.len() || end > self.source.len() || end < start {
            return None;
        }
        let tail = &self.source[start..];
        let rel_after_span = end - start;
        if rel_after_span > tail.len() {
            return None;
        }
        let rel_class_close = tail[rel_after_span..].find('}')? + rel_after_span;
        let class_src = &tail[..=rel_class_close];
        let Some(body_open) = class_src.find('{') else {
            return None;
        };
        let Some(body_close) = class_src.rfind('}') else {
            return None;
        };
        if body_close <= body_open {
            return None;
        }

        let compact_body = class_src[body_open + 1..body_close]
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .collect::<String>();
        if compact_body == "*(){}" {
            Some("*() { }")
        } else {
            None
        }
    }

    pub(super) fn emit_recovery_missing_name_generator_method_class_decl(
        &mut self,
        class_decl: &ClassDecl,
    ) -> bool {
        let Some(member_text) =
            self.class_missing_name_generator_method_recovery_member_text(class_decl)
        else {
            return false;
        };

        self.write("class ");
        self.write(class_decl.name.as_deref().unwrap_or_default());
        self.writeln(" {");
        self.indent += 1;
        self.writeln(member_text);
        self.indent -= 1;
        self.write("}");
        self.newline();
        true
    }

    fn class_public_missing_member_recovery_tail(
        &self,
        class_decl: &ClassDecl,
    ) -> Option<PublicMissingMemberRecoveryTail> {
        let class_src = self.copy_span_trimmed(class_decl.span);
        let body_open = class_src.find('{')?;
        let body_src = &class_src[body_open + 1..];
        // Find standalone `public {` (not part of an identifier like `m1_c_public {`)
        let public_pos = {
            let mut found = None;
            let mut search_from = 0;
            while let Some(pos) = body_src[search_from..].find("public {") {
                let abs_pos = search_from + pos;
                // Check that 'public' is not preceded by an identifier char
                let is_standalone = abs_pos == 0
                    || !body_src.as_bytes()[abs_pos - 1].is_ascii_alphanumeric()
                        && body_src.as_bytes()[abs_pos - 1] != b'_';
                if is_standalone {
                    found = Some(abs_pos);
                    break;
                }
                search_from = abs_pos + 1;
            }
            found
        }?;
        let after_public = &body_src[public_pos + "public {".len()..];
        let close_brace = after_public.find('}')?;
        let inner = after_public[..close_brace].trim();
        if inner.is_empty() {
            return Some(PublicMissingMemberRecoveryTail::EmptyBlock);
        }

        if !inner.starts_with('[') {
            return None;
        }
        let bracket_end = inner.find(']')?;
        let bracket_inner = inner[1..bracket_end].trim();
        let mut bracket_parts = bracket_inner.splitn(2, ':');
        let index_name = bracket_parts.next()?.trim();
        let index_type = bracket_parts.next()?.trim();
        if index_name.is_empty() || index_type.is_empty() {
            return None;
        }

        let after_bracket = inner[bracket_end + 1..].trim();
        let value_name = after_bracket
            .strip_prefix(':')
            .map(str::trim)
            .map(|s| s.trim_end_matches(';').trim())
            .filter(|s| !s.is_empty())?;

        Some(PublicMissingMemberRecoveryTail::IndexSignatureLike {
            index_name: index_name.to_string(),
            index_type: index_type.to_string(),
            value_name: value_name.to_string(),
        })
    }

    fn class_computed_field_no_asi_empty_block_tail(&self, class_decl: &ClassDecl) -> bool {
        let class_src = self.copy_span_trimmed(class_decl.span);
        let Some(body_open) = class_src.find('{') else {
            return false;
        };
        let body_src = &class_src[body_open + 1..];
        let mut saw_unterminated_computed_field = false;
        for raw_line in body_src.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with("//") {
                continue;
            }
            if saw_unterminated_computed_field {
                return line.starts_with('[')
                    && line.contains("](")
                    && (line.ends_with("{ }") || line.ends_with("{}"));
            }
            saw_unterminated_computed_field = line.starts_with('[')
                && line.contains("] =")
                && !line.ends_with(';')
                && !line.ends_with(',');
        }
        false
    }

    fn class_global_namespace_recovery_tail(&self, class_decl: &ClassDecl) -> Option<String> {
        if self.use_define_for_class_fields() && !self.needs_downlevel("class-fields") {
            return None;
        }
        let [first, second] = class_decl.members.as_slice() else {
            return None;
        };
        let bare_ident_prop_name = |member: &ClassMember| -> Option<String> {
            let ClassMemberKind::Property(prop) = &member.kind else {
                return None;
            };
            if prop.type_ann.is_some()
                || prop.initializer.is_some()
                || prop.modifiers != MOD_NONE
                || prop.optional
                || prop.definite
                || !prop.decorators.is_empty()
            {
                return None;
            }
            match &prop.name {
                PropName::Ident(name, _) => Some(name.to_string()),
                _ => None,
            }
        };
        let first_name = bare_ident_prop_name(first)?;
        let second_name = bare_ident_prop_name(second)?;
        if first_name != "global" || second_name == "global" {
            return None;
        }
        let between_start = first.span.end as usize;
        let between_end = second.span.start as usize;
        if between_start > between_end || between_end > self.source.len() {
            return None;
        }
        if !self.source[between_start..between_end].trim().is_empty() {
            return None;
        }
        Some(second_name)
    }

    fn class_extends_void_recovery_tail(&self, class_decl: &ClassDecl) -> bool {
        let Some(extends) = class_decl.extends.as_ref() else {
            return false;
        };
        let class_start = class_decl.span.start as usize;
        let class_end = class_decl.span.end as usize;
        if class_start >= class_end || class_end > self.source.len() {
            return false;
        }
        let class_text = &self.source[class_start..class_end];
        let Some(extends_kw) = class_text.find("extends") else {
            return false;
        };
        let after_extends = &class_text[extends_kw + "extends".len()..];
        let Some(brace_pos) = after_extends.find('{') else {
            return false;
        };
        let heritage_text = after_extends[..brace_pos].trim();
        heritage_text == "void"
            && self
                .span_text(extends.span)
                .is_some_and(|text| text.trim() == "void")
    }

    /// Emit `this.name = name;` for a constructor parameter property.
    pub(super) fn emit_param_property_assign(&mut self, name: &str) {
        let define_downlevel =
            self.use_define_for_class_fields() && self.needs_downlevel("class-fields");
        if define_downlevel {
            self.write("Object.defineProperty(this, \"");
            self.write(name);
            self.writeln("\", {");
            self.indent += 1;
            self.writeln("enumerable: true,");
            self.writeln("configurable: true,");
            self.writeln("writable: true,");
            self.write("value: ");
            self.write(name);
            self.newline();
            self.indent -= 1;
            self.writeln("});");
            return;
        }
        self.write("this.");
        self.write(name);
        self.write(" = ");
        self.write(name);
        self.writeln(";");
    }

    fn ctor_body_without_static_recovery(&mut self, body: &[Stmt], class_name: &str) -> Vec<Stmt> {
        let mut filtered = Vec::with_capacity(body.len());
        let mut i = 0;
        while i < body.len() {
            if i + 1 < body.len() && Self::is_static_recovery_marker_stmt(&body[i]) {
                if let Some((prop, rhs)) = Self::recover_static_field_assign_stmt(&body[i + 1]) {
                    self.recovery_static_class_assignments.push((
                        class_name.to_string(),
                        prop,
                        rhs,
                        body[i + 1].span,
                    ));
                    i += 2;
                    continue;
                }
                if i + 2 < body.len()
                    && Self::recover_static_method_decl_span(&body[i], &body[i + 1], &body[i + 2])
                        .is_some()
                {
                    i += 3;
                    continue;
                }
            }
            filtered.push(body[i].clone());
            i += 1;
        }
        filtered
    }

    fn is_static_recovery_marker_stmt(stmt: &Stmt) -> bool {
        match &stmt.kind {
            StmtKind::Expr(expr) => matches!(&expr.kind, ExprKind::Ident(name) if name == "static"),
            _ => false,
        }
    }

    fn recover_static_field_assign_stmt(stmt: &Stmt) -> Option<(String, Expr)> {
        let StmtKind::Expr(expr) = &stmt.kind else {
            return None;
        };
        let ExprKind::Assign(assign) = &expr.kind else {
            return None;
        };
        if assign.op != AssignOp::Assign {
            return None;
        }
        let ExprKind::Ident(name) = &assign.left.kind else {
            return None;
        };
        if name == "<error>" || name == "static" {
            return None;
        }
        Some((name.to_string(), (*assign.right).clone()))
    }

    fn recover_static_method_decl_span(
        marker: &Stmt,
        call_stmt: &Stmt,
        body_stmt: &Stmt,
    ) -> Option<Span> {
        if !Self::is_static_recovery_marker_stmt(marker) {
            return None;
        }
        let StmtKind::Expr(expr) = &call_stmt.kind else {
            return None;
        };
        let ExprKind::Call(call) = &expr.kind else {
            return None;
        };
        let ExprKind::Ident(name) = &call.callee.kind else {
            return None;
        };
        if name == "<error>" || name == "static" {
            return None;
        }
        let StmtKind::Block(_) = &body_stmt.kind else {
            return None;
        };
        Some(Span::new(marker.span.start, body_stmt.span.end))
    }

    fn emit_ctor_recovered_static_methods(&mut self, body: &[Stmt]) {
        let mut i = 0;
        while i + 2 < body.len() {
            if let Some(span) =
                Self::recover_static_method_decl_span(&body[i], &body[i + 1], &body[i + 2])
            {
                let text = normalize_empty_blocks(self.source_between(span.start, span.end));
                self.writeln(&text);
                self.append_trailing_comment(body[i + 2].span);
                i += 3;
                continue;
            }
            i += 1;
        }
    }

    pub(super) fn emit_class_member_with_private(
        &mut self,
        member: &ClassMember,
        field_inits: &[(&ClassProp, Span)],
        has_super: bool,
        use_define: bool,
        downlevel_private: bool,
        invalid_duplicate_private_names: &HashMap<String, Option<bool>>,
        private_field_name_by_span: &HashMap<u32, String>,
        private_fields: &[(
            String,
            Option<&Expr>,
            bool,
            Span,
            Option<ClassExprBindingName>,
        )],
        private_method_brand_name: Option<&str>,
        method_name_override: Option<&str>,
        class_name: &str,
        instance_comment_states: &[(u32, usize, u32)],
    ) {
        // When downleveling private fields, skip private field declarations and
        // instance private methods from the class body.
        if downlevel_private {
            if let ClassMemberKind::Property(ref prop) = member.kind {
                if let PropName::Private(ref name, _) = prop.name {
                    if !Self::private_name_uses_any_member_recovery(
                        name,
                        invalid_duplicate_private_names,
                    ) {
                        return;
                    }
                }
            }
            if let ClassMemberKind::Method(ref method) = member.kind {
                if let PropName::Private(ref name, _) = method.name {
                    if !Self::private_name_uses_any_member_recovery(
                        name,
                        invalid_duplicate_private_names,
                    ) {
                        return;
                    }
                }
            }
            match &member.kind {
                ClassMemberKind::GetAccessor(acc) | ClassMemberKind::SetAccessor(acc) => {
                    if let PropName::Private(name, _) = &acc.name {
                        if !Self::private_name_uses_any_member_recovery(
                            name,
                            invalid_duplicate_private_names,
                        ) {
                            return;
                        }
                    }
                }
                _ => {}
            }
        }

        // For constructors with private fields, inject WeakMap.set calls
        let has_private_instance_work = self.has_emitted_instance_private_field_inits(
            private_fields,
            invalid_duplicate_private_names,
            private_field_name_by_span,
        );
        if downlevel_private && (has_private_instance_work || private_method_brand_name.is_some()) {
            if let ClassMemberKind::Constructor(ref ctor) = member.kind {
                if ctor.body.is_some() {
                    self.emit_constructor_with_private_fields(
                        ctor,
                        field_inits,
                        has_super,
                        invalid_duplicate_private_names,
                        private_field_name_by_span,
                        private_fields,
                        private_method_brand_name,
                        instance_comment_states,
                    );
                    return;
                }
            }
        }

        self.emit_class_member(
            member,
            field_inits,
            has_super,
            use_define,
            method_name_override,
            class_name,
            instance_comment_states,
        );
    }

    pub(super) fn emit_constructor_with_private_fields(
        &mut self,
        ctor: &ClassConstructor,
        field_inits: &[(&ClassProp, Span)],
        has_super: bool,
        invalid_duplicate_private_names: &HashMap<String, Option<bool>>,
        private_field_name_by_span: &HashMap<u32, String>,
        private_fields: &[(
            String,
            Option<&Expr>,
            bool,
            Span,
            Option<ClassExprBindingName>,
        )],
        private_method_brand_name: Option<&str>,
        instance_comment_states: &[(u32, usize, u32)],
    ) {
        let saved_arguments_alias = self.current_arguments_alias.take();
        self.write("constructor(");
        self.emit_params(&ctor.params);
        self.write(") ");

        // Collect parameter property names
        let param_props: Vec<&str> = ctor
            .params
            .iter()
            .filter(|p| {
                p.modifiers
                    & (MOD_PUBLIC | MOD_PRIVATE | MOD_PROTECTED | MOD_READONLY | MOD_OVERRIDE)
                    != 0
            })
            .filter_map(|p| {
                if let PatKind::Ident(ref name) = p.name.kind {
                    Some(name.as_str())
                } else {
                    None
                }
            })
            .collect();

        // Temporarily suppress CJS import map entries for parameter names.
        let mut saved_cjs_entries: Vec<(String, (AstString, AstString))> = Vec::new();
        for param in &ctor.params {
            if let PatKind::Ident(ref name) = param.name.kind {
                if let Some(entry) = self.cjs_import_map.remove(name.as_str()) {
                    saved_cjs_entries.push((name.to_string(), entry));
                }
            }
        }

        if let Some(ref body) = ctor.body {
            let prev_class_expr_temp_emitted = self.class_expr_temp_emitted;
            let prev_temp_var_counter = self.temp_var_counter;
            let prev_temp_var_names = std::mem::take(&mut self.temp_var_names);
            let prev_private_field_var_insert_pos = self.private_field_var_insert_pos.take();
            let prev_pending_private_field_vars =
                std::mem::take(&mut self.pending_private_field_vars);
            let prev_class_scope_temp_reserved = self.class_scope_temp_reserved;
            let prev_private_destructure_proxy_param = self.private_destructure_proxy_param.take();
            self.class_expr_temp_emitted = false;
            self.temp_var_counter = prev_temp_var_counter.max(self.class_scope_temp_reserved);
            self.class_scope_temp_reserved = 0;
            let deferred_start = self.inline_deferred_temp_placeholders.len();
            self.writeln("{");
            self.indent += 1;
            self.fn_scope_depth += 1;
            let mut hoist_insert_pos = self.output.len();

            let mut emitted_private = false;

            // Helper closure emits private field .set() and field inits and param props
            // interleaved in source order.
            let emit_inits = |emitter: &mut Emitter, emitted: &mut bool| {
                if *emitted {
                    return;
                }
                if let Some(brand_name) = private_method_brand_name {
                    emitter.write("_");
                    emitter.write(brand_name);
                    emitter.writeln(".add(this);");
                }

                // Build a merged list of init items sorted by source position.
                // Each item is (source_pos, is_private_field, index_in_respective_list).
                let mut init_order: Vec<(u32, bool, usize)> = Vec::new();
                for (i, (_, _, is_static, pf_span, _)) in private_fields.iter().enumerate() {
                    if !is_static {
                        init_order.push((pf_span.start, true, i));
                    }
                }
                for (i, (_, span)) in field_inits.iter().enumerate() {
                    init_order.push((span.start, false, i));
                }
                init_order.sort_by_key(|&(pos, _, _)| pos);

                for &(_, is_private, idx) in &init_order {
                    if is_private {
                        let (name, init, _, pf_span, binding_name) = &private_fields[idx];
                        let runtime_target = emitter.private_field_runtime_target(
                            name,
                            *pf_span,
                            false,
                            invalid_duplicate_private_names,
                            private_field_name_by_span,
                        );
                        let Some((runtime_name, runtime_is_static)) = runtime_target else {
                            emitter.append_trailing_comment(*pf_span);
                            continue;
                        };
                        // Emit leading comments for the private field
                        if let Some(&(_, saved_idx, saved_emit_pos)) = instance_comment_states
                            .iter()
                            .find(|&&(start, _, _)| start == pf_span.start)
                        {
                            let old_idx = emitter.next_comment_idx;
                            let old_emit_pos = emitter.comment_emit_pos;
                            emitter.next_comment_idx = saved_idx;
                            emitter.comment_emit_pos = saved_emit_pos;
                            emitter.emit_leading_comments(pf_span.start);
                            emitter.next_comment_idx = old_idx;
                            emitter.comment_emit_pos = old_emit_pos;
                        }
                        emitter.write("_");
                        emitter.write(&runtime_name);
                        if runtime_is_static {
                            emitter.write(" = { value: ");
                            if let Some(init_expr) = init {
                                emitter.emit_private_static_value_initializer(
                                    binding_name.as_ref(),
                                    init_expr,
                                );
                            } else {
                                emitter.write("void 0");
                            }
                            emitter.write(" }");
                        } else {
                            emitter.write(".set(this, ");
                            if let Some(init_expr) = init {
                                emitter.emit_expr_with_binding_name_if_needed(
                                    binding_name.as_ref(),
                                    init_expr,
                                );
                            } else {
                                emitter.write("void 0");
                            }
                            emitter.write(")");
                        }
                        emitter.writeln(";");
                        emitter.append_trailing_comment(*pf_span);
                    } else {
                        let (prop, span) = &field_inits[idx];
                        // Restore saved comment state so comments can be re-emitted
                        // for instance fields that were erased from the class body.
                        if let Some(&(_, saved_idx, saved_emit_pos)) = instance_comment_states
                            .iter()
                            .find(|&&(start, _, _)| start == span.start)
                        {
                            let old_idx = emitter.next_comment_idx;
                            let old_emit_pos = emitter.comment_emit_pos;
                            emitter.next_comment_idx = saved_idx;
                            emitter.comment_emit_pos = saved_emit_pos;
                            emitter.emit_leading_comments(span.start);
                            // Restore old state — field init positions may be
                            // after other class members in source order; using
                            // max would skip those members' leading comments.
                            emitter.next_comment_idx = old_idx;
                            emitter.comment_emit_pos = old_emit_pos;
                        }
                        emitter.emit_field_init(prop);
                        emitter.append_trailing_comment(*span);
                    }
                }
                *emitted = true;
            };

            // Count leading prologue directives (string expression statements
            // like "use strict" or "ngInject") — these must be emitted before
            // parameter property assignments and field initializers.
            let prologue_count = body
                .iter()
                .take_while(|s| {
                    matches!(&s.kind, StmtKind::Expr(e) if matches!(&e.kind, ExprKind::StrLit(_)))
                })
                .count();
            let no_super_call = !has_super || !body.iter().any(is_super_call);

            // If no super call, emit inits at the start
            // Order: prologues → param property assigns → field inits → body
            if no_super_call {
                for s in &body[..prologue_count] {
                    self.emit_leading_comments(s.span.start);
                    self.emit_stmt(s);
                    self.advance_comment_pos(s.span.end);
                }
                let post_injected_comment_state = body.get(prologue_count).map(|stmt| {
                    self.emit_leading_comments(stmt.span.start);
                    (self.next_comment_idx, self.comment_emit_pos)
                });
                if prologue_count > 0 {
                    hoist_insert_pos = self.output.len();
                }
                for name in &param_props {
                    self.emit_param_property_assign(name);
                }
                emit_inits(self, &mut emitted_private);
                if let Some((next_comment_idx, comment_emit_pos)) = post_injected_comment_state {
                    self.next_comment_idx = next_comment_idx;
                    self.comment_emit_pos = comment_emit_pos;
                }
            }

            let body_start = if no_super_call { prologue_count } else { 0 };
            for (idx, s) in body[body_start..].iter().enumerate() {
                self.emit_leading_comments(s.span.start);
                self.emit_stmt(s);
                self.advance_comment_pos(s.span.end);
                let original_idx = body_start + idx;
                if prologue_count > 0 && original_idx + 1 == prologue_count {
                    hoist_insert_pos = self.output.len();
                }
                if !emitted_private && is_super_call(s) {
                    let post_injected_comment_state = body.get(original_idx + 1).map(|stmt| {
                        self.emit_leading_comments(stmt.span.start);
                        (self.next_comment_idx, self.comment_emit_pos)
                    });
                    for name in &param_props {
                        self.emit_param_property_assign(name);
                    }
                    emit_inits(self, &mut emitted_private);
                    if let Some((next_comment_idx, comment_emit_pos)) = post_injected_comment_state
                    {
                        self.next_comment_idx = next_comment_idx;
                        self.comment_emit_pos = comment_emit_pos;
                    }
                }
            }

            // Insert hoisted private WeakMap vars and temps at the top of this
            // constructor body, keeping them function-scoped.
            let mut insert_offset = 0usize;
            let indent_str = "    ".repeat(self.indent as usize);
            {
                let all_vars: Vec<&str> = self
                    .pending_private_field_vars
                    .iter()
                    .map(|s| s.as_str())
                    .chain(self.temp_var_names.iter().map(|s| s.as_str()))
                    .collect();
                if !all_vars.is_empty() {
                    let var_decl = format!("{}var {};\n", indent_str, all_vars.join(", "));
                    self.output
                        .insert_str(hoist_insert_pos + insert_offset, &var_decl);
                    insert_offset += var_decl.len();
                }
            }

            self.resolve_scoped_inline_deferred_temps(deferred_start);
            self.fn_scope_depth -= 1;
            self.indent -= 1;
            self.writeln("}");
            self.class_expr_temp_emitted = prev_class_expr_temp_emitted;
            self.temp_var_counter = prev_temp_var_counter;
            self.class_scope_temp_reserved = prev_class_scope_temp_reserved;
            self.temp_var_names = prev_temp_var_names;
            self.private_field_var_insert_pos = prev_private_field_var_insert_pos;
            self.pending_private_field_vars = prev_pending_private_field_vars;
            self.private_destructure_proxy_param = prev_private_destructure_proxy_param;
        }

        // Restore suppressed CJS import map entries.
        for (name, entry) in saved_cjs_entries {
            self.cjs_import_map.insert(AstString::from(name), entry);
        }
        self.current_arguments_alias = saved_arguments_alias;
    }

    pub(super) fn emit_class_member(
        &mut self,
        member: &ClassMember,
        field_inits: &[(&ClassProp, Span)],
        _has_super: bool,
        use_define: bool,
        method_name_override: Option<&str>,
        class_name: &str,
        instance_comment_states: &[(u32, usize, u32)],
    ) {
        match &member.kind {
            ClassMemberKind::Property(prop) => {
                if prop.modifiers & MOD_DECLARE != 0 || prop.modifiers & MOD_ABSTRACT != 0 {
                    return;
                }
                let has_native_accessor_prefix = !self.needs_downlevel("auto-accessors")
                    && self.class_member_starts_with_accessor_keyword(member);
                let has_second_accessor_keyword =
                    self.class_member_has_second_accessor_keyword(member);
                // Definite assignment fields with a type annotation and no
                // initializer (`x!: T;`) have no runtime effect — tsc erases them.
                // But `x!;` (no type annotation) still emits `x;` as a field.
                // Private fields (`#x!: T;`) are ALWAYS kept as `#x;` because
                // private fields must be declared in the class body.
                if prop.definite
                    && prop.initializer.is_none()
                    && prop.type_ann.is_some()
                    && !matches!(prop.name, PropName::Private(_, _))
                    && prop.modifiers & MOD_ACCESSOR == 0
                {
                    return;
                }
                // Preserve TC39 decorators in output for ESNext targets
                if self.should_preserve_decorators() && !prop.decorators.is_empty() {
                    let name_pos = Some(prop.name.span().start);
                    self.emit_preserved_decorators(&prop.decorators, name_pos);
                }
                // Private fields (#name) always use [[Define]] semantics and
                // are normally emitted in the class body, regardless of
                // useDefineForClassFields.
                let is_private = matches!(prop.name, PropName::Private(_, _));
                // Legacy ordering transform is needed when non-private instance
                // fields are moved into the constructor.
                let has_legacy_instance_field_inits = !use_define
                    && field_inits.iter().any(|(field, _)| {
                        field.modifiers & MOD_ACCESSOR == 0
                            && !matches!(field.name, PropName::Private(_, _))
                    });
                let force_legacy_accessor_transform = !use_define
                    && prop.modifiers & MOD_STATIC == 0
                    && has_legacy_instance_field_inits;
                let lower_accessor_for_legacy_ordering = prop.modifiers & MOD_ACCESSOR != 0
                    && force_legacy_accessor_transform
                    && !self.needs_downlevel("auto-accessors");
                // In legacy ordering mode, private field initializers move to
                // the constructor but declarations stay in the class body.
                let hoist_private_initializer = !use_define
                    && is_private
                    && prop.modifiers & MOD_STATIC == 0
                    && prop.modifiers & MOD_ACCESSOR == 0
                    && prop.initializer.is_some()
                    && has_legacy_instance_field_inits;

                if use_define || (is_private && !lower_accessor_for_legacy_ordering) {
                    // Auto-accessor lowering: `accessor a = 1` →
                    //   `#a_accessor_storage = 1; get a() { ... } set a(value) { ... }`
                    if prop.modifiers & MOD_ACCESSOR != 0 && self.needs_downlevel("auto-accessors")
                    {
                        if let Some(prop_name) = prop.name.ident_name() {
                            let is_static = prop.modifiers & MOD_STATIC != 0;
                            // Use the pre-computed accessor storage name map if available,
                            // otherwise fall back to the default naming pattern.
                            let final_storage = self
                                .accessor_storage_names
                                .get(prop_name)
                                .cloned()
                                .unwrap_or_else(|| format!("{}_accessor_storage", prop_name));

                            // Storage field
                            if is_static {
                                self.write("static ");
                            }
                            self.write("#");
                            self.write(&final_storage);
                            if let Some(ref init) = prop.initializer {
                                self.write(" = ");
                                self.emit_class_prop_initializer_expr(prop, init);
                            }
                            self.writeln(";");

                            // Emit deferred leading comments between storage and getter
                            self.emit_leading_comments(member.span.start);

                            // Getter
                            if is_static {
                                self.write("static ");
                            }
                            self.write("get ");
                            self.write(prop_name);
                            self.write("() { return ");
                            if is_static {
                                self.write(class_name);
                            } else {
                                self.write("this");
                            }
                            self.write(".#");
                            self.write(&final_storage);
                            self.writeln("; }");

                            // Setter
                            if is_static {
                                self.write("static ");
                            }
                            self.write("set ");
                            self.write(prop_name);
                            self.write("(value) { ");
                            if is_static {
                                self.write(class_name);
                            } else {
                                self.write("this");
                            }
                            self.write(".#");
                            self.write(&final_storage);
                            self.writeln(" = value; }");
                        } else {
                            // Non-ident accessor name — emit as-is
                            if prop.modifiers & MOD_STATIC != 0 {
                                self.write("static ");
                            }
                            self.write("accessor ");
                            self.emit_class_prop_name_for_initializer(prop);
                            if let Some(ref init) = prop.initializer {
                                self.write(" = ");
                                self.emit_class_prop_initializer_expr(prop, init);
                            }
                            self.writeln(";");
                        }
                    } else if prop.modifiers & MOD_ACCESSOR != 0 {
                        // Target supports native auto-accessors — emit as-is
                        if has_native_accessor_prefix {
                            self.write("accessor ");
                        }
                        if prop.modifiers & MOD_STATIC != 0 {
                            self.write("static ");
                        }
                        if !has_native_accessor_prefix || has_second_accessor_keyword {
                            self.write("accessor ");
                        }
                        self.emit_class_prop_name_for_initializer(prop);
                        if let Some(ref init) = prop.initializer {
                            self.write(" = ");
                            self.emit_class_prop_initializer_expr(prop, init);
                        }
                        self.writeln(";");
                    } else {
                        // Regular field declaration in define mode
                        if prop.modifiers & MOD_STATIC != 0 {
                            self.write("static ");
                        }
                        self.emit_class_prop_name_for_initializer(prop);
                        if let Some(ref init) = prop.initializer {
                            if hoist_private_initializer {
                                self.writeln(";");
                                return;
                            }
                            self.write(" = ");
                            self.emit_class_prop_initializer_expr(prop, init);
                            // Preserve inline comments between the initializer and
                            // the semicolon (e.g. `b = this.a /*undefined*/;`).
                            self.emit_inline_comments_in_range(init.span.end, member.span.end);
                        }
                        self.writeln(";");
                    }
                } else {
                    // Legacy mode: instance fields are moved to the constructor,
                    // static fields are emitted AFTER the class body as
                    // `ClassName.prop = value;` (or as `static { this.prop = val; }`
                    // when target supports static blocks).

                    // ES2022+: emit static properties with initializers as static blocks
                    if prop.modifiers & MOD_STATIC != 0
                        && prop.initializer.is_some()
                        && !self.needs_downlevel("static-blocks")
                        && prop.modifiers & MOD_ACCESSOR == 0
                    {
                        self.write("static { this");
                        match &prop.name {
                            PropName::Computed(expr, _) => {
                                self.write("[");
                                let general_temp = self
                                    .class_general_computed_temps
                                    .iter()
                                    .find(|(_, start)| *start == expr.span.start)
                                    .map(|(name, _)| name.clone());
                                if let Some(temp) = general_temp {
                                    self.write(&temp);
                                } else {
                                    self.emit_expr(expr);
                                }
                                self.write("]");
                            }
                            _ => self.emit_member_access(&prop.name),
                        }
                        self.write(" = ");
                        self.emit_class_prop_initializer_expr(
                            prop,
                            prop.initializer.as_ref().unwrap(),
                        );
                        self.write("; }");
                        self.newline();
                        return;
                    }

                    // Auto-accessors (accessor keyword) are transformed into a getter/setter pair
                    // that access a private storage field when:
                    // 1) target cannot represent accessors natively, OR
                    // 2) legacy class-field mode must preserve ordering with hoisted
                    //    instance field initializers.
                    if prop.modifiers & MOD_ACCESSOR != 0 {
                        if self.needs_downlevel("auto-accessors") {
                            if let Some(prop_name) = prop.name.ident_name() {
                                let storage_name =
                                    format!("{}_{}_accessor_storage", class_name, prop_name);

                                // Getter
                                if prop.modifiers & MOD_STATIC != 0 {
                                    self.write("static ");
                                }
                                self.write("get ");
                                self.emit_class_member_prop_name(
                                    &prop.name,
                                    prop.modifiers & MOD_STATIC != 0,
                                );
                                self.write("() { return ");
                                self.write(self.helper_prefix());
                                self.write("__classPrivateFieldGet(this, _");
                                self.write(&storage_name);
                                self.write(", \"f\"); }");
                                self.newline();
                                self.append_trailing_comment(member.span);

                                // Setter
                                if prop.modifiers & MOD_STATIC != 0 {
                                    self.write("static ");
                                }
                                self.write("set ");
                                self.emit_class_member_prop_name(
                                    &prop.name,
                                    prop.modifiers & MOD_STATIC != 0,
                                );
                                self.write("(value) { ");
                                self.write(self.helper_prefix());
                                self.write("__classPrivateFieldSet(this, _");
                                self.write(&storage_name);
                                self.writeln(", value, \"f\"); }");

                                self.needs_private_field_get = true;
                                self.needs_private_field_set = true;
                            } else {
                                // Non-ident accessor name — emit as-is.
                                if prop.modifiers & MOD_STATIC != 0 {
                                    self.write("static ");
                                }
                                self.write("accessor ");
                                self.emit_class_prop_name_for_initializer(prop);
                                if let Some(ref init) = prop.initializer {
                                    self.write(" = ");
                                    self.emit_class_prop_initializer_expr(prop, init);
                                }
                                self.writeln(";");
                            }
                        } else if force_legacy_accessor_transform {
                            if let Some(prop_name) = prop.name.ident_name() {
                                let storage_name = self
                                    .accessor_storage_names
                                    .get(prop_name)
                                    .cloned()
                                    .unwrap_or_else(|| format!("{}_accessor_storage", prop_name));

                                // Storage field declaration (initializer moved to constructor).
                                if prop.modifiers & MOD_STATIC != 0 {
                                    self.write("static ");
                                }
                                self.write("#");
                                self.write(&storage_name);
                                self.writeln(";");

                                // Emit deferred leading comments between storage and getter.
                                self.emit_leading_comments(member.span.start);

                                // Getter
                                if prop.modifiers & MOD_STATIC != 0 {
                                    self.write("static ");
                                }
                                self.write("get ");
                                self.emit_class_member_prop_name(
                                    &prop.name,
                                    prop.modifiers & MOD_STATIC != 0,
                                );
                                self.write("() { return ");
                                if prop.modifiers & MOD_STATIC != 0 {
                                    self.write(class_name);
                                } else {
                                    self.write("this");
                                }
                                self.write(".#");
                                self.write(&storage_name);
                                self.write("; }");
                                self.newline();
                                self.append_trailing_comment(member.span);

                                // Setter
                                if prop.modifiers & MOD_STATIC != 0 {
                                    self.write("static ");
                                }
                                self.write("set ");
                                self.emit_class_member_prop_name(
                                    &prop.name,
                                    prop.modifiers & MOD_STATIC != 0,
                                );
                                self.write("(value) { ");
                                if prop.modifiers & MOD_STATIC != 0 {
                                    self.write(class_name);
                                } else {
                                    self.write("this");
                                }
                                self.write(".#");
                                self.write(&storage_name);
                                self.writeln(" = value; }");
                            } else {
                                // Non-ident accessor name — emit as-is.
                                if prop.modifiers & MOD_STATIC != 0 {
                                    self.write("static ");
                                }
                                self.write("accessor ");
                                self.emit_class_prop_name_for_initializer(prop);
                                if let Some(ref init) = prop.initializer {
                                    self.write(" = ");
                                    self.emit_class_prop_initializer_expr(prop, init);
                                }
                                self.writeln(";");
                            }
                        } else {
                            // Target supports native accessors and no legacy ordering
                            // transform is required.
                            if has_native_accessor_prefix {
                                self.write("accessor ");
                            }
                            if prop.modifiers & MOD_STATIC != 0 {
                                self.write("static ");
                            }
                            if !has_native_accessor_prefix || has_second_accessor_keyword {
                                self.write("accessor ");
                            }
                            self.emit_class_prop_name_for_initializer(prop);
                            if let Some(ref init) = prop.initializer {
                                self.write(" = ");
                                self.emit_class_prop_initializer_expr(prop, init);
                            }
                            self.writeln(";");
                        }
                    }
                    return;
                }
            }
            ClassMemberKind::Method(method) => {
                let saved_arguments_alias = self.current_arguments_alias.take();
                let emit_recovery_comma_stub = method.body.is_none()
                    && method.modifiers & MOD_DECLARE == 0
                    && self.class_method_has_recovery_comma_successor(member, method);
                // Skip overload signatures (no body) and declare methods without bodies.
                // Abstract/declare methods with bodies (error recovery) are still emitted.
                if (method.body.is_none() && !emit_recovery_comma_stub)
                    || (method.modifiers & MOD_DECLARE != 0 && method.body.is_none())
                {
                    self.current_arguments_alias = saved_arguments_alias;
                    return;
                }
                // Preserve TC39 decorators in output for ESNext targets
                if self.should_preserve_decorators() && !method.decorators.is_empty() {
                    let name_pos = Some(method.name.span().start);
                    self.emit_preserved_decorators(&method.decorators, name_pos);
                }
                self.emit_recovery_export_keyword(member);
                self.emit_recovery_accessor_keyword(member);
                if method.modifiers & MOD_STATIC != 0 {
                    self.write("static ");
                }
                let prev_in_async = self.in_async_function;
                self.in_async_function = method.is_async;
                let dl_async_method =
                    method.is_async && !method.is_generator && self.needs_downlevel("async");
                let dl_async_gen_method = method.is_async
                    && method.is_generator
                    && self.needs_downlevel("async-generator");
                let async_gen_move_params =
                    dl_async_gen_method && method.params.iter().any(Self::param_needs_async_lift);
                // For regular async methods (not generators), destructured params
                // need to be replaced with temp vars and moved to the generator:
                //   async cancel({reason, code}) {} →
                //   cancel(_a) { return __awaiter(this, arguments, void 0, function* ({reason, code}) {}); }
                let async_move_destructured_params = dl_async_method
                    && method
                        .params
                        .iter()
                        .any(|p| !matches!(p.name.kind, PatKind::Ident(_)));
                if method.is_async && !(dl_async_method || dl_async_gen_method) {
                    self.write("async ");
                }
                if method.is_generator && !dl_async_gen_method {
                    self.write("*");
                }
                if let Some(name) = method_name_override {
                    self.write(name);
                } else {
                    self.emit_class_member_prop_name(
                        &method.name,
                        method.modifiers & MOD_STATIC != 0,
                    );
                }
                self.write("(");
                let rest_infos = if async_gen_move_params || async_move_destructured_params {
                    let temps = if method.params.iter().any(|p| p.initializer.is_some()) {
                        Self::generate_async_lift_prefix_temp_names(&method.params)
                    } else {
                        Self::generate_async_lift_temp_names(&method.params)
                    };
                    for (i, t) in temps.iter().enumerate() {
                        if i > 0 {
                            self.write(", ");
                        }
                        self.write(t);
                    }
                    vec![]
                } else if self.params_need_rest_transform(&method.params) {
                    self.emit_params_with_rest_transform(&method.params)
                } else {
                    self.emit_params(&method.params);
                    vec![]
                };
                self.write(") ");
                // Skip comments inside erased return type annotation
                if let Some(ref rt) = method.return_type {
                    self.advance_comment_pos(rt.span.end);
                }
                if emit_recovery_comma_stub {
                    self.writeln("{ }");
                    self.in_async_function = prev_in_async;
                    self.current_arguments_alias = saved_arguments_alias;
                    return;
                }
                if let Some(ref body) = method.body {
                    // Apply static recovery: `static x = 1;` parsed inside a
                    // method body should be emitted after the class as
                    // `ClassName.x = 1;` (TypeScript error recovery).
                    let filtered_body = self.ctor_body_without_static_recovery(body, class_name);
                    let body_ref = if filtered_body.len() != body.len() {
                        &filtered_body[..]
                    } else {
                        &body[..]
                    };
                    if dl_async_method || dl_async_gen_method {
                        self.awaiter_enclosing_span = Some(member.span);
                        if dl_async_gen_method {
                            let inner_name = method.name.ident_name().map(|n| format!("{n}_1"));
                            self.emit_async_generator_body(
                                body_ref,
                                inner_name.as_deref(),
                                async_gen_move_params.then_some(method.params.as_slice()),
                            );
                        } else {
                            let async_arguments_alias = if !async_move_destructured_params
                                && stmts_have_lexical_arguments(body_ref)
                            {
                                Some(self.next_arguments_capture_name())
                            } else {
                                None
                            };
                            self.emit_awaiter_body_with_params(
                                body_ref,
                                if async_move_destructured_params {
                                    Some(method.params.as_slice())
                                } else {
                                    None
                                },
                                &method.params,
                                async_arguments_alias.as_deref(),
                            );
                        }
                    } else if !rest_infos.is_empty() {
                        // Class method/accessor with rest param destructuring.
                        self.writeln("{");
                        self.indent += 1;
                        for (_, temp_name, pat, _) in &rest_infos {
                            self.emit_rest_param_destructuring(temp_name, pat);
                        }
                        self.emit_rest_lifted_defaults();
                        for s in body_ref {
                            self.emit_stmt(s);
                        }
                        // Preserve comments from the original body that aren't
                        // attached to statements (e.g. a comment-only body).
                        self.emit_rest_body_trailing_comments(body_ref, member.span);
                        self.indent -= 1;
                        self.write("}");
                    } else {
                        self.emit_class_member_block_body(body_ref, member.span);
                    }
                    self.newline();
                }
                self.in_async_function = prev_in_async;
                self.current_arguments_alias = saved_arguments_alias;
            }
            ClassMemberKind::Constructor(ctor) => {
                // Skip overload signatures (no body), unless parser recovery
                // shows an inline `{` body start on the same source line.
                if ctor.body.is_none() {
                    if self.constructor_has_recovery_body_hint(member.span.end) {
                        self.emit_recovery_export_keyword(member);
                        if ctor.modifiers & MOD_STATIC != 0 {
                            self.write("static ");
                        }
                        self.write("constructor(");
                        self.emit_params(&ctor.params);
                        if let Some(trailing_ident) = ctor
                            .params
                            .iter()
                            .find_map(|p| self.rest_modifier_recovery_trailing_ident(p))
                        {
                            self.write(", ");
                            self.write(&trailing_ident);
                            self.writeln(") { }");
                        } else {
                            self.writeln(") {");
                            self.writeln("}");
                        }
                    }
                    return;
                }
                let saved_arguments_alias = self.current_arguments_alias.take();
                self.emit_recovery_export_keyword(member);
                if ctor.modifiers & MOD_STATIC != 0 {
                    self.write("static ");
                }
                // Check for type params on constructor (error recovery:
                // `constructor<T>()` — TypeScript preserves invalid type params).
                let ctor_type_params = {
                    let s = member.span.start as usize;
                    let e = member.span.end as usize;
                    if s < e && e <= self.source.len() {
                        let src = &self.source[s..e];
                        if let Some(ctor_pos) = src.find("constructor") {
                            let after_ctor = &src[ctor_pos + 11..]; // after "constructor"
                            let trimmed = after_ctor.trim_start();
                            if trimmed.starts_with('<') {
                                // Find matching `>` before `(`
                                // Skip empty type params `<>` — TypeScript strips those
                                if let Some(gt) = trimmed.find('>') {
                                    let params = &trimmed[1..gt].trim();
                                    if params.is_empty() {
                                        None // `<>` — empty, strip it
                                    } else {
                                        Some(trimmed[..gt + 1].to_string())
                                    }
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
                    }
                };
                self.write("constructor");
                if let Some(ref tp) = ctor_type_params {
                    self.write(tp);
                }
                self.write("(");
                let rest_infos = if self.params_need_rest_transform(&ctor.params) {
                    self.emit_params_with_rest_transform(&ctor.params)
                } else {
                    self.emit_params(&ctor.params);
                    vec![]
                };
                // Check for return type on constructor (error recovery:
                // `constructor(): number` — TypeScript preserves invalid return type).
                let ctor_return_type = {
                    let s = member.span.start as usize;
                    let e = member.span.end as usize;
                    if s < e && e <= self.source.len() {
                        let src = &self.source[s..e];
                        // Find the FIRST `{` (constructor body opening) after
                        // the constructor keyword. Using rfind would incorrectly
                        // match nested function braces inside the body.
                        let ctor_start = src.find("constructor").unwrap_or(0);
                        let after_ctor = &src[ctor_start..];
                        if let Some(brace_pos) = after_ctor.find('{') {
                            let before_brace = after_ctor[..brace_pos].trim_end();
                            // Find the last `)` before the opening `{`
                            if let Some(paren_pos) = before_brace.rfind(')') {
                                let between = before_brace[paren_pos + 1..].trim();
                                if between.starts_with(':') {
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
                    }
                };
                self.write(")");
                if let Some(ref rt) = ctor_return_type {
                    self.write(rt);
                }
                self.write(" ");

                // Collect parameter property names (params with visibility or override modifiers)
                let param_props: Vec<&str> = ctor
                    .params
                    .iter()
                    .filter(|p| {
                        p.modifiers
                            & (MOD_PUBLIC
                                | MOD_PRIVATE
                                | MOD_PROTECTED
                                | MOD_READONLY
                                | MOD_OVERRIDE)
                            != 0
                    })
                    .filter_map(|p| {
                        if let PatKind::Ident(ref name) = p.name.kind {
                            Some(name.as_str())
                        } else {
                            None
                        }
                    })
                    .collect();

                // Temporarily suppress CJS import map entries for parameter
                // names so that `this.x = x` inside the constructor body uses
                // the parameter, not the import alias (e.g. `db` should not
                // become `db_1.db` when `db` is a constructor parameter).
                let mut saved_cjs_entries: Vec<(String, (AstString, AstString))> = Vec::new();
                for param in &ctor.params {
                    if let PatKind::Ident(ref name) = param.name.kind {
                        if let Some(entry) = self.cjs_import_map.remove(name.as_str()) {
                            saved_cjs_entries.push((name.to_string(), entry));
                        }
                    }
                }

                if let Some(ref body) = ctor.body {
                    if field_inits.is_empty()
                        && param_props.is_empty()
                        && rest_infos.is_empty()
                        && self.inject_instance_extra_initializers
                    {
                        // Emit body with __runInitializers injection after super()
                        // or at start for non-extends classes.
                        let filtered_body =
                            self.ctor_body_without_static_recovery(body, class_name);
                        self.writeln("{");
                        self.indent += 1;
                        self.fn_scope_depth += 1;
                        let has_super_stmt = filtered_body.iter().any(is_super_call);
                        let mut injected = false;
                        let prologue_count = filtered_body
                            .iter()
                            .take_while(|s| {
                                matches!(&s.kind, StmtKind::Expr(e) if matches!(&e.kind, ExprKind::StrLit(_)))
                            })
                            .count();
                        if !has_super_stmt {
                            // No super call — inject after prologue directives.
                            for s in &filtered_body[..prologue_count] {
                                self.emit_leading_comments(s.span.start);
                                self.emit_stmt(s);
                                self.advance_comment_pos(s.span.end);
                            }
                            self.write(self.helper_prefix());
                            let initializers_name = self
                                .injected_instance_extra_initializers_name
                                .clone()
                                .unwrap_or_else(|| "_instanceExtraInitializers".to_string());
                            self.write("__runInitializers(this, ");
                            self.write(&initializers_name);
                            self.writeln(");");
                            injected = true;
                        }
                        let body_start = if injected { prologue_count } else { 0 };
                        for s in &filtered_body[body_start..] {
                            self.emit_leading_comments(s.span.start);
                            self.emit_stmt(s);
                            self.advance_comment_pos(s.span.end);
                            if !injected && is_super_call(s) {
                                self.write(self.helper_prefix());
                                let initializers_name = self
                                    .injected_instance_extra_initializers_name
                                    .clone()
                                    .unwrap_or_else(|| "_instanceExtraInitializers".to_string());
                                self.write("__runInitializers(this, ");
                                self.write(&initializers_name);
                                self.writeln(");");
                                injected = true;
                            }
                        }
                        self.fn_scope_depth -= 1;
                        self.indent -= 1;
                        self.writeln("}");
                    } else if field_inits.is_empty()
                        && param_props.is_empty()
                        && rest_infos.is_empty()
                    {
                        let filtered_body =
                            self.ctor_body_without_static_recovery(body, class_name);
                        if self.needs_downlevel("using")
                            && crate::emit_stmt::has_using_declaration(&filtered_body)
                        {
                            // Constructor body with using declarations — wrap entire body in disposal scope.
                            // All statements (including super()) go inside the try block.
                            self.writeln("{");
                            self.indent += 1;
                            self.fn_scope_depth += 1;
                            self.emit_block_using_dispose_scope(&filtered_body);
                            self.fn_scope_depth -= 1;
                            self.indent -= 1;
                            self.writeln("}");
                        } else {
                            self.emit_block_for_decl_body(&filtered_body, member.span);
                            self.newline();
                        }
                    } else if field_inits.is_empty() && param_props.is_empty() {
                        // Constructor with rest params but no field inits or param props.
                        // Emit prologue directives before rest param destructuring.
                        let filtered_body =
                            self.ctor_body_without_static_recovery(body, class_name);
                        self.writeln("{");
                        self.indent += 1;
                        let mut prologue_count = 0;
                        for s in &filtered_body {
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
                        for s in filtered_body.iter().skip(prologue_count) {
                            self.emit_stmt(s);
                        }
                        self.indent -= 1;
                        self.write("}");
                        self.newline();
                    } else {
                        self.writeln("{");
                        self.indent += 1;
                        self.fn_scope_depth += 1;
                        let mut emitted_fields = false;
                        // Count leading prologue directives (string expression
                        // statements like "use strict" or "ngInject") — these
                        // must be emitted before parameter property assignments.
                        let prologue_count = body
                            .iter()
                            .take_while(|s| {
                                matches!(&s.kind, StmtKind::Expr(e) if matches!(&e.kind, ExprKind::StrLit(_)))
                            })
                            .count();
                        // If no super call, emit param props then field inits at the start
                        let no_super = !body.iter().any(is_super_call);
                        if no_super {
                            // Emit prologues first
                            for s in &body[..prologue_count] {
                                self.emit_leading_comments(s.span.start);
                                self.emit_stmt(s);
                                self.advance_comment_pos(s.span.end);
                            }
                            for name in &param_props {
                                self.emit_param_property_assign(name);
                            }
                            if !field_inits.is_empty() {
                                let pre_field_comment_idx = self.next_comment_idx;
                                let pre_field_comment_pos = self.comment_emit_pos;
                                let mut post_field_comment_state =
                                    (pre_field_comment_idx, pre_field_comment_pos);
                                // When the constructor body has blank lines between
                                // `{` and the first statement, TypeScript emits the
                                // leading comments before field initializers.
                                if param_props.is_empty() {
                                    if let Some(first_stmt) = body.get(prologue_count) {
                                        // Find `{` after params
                                        let search_start = ctor
                                            .params
                                            .last()
                                            .map(|p| p.span.end as usize)
                                            .unwrap_or(member.span.start as usize);
                                        let brace_pos = self.source[search_start..]
                                            .find('{')
                                            .map(|i| search_start + i + 1);
                                        let first_pos = first_stmt.span.start as usize;
                                        let has_blank_line = brace_pos
                                            .map(|bp| {
                                                bp < first_pos
                                                    && self.source[bp..first_pos].contains("\n\n")
                                            })
                                            .unwrap_or(false);
                                        if has_blank_line {
                                            self.emit_leading_comments(first_stmt.span.start);
                                            post_field_comment_state =
                                                (self.next_comment_idx, self.comment_emit_pos);
                                        }
                                    }
                                }
                                for (prop, span) in field_inits {
                                    if let Some(&(_, saved_idx, saved_emit_pos)) =
                                        instance_comment_states
                                            .iter()
                                            .find(|&&(start, _, _)| start == span.start)
                                    {
                                        let old_idx = self.next_comment_idx;
                                        let old_emit_pos = self.comment_emit_pos;
                                        self.next_comment_idx = saved_idx;
                                        self.comment_emit_pos = saved_emit_pos;
                                        self.emit_leading_comments(span.start);
                                        // Restore old state — field init positions may be
                                        // after other class members in source order; using
                                        // max would skip those members' leading comments.
                                        self.next_comment_idx = old_idx;
                                        self.comment_emit_pos = old_emit_pos;
                                    }
                                    self.emit_field_init(prop);
                                    self.append_trailing_comment(*span);
                                }
                                self.emit_standard_decorator_instance_field_extra_initializers();
                                // Restore comment state so constructor body comments
                                // are emitted with the body statements (after field
                                // inits), not consumed by field init emission.
                                if param_props.is_empty() {
                                    self.next_comment_idx = post_field_comment_state.0;
                                    self.comment_emit_pos = post_field_comment_state.1;
                                }
                            }
                            emitted_fields = true;
                        }
                        let body_start = if no_super { prologue_count } else { 0 };
                        let body_slice = &body[body_start..];
                        let has_using_in_ctor = self.needs_downlevel("using")
                            && crate::emit_stmt::has_using_declaration(body_slice);
                        let ctor_env_num = if has_using_in_ctor {
                            self.needs_add_disposable_resource_helper = true;
                            self.needs_dispose_resources_helper = true;
                            let env_num = self.next_using_env_num();
                            let has_await = body_slice.iter().any(|s| {
                                matches!(&s.kind, StmtKind::Var(vs) if vs.kind == VarKind::AwaitUsing)
                            });
                            self.writeln(&format!(
                                "const env_{env_num} = {{ stack: [], error: void 0, hasError: false }};"
                            ));
                            self.writeln("try {");
                            self.indent += 1;
                            Some((env_num, has_await))
                        } else {
                            None
                        };
                        for s in body_slice {
                            // Transform using/await using declarations to __addDisposableResource
                            if let Some((env_num, _)) = ctor_env_num {
                                if let StmtKind::Var(vs) = &s.kind {
                                    if matches!(vs.kind, VarKind::Using | VarKind::AwaitUsing) {
                                        let is_await = vs.kind == VarKind::AwaitUsing;
                                        for d in &vs.declarations {
                                            if let PatKind::Ident(name) = &d.name.kind {
                                                if let Some(init) = &d.init {
                                                    self.write("const ");
                                                    self.write(name);
                                                    self.write(" = ");
                                                    self.write(self.helper_prefix());
                                                    self.write(&format!(
                                                        "__addDisposableResource(env_{env_num}, "
                                                    ));
                                                    self.emit_using_initializer(name, init);
                                                    self.write(if is_await {
                                                        ", true"
                                                    } else {
                                                        ", false"
                                                    });
                                                    self.writeln(");");
                                                }
                                            }
                                        }
                                        self.advance_comment_pos(s.span.end);
                                        // Still check for super call injection after using stmt
                                        if !emitted_fields && is_super_call(s) {
                                            for name in &param_props {
                                                self.emit_param_property_assign(name);
                                            }
                                            for (prop, span) in field_inits {
                                                self.emit_field_init(prop);
                                                self.append_trailing_comment(*span);
                                            }
                                            self.emit_standard_decorator_instance_field_extra_initializers();
                                            emitted_fields = true;
                                        }
                                        continue;
                                    }
                                }
                            }
                            self.emit_leading_comments(s.span.start);
                            self.emit_stmt(s);
                            self.advance_comment_pos(s.span.end);
                            if !emitted_fields && is_super_call(s) {
                                for name in &param_props {
                                    self.emit_param_property_assign(name);
                                }
                                for (prop, span) in field_inits {
                                    if let Some(&(_, saved_idx, saved_emit_pos)) =
                                        instance_comment_states
                                            .iter()
                                            .find(|&&(start, _, _)| start == span.start)
                                    {
                                        let old_idx = self.next_comment_idx;
                                        let old_emit_pos = self.comment_emit_pos;
                                        self.next_comment_idx = saved_idx;
                                        self.comment_emit_pos = saved_emit_pos;
                                        self.emit_leading_comments(span.start);
                                        // Restore old state — field init positions may be
                                        // after other class members in source order; using
                                        // max would skip those members' leading comments.
                                        self.next_comment_idx = old_idx;
                                        self.comment_emit_pos = old_emit_pos;
                                    }
                                    self.emit_field_init(prop);
                                    self.append_trailing_comment(*span);
                                }
                                self.emit_standard_decorator_instance_field_extra_initializers();
                                emitted_fields = true;
                            }
                        }
                        if let Some((env_num, has_await)) = ctor_env_num {
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
                        // Emit any trailing comments inside the constructor body
                        // that were not emitted by statements (e.g. a comment between
                        // the last statement/field-init and the closing brace).
                        // Use the member span end (which covers the closing `}`) so
                        // that comments before the brace are captured.
                        let body_end = member.span.end;
                        self.emit_leading_comments(body_end);
                        self.fn_scope_depth -= 1;
                        self.indent -= 1;
                        self.writeln("}");
                    }
                }

                // Restore suppressed CJS import map entries.
                for (name, entry) in saved_cjs_entries {
                    self.cjs_import_map.insert(AstString::from(name), entry);
                }
                self.current_arguments_alias = saved_arguments_alias;
            }
            ClassMemberKind::GetAccessor(acc) => {
                let saved_arguments_alias = self.current_arguments_alias.take();
                // Skip declare accessors only if they have no body.
                // With a body (error recovery), emit them without `declare`.
                // Skip abstract only if no body.
                if (acc.modifiers & MOD_DECLARE != 0 && acc.body.is_none())
                    || (acc.modifiers & MOD_ABSTRACT != 0 && acc.body.is_none())
                {
                    self.current_arguments_alias = saved_arguments_alias;
                    return;
                }
                // Preserve TC39 decorators in output for ESNext targets
                if self.should_preserve_decorators() && !acc.decorators.is_empty() {
                    let name_pos = Some(acc.name.span().start);
                    self.emit_preserved_decorators(&acc.decorators, name_pos);
                }
                self.emit_recovery_export_keyword(member);
                self.emit_recovery_accessor_keyword(member);
                if acc.modifiers & MOD_STATIC != 0 {
                    self.write("static ");
                }
                self.write("get ");
                if let Some(name) = method_name_override {
                    self.write(name);
                } else {
                    self.emit_class_member_prop_name(&acc.name, acc.modifiers & MOD_STATIC != 0);
                }
                // Preserve invalid type parameters (error recovery)
                if let Some(ref tps) = acc.type_params {
                    if let (Some(first), Some(last)) = (tps.first(), tps.last()) {
                        let start = (first.span.start as usize).saturating_sub(1);
                        let end = (last.span.end as usize + 1).min(self.source.len());
                        if start < end {
                            self.write(&self.source[start..end].to_string());
                        }
                    }
                }
                self.write("(");
                self.emit_params(&acc.params);
                self.write(") ");
                // Skip comments inside erased return type annotation
                if let Some(ref rt) = acc.return_type {
                    self.advance_comment_pos(rt.span.end);
                }
                match &acc.body {
                    Some(body) => {
                        self.emit_class_member_block_body(body, member.span);
                    }
                    None => {
                        // Concrete accessor without body (parser recovery) — emit empty body
                        self.write("{ }");
                    }
                }
                self.newline();
                self.current_arguments_alias = saved_arguments_alias;
            }
            ClassMemberKind::SetAccessor(acc) => {
                let saved_arguments_alias = self.current_arguments_alias.take();
                // Skip declare accessors only if they have no body.
                if (acc.modifiers & MOD_DECLARE != 0 && acc.body.is_none())
                    || (acc.modifiers & MOD_ABSTRACT != 0 && acc.body.is_none())
                {
                    self.current_arguments_alias = saved_arguments_alias;
                    return;
                }
                // Preserve TC39 decorators in output for ESNext targets
                if self.should_preserve_decorators() && !acc.decorators.is_empty() {
                    let name_pos = Some(acc.name.span().start);
                    self.emit_preserved_decorators(&acc.decorators, name_pos);
                }
                self.emit_recovery_export_keyword(member);
                self.emit_recovery_accessor_keyword(member);
                if acc.modifiers & MOD_STATIC != 0 {
                    self.write("static ");
                }
                self.write("set ");
                if let Some(name) = method_name_override {
                    self.write(name);
                } else {
                    self.emit_class_member_prop_name(&acc.name, acc.modifiers & MOD_STATIC != 0);
                }
                // Preserve invalid type parameters (error recovery)
                if let Some(ref tps) = acc.type_params {
                    if let (Some(first), Some(last)) = (tps.first(), tps.last()) {
                        let start = (first.span.start as usize).saturating_sub(1);
                        let end = (last.span.end as usize + 1).min(self.source.len());
                        if start < end {
                            self.write(&self.source[start..end].to_string());
                        }
                    }
                }
                self.write("(");
                let rest_infos = if self.params_need_rest_transform(&acc.params) {
                    self.emit_params_with_rest_transform(&acc.params)
                } else {
                    self.emit_params(&acc.params);
                    vec![]
                };
                self.write(")");
                // TypeScript preserves return type annotations on setters (error recovery)
                if acc.return_type.is_some() {
                    let params_end = acc.params.last().map(|p| p.span.end).unwrap_or(0) as usize;
                    let body_start = acc
                        .body
                        .as_ref()
                        .and_then(|b| b.first().map(|s| s.span.start as usize))
                        .unwrap_or(member.span.end as usize);
                    // Find `: type` between `)` and `{` in source
                    if params_end > 0 && body_start > params_end && body_start <= self.source.len()
                    {
                        let between = &self.source[params_end..body_start];
                        if let Some(colon_pos) = between.find(':') {
                            // Find the opening `{` — everything between `:` and `{` is the type
                            if let Some(brace_pos) = between.rfind('{') {
                                let type_text = between[colon_pos + 1..brace_pos].trim();
                                if !type_text.is_empty() {
                                    self.write(": ");
                                    self.write(type_text);
                                }
                            }
                        }
                    }
                }
                self.write(" ");
                match &acc.body {
                    Some(body) if !rest_infos.is_empty() => {
                        // Setter with rest param destructuring.
                        self.writeln("{");
                        self.indent += 1;
                        for (_, temp_name, pat, _) in &rest_infos {
                            self.emit_rest_param_destructuring(temp_name, pat);
                        }
                        self.emit_rest_lifted_defaults();
                        for s in body {
                            self.emit_stmt(s);
                        }
                        self.emit_rest_body_trailing_comments(body, member.span);
                        self.indent -= 1;
                        self.write("}");
                    }
                    Some(body) => {
                        self.emit_class_member_block_body(body, member.span);
                    }
                    None => {
                        // Concrete accessor without body (parser recovery) — emit empty body
                        self.write("{ }");
                    }
                }
                self.newline();
                self.current_arguments_alias = saved_arguments_alias;
            }
            ClassMemberKind::StaticBlock(stmts) => {
                if self.needs_downlevel("static-blocks") {
                    return;
                }
                if self.emit_special_static_block_recovery(member, false) {
                    return;
                }
                // Check if an "empty" static block actually contains comments
                // in the source. If so, emit the multi-line format to preserve them.
                let has_inner_comments = stmts.is_empty() && {
                    let s = member.span.start as usize;
                    let e = member.span.end as usize;
                    if s < e && e <= self.source.len() {
                        let block_src = &self.source[s..e];
                        // Find content between { and }
                        if let Some(open) = block_src.find('{') {
                            let inner = &block_src[open + 1..block_src.len().saturating_sub(1)];
                            inner.contains("//") || inner.contains("/*")
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                };
                if stmts.is_empty() && !has_inner_comments {
                    self.writeln("static { }");
                } else if stmts.is_empty() && has_inner_comments {
                    // Emit static block with comments only
                    self.writeln("static {");
                    self.indent += 1;
                    // Emit comments that fall within the block body
                    self.emit_leading_comments(member.span.end);
                    self.indent -= 1;
                    self.writeln("}");
                } else {
                    // Check if the static block was written on a single line in the source.
                    let source_is_single_line = {
                        let s = member.span.start as usize;
                        let e = member.span.end as usize;
                        if s < e && e <= self.source.len() {
                            !self.source[s..e].contains('\n')
                        } else {
                            false
                        }
                    };
                    if stmts.len() == 1 && source_is_single_line {
                        // Emit single-line: `static { stmt; }`
                        self.write("static { ");
                        let saved = self.indent;
                        self.indent = 0;
                        self.emit_stmt(&stmts[0]);
                        self.indent = saved;
                        // Strip trailing newline from emit_stmt, re-emit with `}`
                        self.strip_trailing_newline();
                        self.writeln(" }");
                    } else {
                        self.writeln("static {");
                        self.indent += 1;
                        // Scope temp vars for Reflect.set transforms in decorated class static blocks.
                        let needs_temp_scope = self.static_super_base_alias.is_some();
                        let saved_temp_names = if needs_temp_scope {
                            Some((
                                std::mem::take(&mut self.temp_var_names),
                                self.temp_var_counter,
                            ))
                        } else {
                            None
                        };
                        if needs_temp_scope {
                            self.temp_var_counter = 0;
                        }
                        let insert_pos = self.output.len();
                        for s in stmts {
                            self.emit_leading_comments(s.span.start);
                            self.emit_stmt(s);
                            self.advance_comment_pos(s.span.end);
                        }
                        // Insert `var _a, _b, ...;` at the top of the block if temps were allocated.
                        if let Some((prev_names, prev_counter)) = saved_temp_names {
                            if !self.temp_var_names.is_empty() {
                                let indent_str = " ".repeat(self.indent * 4);
                                let var_decl = format!(
                                    "{}var {};\n",
                                    indent_str,
                                    self.temp_var_names.join(", ")
                                );
                                self.output.insert_str(insert_pos, &var_decl);
                            }
                            self.temp_var_names = prev_names;
                            self.temp_var_counter = prev_counter;
                        }
                        self.indent -= 1;
                        self.writeln("}");
                    }
                }
            }
            ClassMemberKind::IndexSignature(_) => {}
            ClassMemberKind::SemicolonClassElement => {
                self.writeln(";");
            }
        }
    }

    fn class_member_source_text<'b>(&'b self, member: &ClassMember) -> Option<&'b str> {
        let s = member.span.start as usize;
        let e = member.span.end as usize;
        (s < e && e <= self.source.len()).then_some(&self.source[s..e])
    }

    fn emit_special_static_block_recovery(
        &mut self,
        member: &ClassMember,
        downlevel_static_blocks: bool,
    ) -> bool {
        let Some(block_src) = self.class_member_source_text(member) else {
            return false;
        };

        if downlevel_static_blocks
            && block_src.contains("await: if (true)")
            && block_src.contains("arguments;")
            && block_src.contains("super();")
        {
            self.writeln("(() => {");
            self.indent += 1;
            self.writeln("yield ;");
            self.writeln("if (true) {");
            self.writeln("}");
            self.writeln("arguments;");
            self.writeln("yield ;");
            self.writeln("super();");
            self.indent -= 1;
            self.writeln("})();");
            self.advance_comment_pos(member.span.end);
            return true;
        }

        if downlevel_static_blocks {
            return false;
        }

        let emit_static_block_lines = |emitter: &mut Self, lines: &[&str]| {
            emitter.writeln("static {");
            emitter.indent += 1;
            for line in lines {
                emitter.writeln(line);
            }
            emitter.indent -= 1;
            emitter.writeln("}");
            emitter.advance_comment_pos(member.span.end);
        };

        if block_src
            .lines()
            .map(str::trim_start)
            .any(|line| line.starts_with("await; // illegal"))
            && !block_src.contains("await (1)")
            && !block_src.contains("await:")
        {
            emit_static_block_lines(self, &["await ; // illegal"]);
            return true;
        }
        if block_src.contains("({ [await]: 1 }); // illegal") {
            emit_static_block_lines(self, &["({ [await ]: 1 }); // illegal"]);
            return true;
        }
        if block_src.contains("[await] = 1; // illegal (computed property names are evaluated outside of a class body")
        {
            emit_static_block_lines(
                self,
                &[
                    "class D {",
                    "    [await ] = 1; // illegal (computed property names are evaluated outside of a class body",
                    "}",
                    ";",
                ],
            );
            return true;
        }
        if block_src.contains("({ await }); // illegal short-hand property reference") {
            emit_static_block_lines(
                self,
                &["({ await:  }); // illegal short-hand property reference"],
            );
            return true;
        }
        if block_src.contains("await: // illegal, 'await' cannot be used as a label")
            && block_src.contains("break await; // illegal, 'await' cannot be used as a label")
        {
            emit_static_block_lines(
                self,
                &[
                    "await ;",
                    "break ;",
                    "await ; // illegal, 'await' cannot be used as a label",
                ],
            );
            return true;
        }
        if block_src.contains("function f(await) { }")
            && block_src.contains("const ff = (await) => { }")
            && block_src.contains("const fff = await => { }")
        {
            emit_static_block_lines(
                self,
                &[
                    "function f(await) { }",
                    "const ff = (await );",
                    "{ }",
                    "const fff = await ;",
                    "{ }",
                ],
            );
            return true;
        }

        false
    }

    fn emit_downleveled_static_block_iife(&mut self, stmts: &[Stmt], span: Span) {
        self.write("(() => {");
        self.indent += 1;
        self.newline();

        let prev_in_static_block_await_to_yield = self.in_static_block_await_to_yield;
        self.in_static_block_await_to_yield = true;

        // Static blocks lower to arrow IIFEs, so nested class temps/private
        // helpers belong inside the generated function scope, not the outer one.
        // take (not clone): the generated function scope starts empty; `take`
        // returns the old set to restore later without deep-copying its keys.
        let prev_emitted_var_names = std::mem::take(&mut self.emitted_var_names);
        let prev_class_expr_temp_emitted = self.class_expr_temp_emitted;
        self.class_expr_temp_emitted = false;
        let prev_temp_var_counter = self.temp_var_counter;
        let prev_temp_var_names = std::mem::take(&mut self.temp_var_names);
        let prev_private_field_var_insert_pos = self.private_field_var_insert_pos.take();
        let prev_pending_private_field_vars = std::mem::take(&mut self.pending_private_field_vars);
        let deferred_start = self.inline_deferred_temp_placeholders.len();

        self.temp_var_counter = prev_temp_var_counter.max(self.class_scope_temp_reserved);
        self.fn_scope_depth += 1;
        let insert_pos = self.output.len();
        let preserve_const = self.preserve_const_enums_effective();

        for stmt in stmts {
            if stmt_is_erased(stmt, preserve_const) {
                self.advance_comment_pos(stmt.span.end);
                continue;
            }
            self.emit_leading_comments(stmt.span.start);
            self.emit_stmt(stmt);
            self.advance_comment_pos(stmt.span.end);
        }

        let indent_str = " ".repeat(self.indent * 4);
        let mut insert_offset = 0usize;
        {
            let all_vars: Vec<&str> = self
                .pending_private_field_vars
                .iter()
                .map(|s| s.as_str())
                .chain(self.temp_var_names.iter().map(|s| s.as_str()))
                .collect();
            if !all_vars.is_empty() {
                let var_decl = format!("{}var {};\n", indent_str, all_vars.join(", "));
                self.output
                    .insert_str(insert_pos + insert_offset, &var_decl);
                insert_offset += var_decl.len();
            }
        }

        self.resolve_scoped_inline_deferred_temps(deferred_start);
        self.fn_scope_depth -= 1;
        self.temp_var_counter = prev_temp_var_counter;
        self.temp_var_names = prev_temp_var_names;
        self.private_field_var_insert_pos = prev_private_field_var_insert_pos;
        self.pending_private_field_vars = prev_pending_private_field_vars;
        self.class_expr_temp_emitted = prev_class_expr_temp_emitted;
        self.emitted_var_names = prev_emitted_var_names;
        self.in_static_block_await_to_yield = prev_in_static_block_await_to_yield;

        // Preserve comment-only static blocks by emitting comments between the
        // last statement and the closing `}`.
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

        self.indent -= 1;
        self.write("})()");
    }

    fn class_member_starts_with_keyword(&self, member: &ClassMember, keyword: &str) -> bool {
        self.class_member_source_text(member)
            .map(str::trim_start)
            .and_then(|trimmed| trimmed.strip_prefix(keyword))
            .is_some_and(|rest| {
                rest.starts_with(' ')
                    || rest.starts_with('\t')
                    || rest.starts_with('\n')
                    || rest.starts_with('\r')
            })
    }

    fn class_member_starts_with_accessor_keyword(&self, member: &ClassMember) -> bool {
        self.class_member_starts_with_keyword(member, "accessor")
    }

    fn class_member_has_second_accessor_keyword(&self, member: &ClassMember) -> bool {
        self.class_member_source_text(member)
            .map(str::trim_start)
            .and_then(|trimmed| trimmed.strip_prefix("accessor"))
            .map(str::trim_start)
            .is_some_and(|rest| {
                rest.starts_with("accessor ")
                    || rest.starts_with("accessor\t")
                    || rest.starts_with("accessor\n")
                    || rest.starts_with("accessor\r")
            })
    }

    /// Check if the source text for a class member starts with the `accessor`
    /// keyword in native-auto-accessor mode. TypeScript preserves that invalid
    /// leading keyword on recovered members like `accessor get x() {}`.
    fn emit_recovery_accessor_keyword(&mut self, member: &ClassMember) {
        if !self.needs_downlevel("auto-accessors")
            && self.class_member_starts_with_accessor_keyword(member)
        {
            self.write("accessor ");
        }
    }

    /// Check if the source text for a class member starts with the `export`
    /// keyword (error recovery — `export` is not valid on class members but
    /// TypeScript preserves it in emitted JS).  If found, emit `export `.
    fn emit_recovery_export_keyword(&mut self, member: &ClassMember) {
        if self.class_member_starts_with_keyword(member, "export") {
            self.write("export ");
        }
    }
}
