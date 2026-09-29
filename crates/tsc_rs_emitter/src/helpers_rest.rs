//! Rest destructuring transform and object-rest pattern detection.

use super::*;

/// Extract a `// ...` line comment from a source text range, if present.
/// Returns the comment text (including the `//`), trimmed of trailing whitespace.
pub(crate) fn find_line_comment_in_range(source: &str, start: usize, end: usize) -> Option<&str> {
    let end = end.min(source.len());
    if start >= end {
        return None;
    }
    let slice = &source[start..end];
    let pos = slice.find("//")?;
    // Make sure it's not inside a string or something — simple heuristic:
    // only match if the `//` is preceded by whitespace/comma/brace
    if pos > 0 {
        let prev = slice.as_bytes()[pos - 1];
        if prev != b' ' && prev != b'\t' && prev != b',' && prev != b'{' && prev != b'\n' {
            return None;
        }
    }
    let comment_start = start + pos;
    let comment_end = source[comment_start..]
        .find('\n')
        .map(|p| comment_start + p)
        .unwrap_or(end);
    Some(source[comment_start..comment_end].trim_end())
}

/// Get the span end of an ObjPatProp.
fn obj_pat_prop_end(prop: &ObjPatProp) -> u32 {
    match prop {
        ObjPatProp::KeyValue(_, val) => val.span.end,
        ObjPatProp::Shorthand(_, span) => span.end,
        ObjPatProp::ShorthandAssign(_, _, span) => span.end,
        ObjPatProp::Rest(p) => p.span.end,
    }
}

/// Get the span start of an ObjPatProp.
fn obj_pat_prop_start(prop: &ObjPatProp) -> u32 {
    match prop {
        ObjPatProp::KeyValue(key, _) => key.span().start,
        ObjPatProp::Shorthand(_, span) => span.start,
        ObjPatProp::ShorthandAssign(_, _, span) => span.start,
        ObjPatProp::Rest(p) => p.span.start,
    }
}

#[derive(Clone)]
pub(super) struct NestedPatObjRestTransform {
    pub temp: String,
    pub props: Vec<ObjPatProp>,
}

/// Controls how temp variables are allocated during nested rest pattern rewriting.
#[derive(Clone, Copy)]
pub(super) enum RestTempAlloc {
    /// Allocate hoisted temp vars (added to file-level `var` declaration).
    Hoisted,
    /// Allocate inline temp vars immediately from the counter.
    Inline,
    /// Allocate inline deferred temp placeholders (resolved at end of file).
    InlineDeferred,
}

#[derive(Clone)]
pub(super) enum NestedExprObjRestTransform {
    Object {
        temp: String,
        props: Vec<ObjLitProp>,
    },
    Defaulted {
        source_temp: String,
        target: Expr,
        default_expr: Expr,
    },
    Array {
        source_temp: String,
        pattern: Expr,
        transforms: Vec<NestedExprObjRestTransform>,
    },
}

#[derive(Clone)]
enum RestExcludedKey {
    /// Static key with its quote character (`"\"" or `"'"`).
    Static(String, &'static str),
    DynamicTemp(String),
}

#[derive(Copy, Clone)]
enum RestTransformTempKind {
    Inline,
    Hoisted,
}

impl<'a> Emitter<'a> {
    /// Pre-scan statements to check if any object rest destructuring exists (for __rest helper).
    pub(crate) fn scan_needs_rest(&mut self, stmts: &[Stmt]) {
        if !self.needs_downlevel("object-spread") {
            return;
        }
        let preserve_const_enums = self.options.preserve_const_enums == Some(true);
        for stmt in stmts {
            // Skip erased (ambient/declare) statements -- they don't produce runtime code.
            let recoverable_fn_decl = matches!(
                &stmt.kind,
                StmtKind::FnDecl(fn_decl) if self.fn_decl_requires_recovery_emit(fn_decl)
            );
            if stmt_is_erased(stmt, preserve_const_enums) && !recoverable_fn_decl {
                continue;
            }
            if source_has_object_rest(stmt) {
                self.needs_rest_helper = true;
                return;
            }
        }
    }

    /// Check if any parameter has a top-level object rest pattern that needs
    /// downlevel transform (target < ES2018).
    pub(crate) fn params_need_rest_transform(&self, params: &[Param]) -> bool {
        if !self.needs_downlevel("object-spread") {
            return false;
        }
        params.iter().any(|p| param_has_top_level_object_rest(p))
    }

    /// Emit parameters, replacing any that have object rest patterns with
    /// generated temp names (`_a`, `_b`, ...). Returns a list of
    /// `(param_index, temp_name, original_pat)` for each replaced param,
    /// so the caller can emit the destructuring prefix in the body.
    ///
    /// When a rest-transformed param is encountered, subsequent params with
    /// defaults have their defaults suppressed (emitted only as names) since
    /// TypeScript lifts those defaults into the body after the rest
    /// destructuring. Call `emit_rest_lifted_defaults` after emitting the
    /// rest destructuring to emit the lifted `if (x === void 0) { x = ...; }`.
    pub(crate) fn emit_params_with_rest_transform<'p>(
        &mut self,
        params: &'p [Param],
    ) -> Vec<(usize, String, &'p Pat, Option<&'p Expr>)> {
        let mut rest_infos = Vec::new();
        let mut temp_counter: u8 = b'a';
        let mut first = true;
        let mut seen_rest_transform = false;
        for (i, param) in params.iter().enumerate() {
            // Skip 'this' parameter
            if let PatKind::Ident(ref name) = param.name.kind {
                if name == "this" || name == "<error>" {
                    continue;
                }
            }
            if !first {
                self.write(", ");
            }
            first = false;

            if param_has_top_level_object_rest(param) {
                seen_rest_transform = true;
                // Generate temp name
                let temp_name = format!("_{}", temp_counter as char);
                temp_counter += 1;
                if param.dotdotdot {
                    self.write("...");
                }
                self.write(&temp_name);
                // Preserve default value on the temp parameter
                if let Some(ref init) = param.initializer {
                    self.write(" = ");
                    let prev_in_param_init = self.in_parameter_initializer;
                    self.in_parameter_initializer = true;
                    self.emit_expr(init);
                    self.in_parameter_initializer = prev_in_param_init;
                }
                rest_infos.push((i, temp_name, &param.name, param.initializer.as_deref()));
            } else {
                // Emit normally
                self.emit_inline_comments_before(param.span.start);
                if param.dotdotdot {
                    self.write("...");
                }
                self.emit_binding_name(&param.name);
                self.emit_inline_trailing_comment(param);
                // When a rest-transformed param precedes this one, lift
                // defaults into the body so destructured bindings are
                // available (TypeScript's behavior).
                if let Some(ref init) = param.initializer {
                    if seen_rest_transform {
                        // Default will be lifted — skip inline emit.
                        // Record for body emission.
                        if let PatKind::Ident(ref name) = param.name.kind {
                            self.rest_lifted_defaults
                                .push((name.to_string(), init.span));
                        }
                    } else {
                        self.write(" = ");
                        let prev_in_param_init = self.in_parameter_initializer;
                        self.in_parameter_initializer = true;
                        self.emit_expr(init);
                        self.in_parameter_initializer = prev_in_param_init;
                    }
                }
            }
        }
        // Record how many param temps were used so that body temps
        // start after them (e.g. param `_a` → body starts at `_b`).
        self.fn_param_rest_temp_count = rest_infos.len();
        rest_infos
    }

    /// Emit lifted parameter defaults after object rest destructuring.
    /// Called in the body after `emit_rest_param_destructuring` to emit
    /// `if (name === void 0) { name = <default>; }` for each lifted default.
    pub(crate) fn emit_rest_lifted_defaults(&mut self) {
        let lifted = std::mem::take(&mut self.rest_lifted_defaults);
        for (name, init_span) in &lifted {
            self.write("if (");
            self.write(name);
            self.write(" === void 0) { ");
            self.write(name);
            self.write(" = ");
            self.copy_expr_span(*init_span);
            self.strip_trailing_newline();
            self.write("; }");
            self.newline();
        }
    }

    /// Emit comments from the original body block that come after all user
    /// statements.  This is needed in the rest-param body path where the body
    /// is emitted manually (not through `_emit_block_for_decl_body`), so
    /// comments in comment-only or trailing positions would otherwise be lost.
    pub(crate) fn emit_rest_body_trailing_comments(
        &mut self,
        body_stmts: &[Stmt],
        enclosing_span: Span,
    ) {
        let remove_comments = self.options.remove_comments == Some(true) && !self.preserve_comments;
        if remove_comments {
            return;
        }
        let start = enclosing_span.start as usize;
        let end = enclosing_span.end as usize;
        if start >= end || end > self.source.len() {
            return;
        }
        let fn_text = &self.source[start..end];
        // Find the body block opening `{`
        let brace_pos = match fn_text.rfind('{') {
            Some(pos) => pos,
            None => return,
        };
        let after_brace = &fn_text[brace_pos + 1..];
        let close_pos = match after_brace.rfind('}') {
            Some(pos) => pos,
            None => return,
        };
        let body_content = &after_brace[..close_pos];
        // Determine the source range to scan for comments: after all user
        // statements (or the entire body if no statements).
        let scan_from = if let Some(last) = body_stmts.last() {
            let last_end = last.span.end as usize;
            if last_end > start + brace_pos + 1 {
                last_end - (start + brace_pos + 1)
            } else {
                0
            }
        } else {
            // Empty body — scan everything after the first newline (skip
            // same-line content as `{`)
            match body_content.find('\n') {
                Some(nl) => nl + 1,
                None => return,
            }
        };
        if scan_from >= body_content.len() {
            return;
        }
        let scan_region = &body_content[scan_from..];
        let mut in_block_comment = false;
        for line in scan_region.lines() {
            let trimmed = line.trim();
            if in_block_comment {
                self.writeln(trimmed);
                if trimmed.contains("*/") {
                    in_block_comment = false;
                }
            } else if trimmed.starts_with("//") {
                self.writeln(trimmed);
            } else if trimmed.starts_with("/*") && !trimmed.starts_with("/*!") {
                self.writeln(trimmed);
                if !trimmed.contains("*/") {
                    in_block_comment = true;
                }
            }
        }
    }

    /// Emit the destructuring `var` statement that unpacks an object rest
    /// parameter at the start of the function body.
    ///
    /// Given `({ a, x: b, bar = {}, ...rest })` with temp name `_a`, emits:
    ///   `var { a, x: b, bar = {} } = _a, rest = __rest(_a, ["a", "x", "bar"]);`
    pub(crate) fn emit_rest_param_destructuring(&mut self, temp_name: &str, pat: &Pat) {
        // Ensure body temps don't conflict with param temps (e.g. param `_a` →
        // body temps start at `_b`). The param counter is tracked by
        // `emit_params_with_rest_transform`.
        if self.fn_param_rest_temp_count > 0 {
            self.temp_var_counter = self.temp_var_counter.max(self.fn_param_rest_temp_count);
        }

        if let Some(props) = top_level_object_props(pat) {
            // Check if any non-rest property needs complex lowering (nested rest,
            // computed keys, etc.). If so, delegate to the recursive complex path.
            let needs_complex = props.iter().any(|p| {
                !matches!(p, ObjPatProp::Rest(_))
                    && !self.obj_pat_prop_can_use_simple_rest_destructure(p)
            });

            if needs_complex {
                self.write("var ");
                let mut first = true;
                self.emit_object_pat_rest_bindings_from_source(
                    props,
                    temp_name,
                    RestTransformTempKind::Inline,
                    &mut first,
                );
                self.writeln(";");
                return;
            }

            // Simple path: all non-rest props are simple bindings.
            let mut non_rest: Vec<&ObjPatProp> = Vec::new();
            let mut rest_binding: Option<&Pat> = None;
            let mut excluded_keys: Vec<String> = Vec::new();

            for (idx, prop) in props.iter().enumerate() {
                let is_last = idx == props.len() - 1;
                match prop {
                    ObjPatProp::Rest(p) => {
                        if is_last {
                            rest_binding = Some(p);
                        } else {
                            // TypeScript error recovery: non-last rest is stripped
                            if let PatKind::Ident(name) = &p.kind {
                                excluded_keys.push(name.to_string());
                            }
                        }
                    }
                    ObjPatProp::KeyValue(key, _) => {
                        if let Some(key_str) = prop_name_to_string(key) {
                            excluded_keys.push(key_str);
                        }
                        non_rest.push(prop);
                    }
                    ObjPatProp::Shorthand(name, _) => {
                        excluded_keys.push(name.to_string());
                        non_rest.push(prop);
                    }
                    ObjPatProp::ShorthandAssign(name, _, _) => {
                        excluded_keys.push(name.to_string());
                        non_rest.push(prop);
                    }
                }
            }

            self.write("var ");

            if !non_rest.is_empty() {
                self.write("{ ");
                for (i, prop) in non_rest.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    match prop {
                        ObjPatProp::KeyValue(key, val) => {
                            self.emit_prop_name(key);
                            self.write(": ");
                            self.emit_binding_name(val);
                        }
                        ObjPatProp::Shorthand(name, _) => self.write(name),
                        ObjPatProp::ShorthandAssign(name, init, _) => {
                            self.write(name);
                            self.write(" = ");
                            self.emit_expr(init);
                        }
                        ObjPatProp::Rest(_) => unreachable!(),
                    }
                }
                self.write(" } = ");
                self.write(temp_name);
            }

            if let Some(rest_pat) = rest_binding {
                if !non_rest.is_empty() {
                    self.write(", ");
                }
                self.emit_binding_name(rest_pat);
                self.write(" = ");
                self.write(self.helper_prefix());
                self.write("__rest(");
                self.write(temp_name);
                self.write(", [");
                for (i, key) in excluded_keys.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.write("\"");
                    self.write(key);
                    self.write("\"");
                }
                self.write("])");
            }

            self.writeln(";");
        }
    }

    /// Emit a var declarator that has an object rest pattern (top-level or nested).
    ///
    /// Given `let { a, x: b, ...rest } = obj`, emits:
    ///   `{ a, x: b } = obj, rest = __rest(obj, ["a", "x"])`
    ///
    /// Given `let { f: { a, ...rest } } = obj` (nested rest), emits:
    ///   `_a = obj.f, { a } = _a, rest = __rest(_a, ["a"])`
    ///
    /// (without leading keyword or trailing semicolon; called from emit_var_stmt)
    fn next_rest_transform_temp(&mut self, kind: RestTransformTempKind) -> String {
        match kind {
            RestTransformTempKind::Inline => self.next_inline_deferred_temp_placeholder(),
            RestTransformTempKind::Hoisted => {
                // Use pre-allocated assignment rest temps if available (these
                // get lower-numbered names to match TypeScript's allocation order).
                if !self.pre_allocated_assignment_rest_temps.is_empty() {
                    let name = self.pre_allocated_assignment_rest_temps.remove(0);
                    self.temp_var_names.push(name.clone().into());
                    name
                } else {
                    self.next_temp_var()
                }
            }
        }
    }

    fn emit_rest_transform_sep(&mut self, first: &mut bool) {
        if !*first {
            self.write(", ");
        }
        *first = false;
    }

    fn emit_rest_excluded_keys(&mut self, excluded_keys: &[RestExcludedKey]) {
        self.write("[");
        for (i, key) in excluded_keys.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            match key {
                RestExcludedKey::Static(key, quote) => {
                    self.write(quote);
                    self.write(key);
                    self.write(quote);
                }
                RestExcludedKey::DynamicTemp(temp) => {
                    self.write("typeof ");
                    self.write(temp);
                    self.write(" === \"symbol\" ? ");
                    self.write(temp);
                    self.write(" : ");
                    self.write(temp);
                    self.write(" + \"\"");
                }
            }
        }
        self.write("]");
    }

    fn obj_pat_prop_excluded_key(&self, prop: &ObjPatProp) -> Option<RestExcludedKey> {
        match prop {
            ObjPatProp::KeyValue(key, _) => self.prop_name_to_excluded_key(key),
            ObjPatProp::Shorthand(name, _) | ObjPatProp::ShorthandAssign(name, _, _) => {
                Some(RestExcludedKey::Static(name.to_string(), "\""))
            }
            ObjPatProp::Rest(_) => None,
        }
    }

    fn prop_name_to_excluded_key(&self, name: &PropName) -> Option<RestExcludedKey> {
        match name {
            PropName::Ident(s, _) | PropName::Number(s, _) => {
                Some(RestExcludedKey::Static(s.to_string(), "\""))
            }
            PropName::String(s, span) => Some(RestExcludedKey::Static(
                s.to_string(),
                self.original_quote_char(span),
            )),
            PropName::Computed(expr, _) => match &expr.kind {
                ExprKind::StrLit(s) => Some(RestExcludedKey::Static(
                    s.to_string(),
                    self.original_quote_char(&expr.span),
                )),
                ExprKind::NumLit(s) => Some(RestExcludedKey::Static(s.to_string(), "\"")),
                _ => None,
            },
            PropName::Private(_, _) => None,
        }
    }

    fn pat_can_use_simple_rest_destructure(&self, pat: &Pat) -> bool {
        match &pat.kind {
            PatKind::Ident(_) => true,
            PatKind::Assign(inner, init) => {
                matches!(&inner.kind, PatKind::Ident(_))
                    && !matches!(&init.kind, ExprKind::Assign(_))
            }
            // Nested object/array patterns without rest at any depth can be
            // preserved in destructuring. TypeScript keeps `{ b: { '0': n } } = o` as-is.
            PatKind::Object(_) | PatKind::Array(_) => !pat_has_object_rest(pat),
            _ => false,
        }
    }

    fn obj_pat_prop_can_use_simple_rest_destructure(&self, prop: &ObjPatProp) -> bool {
        match prop {
            ObjPatProp::KeyValue(key, value) => {
                !matches!(key, PropName::Computed(_, _) | PropName::Private(_, _))
                    && self.pat_can_use_simple_rest_destructure(value)
            }
            ObjPatProp::Shorthand(_, _) => true,
            ObjPatProp::ShorthandAssign(_, init, _) => !matches!(&init.kind, ExprKind::Assign(_)),
            ObjPatProp::Rest(_) => false,
        }
    }

    /// Like `obj_pat_prop_can_use_simple_rest_destructure` but also allows
    /// computed keys that are string/numeric literals (e.g. `['a']`, `[42]`).
    /// Used for internal bucketing in the complex rest path.
    fn obj_pat_prop_can_use_complex_simple_bucket(&self, prop: &ObjPatProp) -> bool {
        if self.obj_pat_prop_can_use_simple_rest_destructure(prop) {
            return true;
        }
        match prop {
            ObjPatProp::KeyValue(PropName::Computed(expr, _), value) => {
                matches!(&expr.kind, ExprKind::StrLit(_) | ExprKind::NumLit(_))
                    && self.pat_can_use_simple_rest_destructure(value)
            }
            _ => false,
        }
    }

    fn obj_pat_props_need_complex_rest_transform(&self, props: &[ObjPatProp]) -> bool {
        props.iter().any(|prop| {
            !matches!(prop, ObjPatProp::Rest(_))
                && !self.obj_pat_prop_can_use_simple_rest_destructure(prop)
        })
    }

    fn emit_source_ref_member_access(&mut self, source_ref: &str, key: &PropName) {
        self.write(source_ref);
        self.emit_member_access(key);
    }

    fn emit_source_ref_computed_access(&mut self, source_ref: &str, key_temp: &str) {
        self.write(source_ref);
        self.write("[");
        self.write(key_temp);
        self.write("]");
    }

    fn expr_can_inline_member_access_for_nested_rest(&self, expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Ident(_)
            | ExprKind::This
            | ExprKind::Super
            | ExprKind::Member(_)
            | ExprKind::ElemAccess(_)
            | ExprKind::Call(_)
            | ExprKind::New(_)
            | ExprKind::ObjectLit(_) => true,
            ExprKind::Paren(inner) | ExprKind::NonNull(inner) => {
                self.expr_can_inline_member_access_for_nested_rest(inner)
            }
            ExprKind::As(a) => self.expr_can_inline_member_access_for_nested_rest(&a.expr),
            ExprKind::Satisfies(s) => self.expr_can_inline_member_access_for_nested_rest(&s.expr),
            ExprKind::TypeAssertion(ta) => {
                self.expr_can_inline_member_access_for_nested_rest(&ta.expr)
            }
            _ => false,
        }
    }

    pub(crate) fn emit_single_nested_obj_rest_assign_downlevel(
        &mut self,
        left: &Expr,
        rhs: &Expr,
    ) -> bool {
        let Some(outer_props) = self.as_object_destructure_pattern(left) else {
            return false;
        };
        let [ObjLitProp::Property(outer_prop)] = outer_props else {
            return false;
        };
        let Some(inner_props) = self.as_object_destructure_pattern(&outer_prop.value) else {
            return false;
        };
        let [ObjLitProp::Spread(inner_target, _)] = inner_props else {
            return false;
        };
        if self
            .direct_private_destructure_member(inner_target)
            .is_some()
        {
            return false;
        }
        if !self.expr_can_inline_member_access_for_nested_rest(rhs) {
            return false;
        }

        if self.needs_downlevel("optional-chaining")
            && super::emit_expr::is_oc_chain(inner_target.as_ref())
        {
            self.suppress_oc_parens = true;
        }
        self.emit_expr(inner_target.as_ref());
        self.write(" = ");
        self.write(self.helper_prefix());
        self.write("__rest(");
        self.emit_expr(rhs);
        self.emit_member_access(&outer_prop.key);
        self.write(", [])");
        true
    }

    fn emit_obj_pat_simple_rest_bucket(
        &mut self,
        props: &[&ObjPatProp],
        source_ref: &str,
        first: &mut bool,
    ) {
        if props.is_empty() {
            return;
        }
        self.emit_rest_transform_sep(first);
        self.write("{ ");
        for (i, prop) in props.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            match prop {
                ObjPatProp::KeyValue(key, val) => {
                    self.emit_prop_name(key);
                    self.write(": ");
                    self.emit_binding_name(val);
                }
                ObjPatProp::Shorthand(name, _) => self.write(name),
                ObjPatProp::ShorthandAssign(name, init, _) => {
                    self.write(name);
                    self.write(" = ");
                    if !self.emit_compact_rest_bucket_init(init) {
                        self.emit_expr(init);
                    }
                }
                ObjPatProp::Rest(_) => unreachable!(),
            }
        }
        self.write(" } = ");
        self.write(source_ref);
    }

    fn emit_compact_rest_bucket_init(&mut self, init: &Expr) -> bool {
        let ExprKind::FnExpr(fn_decl) = &init.kind else {
            return false;
        };
        if !fn_decl.is_async
            || !fn_decl.is_generator
            || !self.needs_downlevel("async-generator")
            || fn_decl.name.is_some()
            || !fn_decl.params.is_empty()
            || fn_decl.return_type.is_some()
        {
            return false;
        }
        let Some(body) = fn_decl.body.as_ref() else {
            return false;
        };
        if !body.is_empty() {
            return false;
        }

        self.write("function () { return ");
        self.write(self.helper_prefix());
        self.write("__asyncGenerator(this, arguments, function* () { }); }");
        true
    }

    fn emit_pat_binding_from_source(
        &mut self,
        pat: &Pat,
        source_ref: &str,
        temp_kind: RestTransformTempKind,
        first: &mut bool,
    ) {
        match &pat.kind {
            PatKind::Ident(name) => {
                self.emit_rest_transform_sep(first);
                self.write(name);
                self.write(" = ");
                self.write(source_ref);
            }
            PatKind::Assign(inner, init) => {
                if let PatKind::Ident(name) = &inner.kind {
                    self.emit_rest_transform_sep(first);
                    self.write(name);
                    self.write(" = ");
                    self.write(source_ref);
                    self.write(" === void 0 ? ");
                    self.emit_expr(init);
                    self.write(" : ");
                    self.write(source_ref);
                } else {
                    let resolved = self.next_rest_transform_temp(temp_kind);
                    self.emit_rest_transform_sep(first);
                    self.write(&resolved);
                    self.write(" = ");
                    self.write(source_ref);
                    self.write(" === void 0 ? ");
                    self.emit_expr(init);
                    self.write(" : ");
                    self.write(source_ref);
                    self.emit_pat_binding_from_source(inner, &resolved, temp_kind, first);
                }
            }
            PatKind::Object(props) => {
                if props.iter().any(|prop| matches!(prop, ObjPatProp::Rest(_))) {
                    self.emit_object_pat_rest_bindings_from_source(
                        props, source_ref, temp_kind, first,
                    );
                } else {
                    self.emit_object_pat_no_rest_bindings_from_source(
                        props, source_ref, temp_kind, first,
                    );
                }
            }
            PatKind::Array(_) => {
                // When an array element contains object rest (e.g., [{ ...x }, ...y]),
                // TypeScript replaces the rest element with a temp and generates
                // a separate __rest call: [_d, ...y] = src, x = __rest(_d, [])
                if pat_has_object_rest(pat) {
                    if let Some((rewritten, transforms)) =
                        self.rewrite_nested_obj_rest_pat_with_temps(pat, RestTempAlloc::Inline)
                    {
                        self.emit_rest_transform_sep(first);
                        self.emit_binding_name(&rewritten);
                        self.write(" = ");
                        self.write(source_ref);
                        // emit_object_pat_rest_bindings_from_source handles its
                        // own separators, so don't add extra ones here.
                        for tr in transforms {
                            self.emit_object_pat_rest_bindings_from_source(
                                &tr.props, &tr.temp, temp_kind, first,
                            );
                        }
                    } else {
                        self.emit_rest_transform_sep(first);
                        self.emit_binding_name(pat);
                        self.write(" = ");
                        self.write(source_ref);
                    }
                } else {
                    self.emit_rest_transform_sep(first);
                    self.emit_binding_name(pat);
                    self.write(" = ");
                    self.write(source_ref);
                }
            }
            PatKind::Rest(_) => {
                self.emit_rest_transform_sep(first);
                self.emit_binding_name(pat);
                self.write(" = ");
                self.write(source_ref);
            }
        }
    }

    fn emit_pat_binding_from_member_source(
        &mut self,
        pat: &Pat,
        source_ref: &str,
        key: &PropName,
        temp_kind: RestTransformTempKind,
        first: &mut bool,
    ) {
        if matches!(&pat.kind, PatKind::Ident(_)) {
            self.emit_rest_transform_sep(first);
            self.emit_binding_name(pat);
            self.write(" = ");
            self.emit_source_ref_member_access(source_ref, key);
            return;
        }

        // When the target is a pure rest pattern `{ ...x }`, TypeScript passes
        // the member access inline to __rest without allocating a temp:
        //   `nr = __rest(source.key, [])` instead of `_h = source.key, nr = __rest(_h, [])`
        if let PatKind::Object(props) = &pat.kind {
            if props.len() == 1 && matches!(&props[0], ObjPatProp::Rest(_)) {
                let saved = self.output.len();
                self.emit_source_ref_member_access(source_ref, key);
                let composed_ref = self.output[saved..].to_string();
                self.output.truncate(saved);
                self.emit_pat_binding_from_source(pat, &composed_ref, temp_kind, first);
                return;
            }
        }

        // When the target is an array with nested object rest, TypeScript puts
        // the array destructuring directly on the member access without caching:
        //   `[_d, ...y] = source.key, x = __rest(_d, [])` instead of
        //   `_e = source.key, [_d, ...y] = _e, x = __rest(_d, [])`
        if matches!(&pat.kind, PatKind::Array(_)) && pat_has_object_rest(pat) {
            let saved = self.output.len();
            self.emit_source_ref_member_access(source_ref, key);
            let composed_ref = self.output[saved..].to_string();
            self.output.truncate(saved);
            self.emit_pat_binding_from_source(pat, &composed_ref, temp_kind, first);
            return;
        }

        let access_temp = self.next_rest_transform_temp(temp_kind);
        self.emit_rest_transform_sep(first);
        self.write(&access_temp);
        self.write(" = ");
        self.emit_source_ref_member_access(source_ref, key);
        self.emit_pat_binding_from_source(pat, &access_temp, temp_kind, first);
    }

    fn emit_pat_binding_from_computed_source(
        &mut self,
        pat: &Pat,
        source_ref: &str,
        key_temp: &str,
        temp_kind: RestTransformTempKind,
        first: &mut bool,
    ) {
        if matches!(&pat.kind, PatKind::Ident(_)) {
            self.emit_rest_transform_sep(first);
            self.emit_binding_name(pat);
            self.write(" = ");
            self.emit_source_ref_computed_access(source_ref, key_temp);
            return;
        }

        let access_temp = self.next_rest_transform_temp(temp_kind);
        self.emit_rest_transform_sep(first);
        self.write(&access_temp);
        self.write(" = ");
        self.emit_source_ref_computed_access(source_ref, key_temp);
        self.emit_pat_binding_from_source(pat, &access_temp, temp_kind, first);
    }

    fn emit_object_pat_no_rest_bindings_from_source(
        &mut self,
        props: &[ObjPatProp],
        source_ref: &str,
        temp_kind: RestTransformTempKind,
        first: &mut bool,
    ) {
        // Collect consecutive simple properties into a bucket and emit them
        // as a destructuring pattern `{ ... } = source_ref`, matching TypeScript.
        let mut simple_bucket: Vec<&ObjPatProp> = Vec::new();
        for prop in props {
            let is_simple = match prop {
                ObjPatProp::Shorthand(_, _) => true,
                ObjPatProp::KeyValue(key, value) => {
                    !matches!(key, PropName::Computed(_, _))
                        && self.pat_can_use_simple_rest_destructure(value)
                }
                _ => false,
            };
            if is_simple {
                simple_bucket.push(prop);
                continue;
            }
            // Flush bucket before complex property
            self.emit_obj_pat_simple_rest_bucket(&simple_bucket, source_ref, first);
            simple_bucket.clear();
            match prop {
                ObjPatProp::KeyValue(key, value) => match key {
                    PropName::Computed(expr, _) => {
                        let key_temp = self.next_rest_transform_temp(temp_kind);
                        self.emit_rest_transform_sep(first);
                        self.write(&key_temp);
                        self.write(" = ");
                        self.emit_expr(expr);
                        self.emit_pat_binding_from_computed_source(
                            value, source_ref, &key_temp, temp_kind, first,
                        );
                    }
                    _ => self.emit_pat_binding_from_member_source(
                        value, source_ref, key, temp_kind, first,
                    ),
                },
                ObjPatProp::ShorthandAssign(name, init, _) => {
                    let access_temp = self.next_rest_transform_temp(temp_kind);
                    self.emit_rest_transform_sep(first);
                    self.write(&access_temp);
                    self.write(" = ");
                    self.write(source_ref);
                    self.write(".");
                    self.write(name);
                    self.emit_rest_transform_sep(first);
                    self.write(name);
                    self.write(" = ");
                    self.write(&access_temp);
                    self.write(" === void 0 ? ");
                    self.emit_expr(init);
                    self.write(" : ");
                    self.write(&access_temp);
                }
                ObjPatProp::Rest(_) => unreachable!(),
                _ => {} // Shorthand handled by bucket above
            }
        }
        // Flush remaining simple properties
        self.emit_obj_pat_simple_rest_bucket(&simple_bucket, source_ref, first);
    }

    fn emit_object_pat_rest_bindings_from_source(
        &mut self,
        props: &[ObjPatProp],
        source_ref: &str,
        temp_kind: RestTransformTempKind,
        first: &mut bool,
    ) {
        let mut simple_bucket: Vec<&ObjPatProp> = Vec::new();
        let mut excluded_keys: Vec<RestExcludedKey> = Vec::new();
        let mut rest_binding: Option<&Pat> = None;

        for (idx, prop) in props.iter().enumerate() {
            let is_last = idx == props.len() - 1;
            match prop {
                ObjPatProp::Rest(rest) => {
                    if is_last {
                        rest_binding = Some(rest);
                    } else {
                        // TypeScript error recovery: non-last rest is stripped
                        if let PatKind::Ident(name) = &rest.kind {
                            excluded_keys.push(RestExcludedKey::Static(name.to_string(), "\""));
                        }
                    }
                }
                _ if self.obj_pat_prop_can_use_complex_simple_bucket(prop) => {
                    if let Some(key) = self.obj_pat_prop_excluded_key(prop) {
                        excluded_keys.push(key);
                    }
                    simple_bucket.push(prop);
                }
                ObjPatProp::KeyValue(key, value) => {
                    self.emit_obj_pat_simple_rest_bucket(&simple_bucket, source_ref, first);
                    simple_bucket.clear();
                    match key {
                        PropName::Computed(expr, _) => {
                            let key_temp = self.next_rest_transform_temp(temp_kind);
                            excluded_keys.push(RestExcludedKey::DynamicTemp(key_temp.clone()));
                            self.emit_rest_transform_sep(first);
                            self.write(&key_temp);
                            self.write(" = ");
                            self.emit_expr(expr);
                            self.emit_pat_binding_from_computed_source(
                                value, source_ref, &key_temp, temp_kind, first,
                            );
                        }
                        _ => {
                            if let Some(key) = self.prop_name_to_excluded_key(key) {
                                excluded_keys.push(key);
                            }
                            self.emit_pat_binding_from_member_source(
                                value, source_ref, key, temp_kind, first,
                            );
                        }
                    }
                }
                ObjPatProp::Shorthand(name, _) => {
                    self.emit_obj_pat_simple_rest_bucket(&simple_bucket, source_ref, first);
                    simple_bucket.clear();
                    excluded_keys.push(RestExcludedKey::Static(name.to_string(), "\""));
                    self.emit_rest_transform_sep(first);
                    self.write(name);
                    self.write(" = ");
                    self.write(source_ref);
                    self.write(".");
                    self.write(name);
                }
                ObjPatProp::ShorthandAssign(name, init, _) => {
                    self.emit_obj_pat_simple_rest_bucket(&simple_bucket, source_ref, first);
                    simple_bucket.clear();
                    excluded_keys.push(RestExcludedKey::Static(name.to_string(), "\""));
                    let access_temp = self.next_rest_transform_temp(temp_kind);
                    self.emit_rest_transform_sep(first);
                    self.write(&access_temp);
                    self.write(" = ");
                    self.write(source_ref);
                    self.write(".");
                    self.write(name);
                    self.emit_rest_transform_sep(first);
                    self.write(name);
                    self.write(" = ");
                    self.write(&access_temp);
                    self.write(" === void 0 ? ");
                    self.emit_expr(init);
                    self.write(" : ");
                    self.write(&access_temp);
                }
            }
        }

        self.emit_obj_pat_simple_rest_bucket(&simple_bucket, source_ref, first);

        if let Some(rest_pat) = rest_binding {
            self.emit_rest_transform_sep(first);
            self.emit_binding_name(rest_pat);
            self.write(" = ");
            self.write(self.helper_prefix());
            self.write("__rest(");
            self.write(source_ref);
            self.write(", ");
            self.emit_rest_excluded_keys(&excluded_keys);
            self.write(")");
        }
    }

    fn emit_var_declarator_object_rest_transform_complex(
        &mut self,
        decl: &VarDeclarator,
        props: &[ObjPatProp],
    ) {
        let Some(init) = decl.init.as_ref() else {
            self.emit_var_declarator_object_rest_transform_simple(decl, props);
            return;
        };

        // When any computed key is a dynamic expression (not a string/numeric
        // literal), TypeScript captures the source object in a temp to preserve
        // evaluation order: `_a = obj, _b = key, val = _a[_b], rest = __rest(_a, ...)`
        let has_dynamic_computed = props.iter().any(|p| {
            matches!(
                p,
                ObjPatProp::KeyValue(PropName::Computed(expr, _), _)
                    if !matches!(&expr.kind, ExprKind::StrLit(_) | ExprKind::NumLit(_))
            )
        });

        let mut first = true;
        let source_ref = match &init.kind {
            ExprKind::Ident(name) if !has_dynamic_computed => name.to_string(),
            _ => {
                let temp = self.next_rest_transform_temp(RestTransformTempKind::Inline);
                self.emit_rest_transform_sep(&mut first);
                self.write(&temp);
                self.write(" = ");
                self.suppress_oc_parens = true;
                self.emit_expr(init);
                temp
            }
        };

        self.emit_object_pat_rest_bindings_from_source(
            props,
            &source_ref,
            RestTransformTempKind::Inline,
            &mut first,
        );
    }

    fn emit_var_declarator_object_rest_transform_simple(
        &mut self,
        decl: &VarDeclarator,
        props: &[ObjPatProp],
    ) {
        let mut non_rest: Vec<&ObjPatProp> = Vec::new();
        let mut rest_binding: Option<&Pat> = None;
        let mut excluded_keys: Vec<RestExcludedKey> = Vec::new();
        let mut had_rest_stripped = false;

        for (idx, prop) in props.iter().enumerate() {
            let is_last = idx == props.len() - 1;
            match prop {
                ObjPatProp::Rest(p) => {
                    if is_last {
                        rest_binding = Some(p);
                    } else {
                        // TypeScript error recovery: rest that's not the last element
                        // is stripped from __rest emission. Its binding name becomes
                        // an excluded key for the actual last rest (if any).
                        had_rest_stripped = true;
                        if let PatKind::Ident(name) = &p.kind {
                            excluded_keys.push(RestExcludedKey::Static(name.to_string(), "\""));
                        }
                    }
                }
                ObjPatProp::KeyValue(key, _) => {
                    if let Some(k) = self.prop_name_to_excluded_key(key) {
                        excluded_keys.push(k);
                    }
                    non_rest.push(prop);
                }
                ObjPatProp::Shorthand(name, _) => {
                    excluded_keys.push(RestExcludedKey::Static(name.to_string(), "\""));
                    non_rest.push(prop);
                }
                ObjPatProp::ShorthandAssign(name, _, _) => {
                    excluded_keys.push(RestExcludedKey::Static(name.to_string(), "\""));
                    non_rest.push(prop);
                }
            }
        }

        // If there's no top-level rest, check for nested rest in KeyValue props.
        // e.g., `{ f: { a, ...spread } } = value` → `_a = value.f, { a } = _a, spread = __rest(_a, ["a"])`
        if rest_binding.is_none() && !had_rest_stripped {
            let mut first = true;
            for prop in props {
                if let ObjPatProp::KeyValue(key, val) = prop {
                    if pat_has_object_rest(val) {
                        if let PatKind::Object(inner_props) = &val.kind {
                            // Create temp: _a = init.key
                            let temp = self.next_inline_temp_var();
                            if !first {
                                self.write(", ");
                            }
                            first = false;
                            self.write(&temp);
                            self.write(" = ");
                            if let Some(ref init) = decl.init {
                                self.emit_expr(init);
                            }
                            self.write(".");
                            self.emit_prop_name(key);
                            // Emit inner destructuring + __rest from the temp
                            self.write(", ");
                            self.emit_obj_rest_destr_with_temp(inner_props, &temp);
                        }
                    }
                }
            }
            return;
        }

        if non_rest.is_empty() {
            // Only a rest element (no non-rest props): emit `rest = __rest(init, [])`
            if let Some(rest_pat) = rest_binding {
                self.emit_binding_name(rest_pat);
                self.write(" = ");
                self.write(self.helper_prefix());
                self.write("__rest(");
                if let Some(ref init) = decl.init {
                    self.suppress_oc_parens = true;
                    self.emit_expr(init);
                }
                self.write(", [])");
            }
        } else {
            // When there is both a destructuring part and a rest binding,
            // TypeScript introduces a temp variable for the RHS so the same
            // source is shared between the destructuring and __rest():
            //   `const _a = expr, { a } = _a, rest = __rest(_a, ["a"])`
            let needs_temp = (rest_binding.is_some() || had_rest_stripped)
                && decl
                    .init
                    .as_ref()
                    .is_some_and(|init| !matches!(&init.kind, ExprKind::Ident(_)));
            let temp_name = if needs_temp {
                let t = self.next_inline_temp_var();
                self.write(&t);
                self.write(" = ");
                if let Some(ref init) = decl.init {
                    self.suppress_oc_parens = true;
                    self.emit_expr(init);
                }
                self.write(", ");
                Some(t)
            } else {
                None
            };
            // Emit non-rest destructuring part: { a, x: b } = rhs
            // Preserve any source comments between properties in the original pattern.
            let pat_span_start = decl.name.span.start as usize;
            // Find the rest element to know its start position
            let rest_start = props
                .iter()
                .filter_map(|p| {
                    if matches!(p, ObjPatProp::Rest(_)) {
                        Some(obj_pat_prop_start(p) as usize)
                    } else {
                        None
                    }
                })
                .next();
            // Track if the last non-rest property has a trailing comment that
            // should be emitted after the `= init,` part (because `//` ends the line).
            let mut deferred_trailing_comment: Option<String> = None;
            self.write("{ ");
            // Check for comment between `{` and first non-rest property
            if let Some(first_prop) = non_rest.first() {
                let first_start = obj_pat_prop_start(first_prop) as usize;
                // Search from `{` (pat_span_start) to first property
                if let Some(comment) =
                    find_line_comment_in_range(self.source, pat_span_start, first_start)
                {
                    self.newline();
                    self.write(comment);
                    self.newline();
                }
            }
            let mut prev_had_comment = false;
            for (i, prop) in non_rest.iter().enumerate() {
                if i > 0 && !prev_had_comment {
                    self.write(", ");
                }
                prev_had_comment = false;
                match prop {
                    ObjPatProp::KeyValue(key, val) => {
                        self.emit_prop_name(key);
                        self.write(": ");
                        self.emit_binding_name(val);
                    }
                    ObjPatProp::Shorthand(name, _) => self.write(name),
                    ObjPatProp::ShorthandAssign(name, init, _) => {
                        self.write(name);
                        self.write(" = ");
                        self.emit_expr(init);
                    }
                    ObjPatProp::Rest(_) => unreachable!(),
                }
                // Check for trailing line comment after this property
                let prop_end = obj_pat_prop_end(prop) as usize;
                let next_start = if i + 1 < non_rest.len() {
                    obj_pat_prop_start(non_rest[i + 1]) as usize
                } else {
                    // Last non-rest property: look up to rest element start
                    rest_start.unwrap_or(prop_end)
                };
                if let Some(comment) = find_line_comment_in_range(self.source, prop_end, next_start)
                {
                    if i + 1 < non_rest.len() {
                        // Mid-property comment: emit comma, then comment.
                        // Since `//` is a line comment, this forces a newline.
                        self.write(", ");
                        self.write(comment);
                        self.newline();
                        prev_had_comment = true;
                    } else {
                        // Last property's trailing comment: defer until after `= init,`
                        deferred_trailing_comment = Some(comment.to_string());
                    }
                }
            }
            self.write(" }");
            if let Some(ref t) = temp_name {
                self.write(" = ");
                self.write(t);
            } else if let Some(ref init) = decl.init {
                self.write(" = ");
                self.suppress_oc_parens = true;
                self.emit_expr(init);
            }
            // Emit rest binding: rest = __rest(rhs, ["a", "x"])
            if let Some(rest_pat) = rest_binding {
                self.write(", ");
                // If there's a deferred trailing comment from the last non-rest
                // property, emit it after the `, ` (after `= init,`).
                if let Some(ref comment) = deferred_trailing_comment {
                    self.write(comment);
                    self.newline();
                }
                self.emit_binding_name(rest_pat);
                self.write(" = ");
                self.write(self.helper_prefix());
                self.write("__rest(");
                if let Some(ref t) = temp_name {
                    self.write(t);
                } else if let Some(ref init) = decl.init {
                    self.emit_expr(init);
                }
                self.write(", ");
                self.emit_rest_excluded_keys(&excluded_keys);
                self.write(")");
            }
        }
    }

    pub(crate) fn emit_var_declarator_object_rest_transform(&mut self, decl: &VarDeclarator) {
        match &decl.name.kind {
            PatKind::Object(props) => {
                let has_top_level_rest =
                    props.iter().any(|prop| matches!(prop, ObjPatProp::Rest(_)));
                if has_top_level_rest && self.obj_pat_props_need_complex_rest_transform(props) {
                    self.emit_var_declarator_object_rest_transform_complex(decl, props);
                } else {
                    self.emit_var_declarator_object_rest_transform_simple(decl, props);
                }
            }
            PatKind::Array(_) => {
                self.emit_var_declarator_array_rest_transform(decl);
            }
            _ => {}
        }
    }

    /// Handle `let [{ ...a }, b = a] = [{ x: 1 }]` →
    /// `let [_f, _g] = [{ x: 1 }], a = __rest(_f, []), b = _g === void 0 ? a : _g`
    fn emit_var_declarator_array_rest_transform(&mut self, decl: &VarDeclarator) {
        let Some(init) = &decl.init else { return };
        let init = init.clone();

        // Step 1: Rewrite nested object rest patterns into deferred temps
        // (deferred ensures correct naming order relative to other deferred temps
        // allocated in earlier statements)
        let Some((rewritten, transforms)) =
            self.rewrite_nested_obj_rest_pat_with_temps(&decl.name, RestTempAlloc::InlineDeferred)
        else {
            // No nested rest found — shouldn't happen since caller checks pat_has_object_rest
            self.emit_binding_name(&decl.name);
            if let Some(init) = &decl.init {
                self.write(" = ");
                self.emit_expr(init);
            }
            return;
        };

        // Step 2: Extract array elements with defaults into temps so that
        // default expressions can reference rest bindings assigned earlier.
        // `[_f, b = a]` → `[_f, _g]` with default record `b = _g === void 0 ? a : _g`
        struct DefaultRecord {
            temp: String,
            binding: Pat,
            default_expr: Expr,
        }
        let mut default_records: Vec<DefaultRecord> = Vec::new();

        let final_pat = if let PatKind::Array(elements) = &rewritten.kind {
            let new_elements: Vec<Option<ArrayPatElem>> = elements
                .iter()
                .map(|elem| match elem {
                    Some(ArrayPatElem::Pat(pat)) if matches!(&pat.kind, PatKind::Assign(_, _)) => {
                        let PatKind::Assign(inner, default_expr) = &pat.kind else {
                            unreachable!()
                        };
                        let temp = self.next_inline_deferred_temp_placeholder();
                        default_records.push(DefaultRecord {
                            temp: temp.clone(),
                            binding: (**inner).clone(),
                            default_expr: (**default_expr).clone(),
                        });
                        Some(ArrayPatElem::Pat(Pat {
                            kind: PatKind::Ident(temp.into()),
                            span: pat.span,
                        }))
                    }
                    other => other.clone(),
                })
                .collect();
            Pat {
                kind: PatKind::Array(new_elements),
                span: rewritten.span,
            }
        } else {
            rewritten
        };

        // Step 3: Emit `[_f, _g] = init`
        self.emit_binding_name(&final_pat);
        self.write(" = ");
        self.emit_expr(&init);

        // Step 4: Emit rest transforms: `, a = __rest(_f, [])`
        for transform in &transforms {
            self.write(", ");
            self.emit_obj_rest_destr_with_temp(&transform.props, &transform.temp);
        }

        // Step 5: Emit default assignments: `, b = _g === void 0 ? a : _g`
        for rec in &default_records {
            self.write(", ");
            self.emit_binding_name(&rec.binding);
            self.write(" = ");
            self.write(&rec.temp);
            self.write(" === void 0 ? ");
            self.emit_expr(&rec.default_expr);
            self.write(" : ");
            self.write(&rec.temp);
        }
    }

    /// Check if a for-of loop has object rest in its left-side binding.
    pub(crate) fn for_of_has_obj_rest(&self, fo: &ForOfStmt) -> bool {
        match &fo.left {
            ForInOfLeft::Var(vs) => vs.declarations.iter().any(|d| pat_has_object_rest(&d.name)),
            ForInOfLeft::Pat(pat) => pat_has_object_rest(pat),
            ForInOfLeft::Expr(_) => false,
        }
    }

    pub(super) fn rewrite_nested_obj_rest_pat_with_temps(
        &mut self,
        pat: &Pat,
        alloc: RestTempAlloc,
    ) -> Option<(Pat, Vec<NestedPatObjRestTransform>)> {
        let mut transforms = Vec::new();
        let rewritten = self.rewrite_nested_obj_rest_pat_inner(pat, false, alloc, &mut transforms);
        if transforms.is_empty() {
            None
        } else {
            Some((rewritten, transforms))
        }
    }

    fn rewrite_nested_obj_rest_pat_inner(
        &mut self,
        pat: &Pat,
        nested: bool,
        alloc: RestTempAlloc,
        transforms: &mut Vec<NestedPatObjRestTransform>,
    ) -> Pat {
        match &pat.kind {
            PatKind::Ident(_) => pat.clone(),
            PatKind::Object(props) => {
                let has_rest = props.iter().any(|prop| matches!(prop, ObjPatProp::Rest(_)));
                if nested && has_rest {
                    let temp = match alloc {
                        RestTempAlloc::Hoisted => self.next_temp_var(),
                        RestTempAlloc::Inline => self.next_inline_temp_var(),
                        RestTempAlloc::InlineDeferred => {
                            self.next_inline_deferred_temp_placeholder()
                        }
                    };
                    transforms.push(NestedPatObjRestTransform {
                        temp: temp.clone(),
                        props: props.clone(),
                    });
                    Pat {
                        kind: PatKind::Ident(temp.into()),
                        span: pat.span,
                    }
                } else {
                    Pat {
                        kind: PatKind::Object(
                            props
                                .iter()
                                .map(|prop| match prop {
                                    ObjPatProp::KeyValue(key, value) => ObjPatProp::KeyValue(
                                        key.clone(),
                                        self.rewrite_nested_obj_rest_pat_inner(
                                            value, true, alloc, transforms,
                                        ),
                                    ),
                                    ObjPatProp::Shorthand(name, span) => {
                                        ObjPatProp::Shorthand(name.clone(), *span)
                                    }
                                    ObjPatProp::Rest(rest) => {
                                        ObjPatProp::Rest(self.rewrite_nested_obj_rest_pat_inner(
                                            rest, true, alloc, transforms,
                                        ))
                                    }
                                    ObjPatProp::ShorthandAssign(name, init, span) => {
                                        ObjPatProp::ShorthandAssign(
                                            name.clone(),
                                            init.clone(),
                                            *span,
                                        )
                                    }
                                })
                                .collect(),
                        ),
                        span: pat.span,
                    }
                }
            }
            PatKind::Array(elements) => Pat {
                kind: PatKind::Array(
                    elements
                        .iter()
                        .map(|elem| {
                            elem.as_ref().map(|elem| match elem {
                                ArrayPatElem::Pat(p) => {
                                    ArrayPatElem::Pat(self.rewrite_nested_obj_rest_pat_inner(
                                        p, true, alloc, transforms,
                                    ))
                                }
                                ArrayPatElem::Rest(p) => {
                                    ArrayPatElem::Rest(self.rewrite_nested_obj_rest_pat_inner(
                                        p, true, alloc, transforms,
                                    ))
                                }
                            })
                        })
                        .collect(),
                ),
                span: pat.span,
            },
            PatKind::Assign(inner, init) => Pat {
                kind: PatKind::Assign(
                    Box::new(
                        self.rewrite_nested_obj_rest_pat_inner(inner, nested, alloc, transforms),
                    ),
                    init.clone(),
                ),
                span: pat.span,
            },
            PatKind::Rest(inner) => Pat {
                kind: PatKind::Rest(Box::new(
                    self.rewrite_nested_obj_rest_pat_inner(inner, nested, alloc, transforms),
                )),
                span: pat.span,
            },
        }
    }

    pub(super) fn rewrite_nested_obj_rest_expr_with_hoisted_temps(
        &mut self,
        expr: &Expr,
    ) -> Option<(Expr, Vec<NestedExprObjRestTransform>)> {
        let mut transforms = Vec::new();
        let mut rewritten = self.rewrite_nested_obj_rest_expr_inner(expr, false, &mut transforms);
        if transforms.is_empty() {
            None
        } else {
            Self::clear_expr_spans_for_emit(&mut rewritten);
            Some((rewritten, transforms))
        }
    }

    fn clear_expr_spans_for_emit(expr: &mut Expr) {
        expr.span = Span { start: 0, end: 0 };
        match &mut expr.kind {
            ExprKind::ArrayLit(elements) => {
                for elem in elements.iter_mut().flatten() {
                    Self::clear_expr_spans_for_emit(elem);
                }
            }
            ExprKind::ObjectLit(props) => {
                for prop in props {
                    match prop {
                        ObjLitProp::Property(prop) => {
                            Self::clear_expr_spans_for_emit(&mut prop.value)
                        }
                        ObjLitProp::ShorthandDefault(_, init, _) => {
                            Self::clear_expr_spans_for_emit(init)
                        }
                        ObjLitProp::Spread(inner, _) => Self::clear_expr_spans_for_emit(inner),
                        _ => {}
                    }
                }
            }
            ExprKind::Paren(inner)
            | ExprKind::NonNull(inner)
            | ExprKind::Typeof(inner)
            | ExprKind::Delete(inner)
            | ExprKind::Void(inner)
            | ExprKind::Spread(inner)
            | ExprKind::Await(inner) => Self::clear_expr_spans_for_emit(inner),
            ExprKind::As(inner) => Self::clear_expr_spans_for_emit(&mut inner.expr),
            ExprKind::Satisfies(inner) => Self::clear_expr_spans_for_emit(&mut inner.expr),
            ExprKind::TypeAssertion(inner) => Self::clear_expr_spans_for_emit(&mut inner.expr),
            ExprKind::Instantiation(inner) => Self::clear_expr_spans_for_emit(&mut inner.expr),
            _ => {}
        }
    }

    fn rewrite_nested_obj_rest_expr_inner(
        &mut self,
        expr: &Expr,
        nested: bool,
        transforms: &mut Vec<NestedExprObjRestTransform>,
    ) -> Expr {
        match &expr.kind {
            ExprKind::ObjectLit(props) => {
                let has_rest = props
                    .iter()
                    .any(|prop| matches!(prop, ObjLitProp::Spread(_, _)));
                if nested && has_rest {
                    let temp = self.next_rest_transform_temp(RestTransformTempKind::Hoisted);
                    transforms.push(NestedExprObjRestTransform::Object {
                        temp: temp.clone(),
                        props: props.clone(),
                    });
                    Expr {
                        kind: ExprKind::Ident(temp.into()),
                        span: expr.span,
                    }
                } else {
                    Expr {
                        kind: ExprKind::ObjectLit(
                            props
                                .iter()
                                .map(|prop| match prop {
                                    ObjLitProp::Property(prop) => ObjLitProp::Property(ObjProp {
                                        key: prop.key.clone(),
                                        value: Box::new(self.rewrite_nested_obj_rest_expr_inner(
                                            &prop.value,
                                            true,
                                            transforms,
                                        )),
                                        computed: prop.computed,
                                        question_token: prop.question_token,
                                        span: prop.span,
                                    }),
                                    ObjLitProp::Shorthand(name, span) => {
                                        ObjLitProp::Shorthand(name.clone(), *span)
                                    }
                                    ObjLitProp::ShorthandDefault(name, init, span) => {
                                        ObjLitProp::ShorthandDefault(
                                            name.clone(),
                                            init.clone(),
                                            *span,
                                        )
                                    }
                                    ObjLitProp::Spread(inner, span) => ObjLitProp::Spread(
                                        Box::new(self.rewrite_nested_obj_rest_expr_inner(
                                            inner, true, transforms,
                                        )),
                                        *span,
                                    ),
                                    ObjLitProp::Method(method) => {
                                        ObjLitProp::Method(method.clone())
                                    }
                                    ObjLitProp::Get(acc) => ObjLitProp::Get(acc.clone()),
                                    ObjLitProp::Set(acc) => ObjLitProp::Set(acc.clone()),
                                })
                                .collect(),
                        ),
                        span: expr.span,
                    }
                }
            }
            ExprKind::ArrayLit(_) if nested && expr_has_defaulted_object_rest_assignment(expr) => {
                // TypeScript flattens a nested array before resolving defaults
                // that contain object-rest targets:
                //   [[{ a, ...r } = d]] = value
                // becomes
                //   [_a] = value, [_b] = _a, _c = _b === void 0 ? d : _b, ...
                // Capturing this array first preserves iterator/evaluation order
                // and keeps later comma expressions out of assignment targets.
                let source_temp = self.next_rest_transform_temp(RestTransformTempKind::Hoisted);
                let mut nested_transforms = Vec::new();
                let mut pattern =
                    self.rewrite_nested_obj_rest_expr_inner(expr, false, &mut nested_transforms);
                Self::clear_expr_spans_for_emit(&mut pattern);
                transforms.push(NestedExprObjRestTransform::Array {
                    source_temp: source_temp.clone(),
                    pattern,
                    transforms: nested_transforms,
                });
                Expr {
                    kind: ExprKind::Ident(source_temp.into()),
                    span: expr.span,
                }
            }
            ExprKind::ArrayLit(elements) => Expr {
                kind: ExprKind::ArrayLit(
                    elements
                        .iter()
                        .map(|elem| {
                            elem.as_ref().map(|elem| {
                                Box::new(
                                    self.rewrite_nested_obj_rest_expr_inner(elem, true, transforms),
                                )
                            })
                        })
                        .collect(),
                ),
                span: expr.span,
            },
            ExprKind::Assign(assign)
                if nested
                    && assign.op == AssignOp::Assign
                    && assign_target_has_object_rest(&assign.left) =>
            {
                let source_temp = self.next_rest_transform_temp(RestTransformTempKind::Hoisted);
                transforms.push(NestedExprObjRestTransform::Defaulted {
                    source_temp: source_temp.clone(),
                    target: (*assign.left).clone(),
                    default_expr: (*assign.right).clone(),
                });
                Expr {
                    kind: ExprKind::Ident(source_temp.into()),
                    span: expr.span,
                }
            }
            ExprKind::Paren(inner) => Expr {
                kind: ExprKind::Paren(Box::new(
                    self.rewrite_nested_obj_rest_expr_inner(inner, nested, transforms),
                )),
                span: expr.span,
            },
            ExprKind::As(as_expr) => Expr {
                kind: ExprKind::As(Box::new(AsExpr {
                    expr: Box::new(self.rewrite_nested_obj_rest_expr_inner(
                        &as_expr.expr,
                        nested,
                        transforms,
                    )),
                    type_node: as_expr.type_node.clone(),
                })),
                span: expr.span,
            },
            ExprKind::Satisfies(sat_expr) => Expr {
                kind: ExprKind::Satisfies(Box::new(SatisfiesExpr {
                    expr: Box::new(self.rewrite_nested_obj_rest_expr_inner(
                        &sat_expr.expr,
                        nested,
                        transforms,
                    )),
                    type_node: sat_expr.type_node.clone(),
                })),
                span: expr.span,
            },
            ExprKind::TypeAssertion(ta_expr) => Expr {
                kind: ExprKind::TypeAssertion(Box::new(TypeAssertionExpr {
                    type_node: ta_expr.type_node.clone(),
                    expr: Box::new(self.rewrite_nested_obj_rest_expr_inner(
                        &ta_expr.expr,
                        nested,
                        transforms,
                    )),
                })),
                span: expr.span,
            },
            ExprKind::NonNull(inner) => Expr {
                kind: ExprKind::NonNull(Box::new(
                    self.rewrite_nested_obj_rest_expr_inner(inner, nested, transforms),
                )),
                span: expr.span,
            },
            _ => expr.clone(),
        }
    }

    pub(super) fn emit_nested_obj_rest_expr_transform(
        &mut self,
        transform: &NestedExprObjRestTransform,
    ) {
        match transform {
            NestedExprObjRestTransform::Object { temp, props } => {
                let temp_expr = Expr {
                    kind: ExprKind::Ident(temp.clone().into()),
                    span: Span { start: 0, end: 0 },
                };
                self.emit_obj_rest_assign_downlevel(props, &temp_expr, false);
            }
            NestedExprObjRestTransform::Defaulted {
                source_temp,
                target,
                default_expr,
            } => {
                // Allocate resolved-default temps only after every source slot
                // in the containing array has been allocated. This gives the
                // same `_a, _b` capture then `_c, _d` resolution order as tsc.
                let resolved = self.next_rest_transform_temp(RestTransformTempKind::Hoisted);
                self.write(&resolved);
                self.write(" = ");
                self.write(source_temp);
                self.write(" === void 0 ? ");
                self.emit_expr(default_expr);
                self.write(" : ");
                self.write(source_temp);

                let mut first = false;
                self.emit_expr_binding_from_source(
                    target,
                    &resolved,
                    RestTransformTempKind::Hoisted,
                    &mut first,
                );
            }
            NestedExprObjRestTransform::Array {
                source_temp,
                pattern,
                transforms,
            } => {
                self.emit_expr(pattern);
                self.write(" = ");
                self.write(source_temp);
                for nested in transforms {
                    self.write(", ");
                    self.emit_nested_obj_rest_expr_transform(nested);
                }
            }
        }
    }

    fn expr_target_can_use_simple_rest_destructure(&self, expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Ident(_)
            | ExprKind::Member(_)
            | ExprKind::ElemAccess(_)
            | ExprKind::This
            | ExprKind::Super => true,
            ExprKind::Paren(inner) | ExprKind::NonNull(inner) => {
                self.expr_target_can_use_simple_rest_destructure(inner)
            }
            ExprKind::As(a) => self.expr_target_can_use_simple_rest_destructure(&a.expr),
            ExprKind::Satisfies(s) => self.expr_target_can_use_simple_rest_destructure(&s.expr),
            ExprKind::TypeAssertion(ta) => {
                self.expr_target_can_use_simple_rest_destructure(&ta.expr)
            }
            ExprKind::Instantiation(inst) => {
                self.expr_target_can_use_simple_rest_destructure(&inst.expr)
            }
            ExprKind::Assign(assign) => {
                assign.op == AssignOp::Assign
                    && self.expr_target_can_use_simple_rest_destructure(&assign.left)
                    && !matches!(&assign.right.kind, ExprKind::Assign(_))
            }
            // Nested destructuring patterns without rest can be preserved as-is.
            // TypeScript keeps `{ b: { '0': n, '1': oooo } } = o` when inner has no rest.
            ExprKind::ObjectLit(props) => {
                !props.iter().any(|p| matches!(p, ObjLitProp::Spread(_, _)))
            }
            ExprKind::ArrayLit(elems) => !elems.iter().any(|e| {
                e.as_ref()
                    .is_some_and(|e| matches!(&e.kind, ExprKind::Spread(_)))
            }),
            _ => true,
        }
    }

    fn expr_target_can_assign_directly(&self, expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Ident(_)
            | ExprKind::Member(_)
            | ExprKind::ElemAccess(_)
            | ExprKind::This
            | ExprKind::Super => true,
            ExprKind::Paren(inner) | ExprKind::NonNull(inner) => {
                self.expr_target_can_assign_directly(inner)
            }
            ExprKind::As(a) => self.expr_target_can_assign_directly(&a.expr),
            ExprKind::Satisfies(s) => self.expr_target_can_assign_directly(&s.expr),
            ExprKind::TypeAssertion(ta) => self.expr_target_can_assign_directly(&ta.expr),
            ExprKind::Instantiation(inst) => self.expr_target_can_assign_directly(&inst.expr),
            _ => false,
        }
    }

    fn obj_expr_prop_can_use_simple_rest_destructure(&self, prop: &ObjLitProp) -> bool {
        match prop {
            ObjLitProp::Property(prop) => {
                !matches!(prop.key, PropName::Computed(_, _) | PropName::Private(_, _))
                    && self.expr_target_can_use_simple_rest_destructure(&prop.value)
            }
            ObjLitProp::Shorthand(_, _) => true,
            ObjLitProp::ShorthandDefault(_, init, _) => !matches!(&init.kind, ExprKind::Assign(_)),
            ObjLitProp::Spread(_, _)
            | ObjLitProp::Method(_)
            | ObjLitProp::Get(_)
            | ObjLitProp::Set(_) => false,
        }
    }

    fn obj_expr_props_need_complex_rest_transform(&self, props: &[ObjLitProp]) -> bool {
        props.iter().any(|prop| {
            !matches!(prop, ObjLitProp::Spread(_, _))
                && !self.obj_expr_prop_can_use_simple_rest_destructure(prop)
        })
    }

    fn obj_expr_prop_excluded_key(&self, prop: &ObjLitProp) -> Option<RestExcludedKey> {
        match prop {
            ObjLitProp::Property(prop) => self.prop_name_to_excluded_key(&prop.key),
            ObjLitProp::Shorthand(name, _) | ObjLitProp::ShorthandDefault(name, _, _) => {
                Some(RestExcludedKey::Static(name.to_string(), "\""))
            }
            ObjLitProp::Method(method) => self.prop_name_to_excluded_key(&method.name),
            ObjLitProp::Get(acc) | ObjLitProp::Set(acc) => {
                self.prop_name_to_excluded_key(&acc.name)
            }
            ObjLitProp::Spread(_, _) => None,
        }
    }

    fn emit_obj_expr_simple_rest_bucket(
        &mut self,
        props: &[&ObjLitProp],
        source_ref: &str,
        first: &mut bool,
    ) {
        if props.is_empty() {
            return;
        }
        self.emit_rest_transform_sep(first);
        self.write("{ ");
        for (i, prop) in props.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            self.emit_obj_lit_prop(prop);
        }
        self.write(" } = ");
        self.write(source_ref);
    }

    fn emit_expr_binding_from_source(
        &mut self,
        expr: &Expr,
        source_ref: &str,
        temp_kind: RestTransformTempKind,
        first: &mut bool,
    ) {
        match &expr.kind {
            ExprKind::Assign(assign) if assign.op == AssignOp::Assign => {
                if self.expr_target_can_assign_directly(&assign.left) {
                    self.emit_rest_transform_sep(first);
                    self.emit_expr(&assign.left);
                    self.write(" = ");
                    self.write(source_ref);
                    self.write(" === void 0 ? ");
                    self.emit_expr(&assign.right);
                    self.write(" : ");
                    self.write(source_ref);
                } else {
                    let resolved = self.next_rest_transform_temp(temp_kind);
                    self.emit_rest_transform_sep(first);
                    self.write(&resolved);
                    self.write(" = ");
                    self.write(source_ref);
                    self.write(" === void 0 ? ");
                    self.emit_expr(&assign.right);
                    self.write(" : ");
                    self.write(source_ref);
                    self.emit_expr_binding_from_source(&assign.left, &resolved, temp_kind, first);
                }
            }
            ExprKind::ObjectLit(props) => {
                if props
                    .iter()
                    .any(|prop| matches!(prop, ObjLitProp::Spread(_, _)))
                {
                    self.emit_object_expr_rest_bindings_from_source(
                        props, source_ref, temp_kind, first,
                    );
                } else {
                    self.emit_object_expr_no_rest_bindings_from_source(
                        props, source_ref, temp_kind, first,
                    );
                }
            }
            ExprKind::ArrayLit(_) if expr_has_object_rest(expr) => {
                if let Some((rewritten, transforms)) =
                    self.rewrite_nested_obj_rest_expr_with_hoisted_temps(expr)
                {
                    self.emit_rest_transform_sep(first);
                    self.emit_expr(&rewritten);
                    self.write(" = ");
                    self.write(source_ref);
                    for transform in transforms {
                        self.emit_rest_transform_sep(first);
                        self.emit_nested_obj_rest_expr_transform(&transform);
                    }
                }
            }
            _ => {
                self.emit_rest_transform_sep(first);
                self.emit_expr(expr);
                self.write(" = ");
                self.write(source_ref);
            }
        }
    }

    fn emit_expr_binding_from_member_source(
        &mut self,
        expr: &Expr,
        source_ref: &str,
        key: &PropName,
        temp_kind: RestTransformTempKind,
        first: &mut bool,
    ) {
        if matches!(expr.kind, ExprKind::ArrayLit(_)) {
            if let Some((rewritten, transforms)) =
                self.rewrite_nested_obj_rest_expr_with_hoisted_temps(expr)
            {
                self.emit_rest_transform_sep(first);
                self.emit_expr(&rewritten);
                self.write(" = ");
                self.emit_source_ref_member_access(source_ref, key);
                for transform in transforms {
                    self.emit_rest_transform_sep(first);
                    self.emit_nested_obj_rest_expr_transform(&transform);
                }
                return;
            }
        }
        if self.expr_target_can_assign_directly(expr) {
            self.emit_rest_transform_sep(first);
            self.emit_expr(expr);
            self.write(" = ");
            self.emit_source_ref_member_access(source_ref, key);
            return;
        }

        let access_temp = self.next_rest_transform_temp(temp_kind);
        self.emit_rest_transform_sep(first);
        self.write(&access_temp);
        self.write(" = ");
        self.emit_source_ref_member_access(source_ref, key);
        self.emit_expr_binding_from_source(expr, &access_temp, temp_kind, first);
    }

    fn emit_expr_binding_from_computed_source(
        &mut self,
        expr: &Expr,
        source_ref: &str,
        key_temp: &str,
        temp_kind: RestTransformTempKind,
        first: &mut bool,
    ) {
        if self.expr_target_can_assign_directly(expr) {
            self.emit_rest_transform_sep(first);
            self.emit_expr(expr);
            self.write(" = ");
            self.emit_source_ref_computed_access(source_ref, key_temp);
            return;
        }

        let access_temp = self.next_rest_transform_temp(temp_kind);
        self.emit_rest_transform_sep(first);
        self.write(&access_temp);
        self.write(" = ");
        self.emit_source_ref_computed_access(source_ref, key_temp);
        self.emit_expr_binding_from_source(expr, &access_temp, temp_kind, first);
    }

    fn emit_object_expr_no_rest_bindings_from_source(
        &mut self,
        props: &[ObjLitProp],
        source_ref: &str,
        temp_kind: RestTransformTempKind,
        first: &mut bool,
    ) {
        for prop in props {
            match prop {
                ObjLitProp::Property(prop) => match &prop.key {
                    PropName::Computed(expr, _) => {
                        let key_temp = self.next_rest_transform_temp(temp_kind);
                        self.emit_rest_transform_sep(first);
                        self.write(&key_temp);
                        self.write(" = ");
                        self.emit_expr(expr);
                        self.emit_expr_binding_from_computed_source(
                            &prop.value,
                            source_ref,
                            &key_temp,
                            temp_kind,
                            first,
                        );
                    }
                    _ => self.emit_expr_binding_from_member_source(
                        &prop.value,
                        source_ref,
                        &prop.key,
                        temp_kind,
                        first,
                    ),
                },
                ObjLitProp::Shorthand(name, _) => {
                    self.emit_rest_transform_sep(first);
                    self.write(name);
                    self.write(" = ");
                    self.write(source_ref);
                    self.write(".");
                    self.write(name);
                }
                ObjLitProp::ShorthandDefault(name, init, _) => {
                    let access_temp = self.next_rest_transform_temp(temp_kind);
                    self.emit_rest_transform_sep(first);
                    self.write(&access_temp);
                    self.write(" = ");
                    self.write(source_ref);
                    self.write(".");
                    self.write(name);
                    self.emit_rest_transform_sep(first);
                    self.write(name);
                    self.write(" = ");
                    self.write(&access_temp);
                    self.write(" === void 0 ? ");
                    self.emit_expr(init);
                    self.write(" : ");
                    self.write(&access_temp);
                }
                ObjLitProp::Spread(_, _)
                | ObjLitProp::Method(_)
                | ObjLitProp::Get(_)
                | ObjLitProp::Set(_) => {}
            }
        }
    }

    fn emit_object_expr_rest_bindings_from_source(
        &mut self,
        props: &[ObjLitProp],
        source_ref: &str,
        temp_kind: RestTransformTempKind,
        first: &mut bool,
    ) {
        let mut simple_bucket: Vec<&ObjLitProp> = Vec::new();
        let mut excluded_keys: Vec<RestExcludedKey> = Vec::new();
        let mut rest_expr: Option<&Expr> = None;

        for (idx, prop) in props.iter().enumerate() {
            let is_last = idx == props.len() - 1;
            match prop {
                ObjLitProp::Spread(expr, _) => {
                    if is_last {
                        rest_expr = Some(expr);
                    } else {
                        // TypeScript error recovery: non-last spread is stripped
                        if let ExprKind::Ident(name) = &expr.kind {
                            excluded_keys.push(RestExcludedKey::Static(name.to_string(), "\""));
                        }
                    }
                }
                _ if self.obj_expr_prop_can_use_simple_rest_destructure(prop) => {
                    if let Some(key) = self.obj_expr_prop_excluded_key(prop) {
                        excluded_keys.push(key);
                    }
                    simple_bucket.push(prop);
                }
                ObjLitProp::Property(prop) => {
                    self.emit_obj_expr_simple_rest_bucket(&simple_bucket, source_ref, first);
                    simple_bucket.clear();
                    match &prop.key {
                        PropName::Computed(expr, _) => {
                            let key_temp = self.next_rest_transform_temp(temp_kind);
                            excluded_keys.push(RestExcludedKey::DynamicTemp(key_temp.clone()));
                            self.emit_rest_transform_sep(first);
                            self.write(&key_temp);
                            self.write(" = ");
                            self.emit_expr(expr);
                            self.emit_expr_binding_from_computed_source(
                                &prop.value,
                                source_ref,
                                &key_temp,
                                temp_kind,
                                first,
                            );
                        }
                        _ => {
                            if let Some(key) = self.prop_name_to_excluded_key(&prop.key) {
                                excluded_keys.push(key);
                            }
                            self.emit_expr_binding_from_member_source(
                                &prop.value,
                                source_ref,
                                &prop.key,
                                temp_kind,
                                first,
                            );
                        }
                    }
                }
                ObjLitProp::Shorthand(name, _) => {
                    self.emit_obj_expr_simple_rest_bucket(&simple_bucket, source_ref, first);
                    simple_bucket.clear();
                    excluded_keys.push(RestExcludedKey::Static(name.to_string(), "\""));
                    self.emit_rest_transform_sep(first);
                    self.write(name);
                    self.write(" = ");
                    self.write(source_ref);
                    self.write(".");
                    self.write(name);
                }
                ObjLitProp::ShorthandDefault(name, init, _) => {
                    self.emit_obj_expr_simple_rest_bucket(&simple_bucket, source_ref, first);
                    simple_bucket.clear();
                    excluded_keys.push(RestExcludedKey::Static(name.to_string(), "\""));
                    let access_temp = self.next_rest_transform_temp(temp_kind);
                    self.emit_rest_transform_sep(first);
                    self.write(&access_temp);
                    self.write(" = ");
                    self.write(source_ref);
                    self.write(".");
                    self.write(name);
                    self.emit_rest_transform_sep(first);
                    self.write(name);
                    self.write(" = ");
                    self.write(&access_temp);
                    self.write(" === void 0 ? ");
                    self.emit_expr(init);
                    self.write(" : ");
                    self.write(&access_temp);
                }
                ObjLitProp::Method(_) | ObjLitProp::Get(_) | ObjLitProp::Set(_) => {
                    self.emit_obj_expr_simple_rest_bucket(&simple_bucket, source_ref, first);
                    simple_bucket.clear();
                }
            }
        }

        self.emit_obj_expr_simple_rest_bucket(&simple_bucket, source_ref, first);

        if let Some(rest_expr) = rest_expr {
            if self.needs_downlevel("optional-chaining") && super::emit_expr::is_oc_chain(rest_expr)
            {
                self.suppress_oc_parens = true;
            }
            self.emit_rest_transform_sep(first);
            self.emit_expr(rest_expr);
            self.write(" = ");
            self.write(self.helper_prefix());
            self.write("__rest(");
            self.write(source_ref);
            self.write(", ");
            self.emit_rest_excluded_keys(&excluded_keys);
            self.write(")");
        }
    }

    /// Emit object rest destructuring in an assignment expression.
    ///
    /// `({ ...bar } = {})` → `(bar = __rest({}, []))`
    /// `({ a, ...rest } = obj)` → `({ a } = obj, rest = __rest(obj, ["a"]))`
    ///
    /// The `props` are from an `ExprKind::ObjectLit` on the LHS of an assignment,
    /// where at least one prop is `ObjLitProp::Spread`.
    pub(crate) fn emit_obj_rest_assign_downlevel(
        &mut self,
        props: &[ObjLitProp],
        rhs: &Expr,
        preserve_result: bool,
    ) {
        let needs_complex =
            preserve_result || self.obj_expr_props_need_complex_rest_transform(props);
        if needs_complex {
            if preserve_result {
                self.write("(");
            }
            // When the RHS is a simple identifier (no side effects), use it
            // directly as the source reference instead of caching in a temp.
            // TypeScript: `({x: {ka, ...r}, ...rest} = obj)` → `(_a = obj.x, {ka} = _a, r = __rest(_a, ["ka"]), rest = __rest(obj, ["x"]))`
            // vs caching: `(_a = obj, _b = _a.x, ...)` — uses extra temp.
            let has_dynamic_computed = props.iter().any(|prop| {
                matches!(
                    prop,
                    ObjLitProp::Property(ObjProp {
                        key: PropName::Computed(key, _),
                        ..
                    }) if !matches!(key.kind, ExprKind::StrLit(_) | ExprKind::NumLit(_))
                )
            });
            let rhs_simple = rhs_is_simple_ident_ref(rhs) && !has_dynamic_computed;
            let mut first = true;
            if rhs_simple {
                // Emit the RHS to a string to use as source_ref
                let rhs_start = self.output.len();
                self.emit_expr(rhs);
                let rhs_text = self.output[rhs_start..].to_string();
                self.output.truncate(rhs_start);
                self.emit_object_expr_rest_bindings_from_source(
                    props,
                    &rhs_text,
                    RestTransformTempKind::Hoisted,
                    &mut first,
                );
                if preserve_result {
                    self.emit_rest_transform_sep(&mut first);
                    self.write(&rhs_text);
                    self.write(")");
                }
            } else {
                let root_temp = self.next_rest_transform_temp(RestTransformTempKind::Hoisted);
                self.emit_rest_transform_sep(&mut first);
                self.write(&root_temp);
                self.write(" = ");
                self.emit_expr(rhs);
                self.emit_object_expr_rest_bindings_from_source(
                    props,
                    &root_temp,
                    RestTransformTempKind::Hoisted,
                    &mut first,
                );
                if preserve_result {
                    self.emit_rest_transform_sep(&mut first);
                    self.write(&root_temp);
                    self.write(")");
                }
            }
            return;
        }

        let mut non_rest: Vec<&ObjLitProp> = Vec::new();
        let mut rest_expr: Option<&Expr> = None;
        let mut excluded_keys: Vec<String> = Vec::new();
        let mut had_rest_stripped = false;

        for (idx, prop) in props.iter().enumerate() {
            let is_last = idx == props.len() - 1;
            match prop {
                ObjLitProp::Spread(expr, _) => {
                    if is_last {
                        rest_expr = Some(expr);
                    } else {
                        // TypeScript error recovery: non-last spread in assignment
                        // destructuring is stripped (name is NOT added to excluded keys
                        // in the assignment case, unlike var declarations).
                        had_rest_stripped = true;
                    }
                }
                ObjLitProp::Property(p) => {
                    if let Some(k) = prop_name_to_string(&p.key) {
                        excluded_keys.push(k);
                    }
                    non_rest.push(prop);
                }
                ObjLitProp::Shorthand(name, _) => {
                    excluded_keys.push(name.to_string());
                    non_rest.push(prop);
                }
                ObjLitProp::ShorthandDefault(name, _, _) => {
                    excluded_keys.push(name.to_string());
                    non_rest.push(prop);
                }
                ObjLitProp::Method(m) => {
                    if let Some(k) = prop_name_to_string(&m.name) {
                        excluded_keys.push(k);
                    }
                    non_rest.push(prop);
                }
                ObjLitProp::Get(acc) | ObjLitProp::Set(acc) => {
                    if let Some(k) = prop_name_to_string(&acc.name) {
                        excluded_keys.push(k);
                    }
                    non_rest.push(prop);
                }
            }
        }

        if non_rest.is_empty() {
            // Only a rest element: ({ ...bar } = rhs) → bar = __rest(rhs, [])
            if let Some(rest) = rest_expr {
                if let Some(mem) = self.direct_private_destructure_member(rest) {
                    if let Some((state_var, kind_str, _get_extra, set_extra, is_static)) =
                        self.private_member_write_info_for_member(mem)
                    {
                        self.needs_private_field_set = true;
                        let recv_temp =
                            self.private_member_destructure_capture_temp(mem, is_static);
                        if let Some(recv_temp_name) = recv_temp.as_ref() {
                            self.write(recv_temp_name);
                            self.write(" = ");
                            self.emit_expr(&mem.object);
                            self.write(", ");
                        }
                        let setter_param = self.private_member_proxy_param_name(&kind_str);
                        self.emit_private_member_setter_proxy_value(
                            mem,
                            recv_temp.as_deref(),
                            &state_var,
                            &kind_str,
                            set_extra.as_deref(),
                            &setter_param,
                            is_static,
                        );
                        self.write(" = ");
                        self.write(self.helper_prefix());
                        self.write("__rest(");
                        self.emit_expr(rhs);
                        self.write(", [])");
                        return;
                    }
                }

                // Non-identifier rest targets without parens (e.g., `{...{}}` or `{...[]}`)
                // need a temp var: `_a = __rest(rhs, [])` instead of `{} = __rest(rhs, [])`.
                // Parenthesized targets like `{...({})}`/`{...([])}` are kept as-is.
                let is_bare_pattern =
                    matches!(&rest.kind, ExprKind::ObjectLit(_) | ExprKind::ArrayLit(_));
                if is_bare_pattern {
                    let temp = self.next_rest_transform_temp(RestTransformTempKind::Hoisted);
                    self.write(&temp);
                } else {
                    // Optional chain target: ({ ...obj?.a } = rhs)
                    //   → obj === null || obj === void 0 ? void 0 : obj.a = __rest(rhs, [])
                    if self.needs_downlevel("optional-chaining")
                        && super::emit_expr::is_oc_chain(rest)
                    {
                        self.suppress_oc_parens = true;
                    }
                    self.emit_expr(rest);
                }
                self.write(" = ");
                self.write(self.helper_prefix());
                self.write("__rest(");
                self.emit_expr(rhs);
                self.write(", [])");
            }
        } else {
            // Mixed: ({ a, ...rest } = rhs)
            // If RHS is a simple identifier, safe to evaluate twice:
            //   { a } = ident, rest = __rest(ident, ["a"])
            // Otherwise, allocate a temp to avoid double evaluation:
            //   _a = rhs, { a } = _a, rest = __rest(_a, ["a"])
            let rhs_ref =
                if (rest_expr.is_some() || had_rest_stripped) && !rhs_is_simple_ident_ref(rhs) {
                    let temp = self.next_rest_transform_temp(RestTransformTempKind::Hoisted);
                    self.write(&temp);
                    self.write(" = ");
                    self.emit_expr(unwrap_rhs_parens_and_types(rhs));
                    self.write(", ");
                    Some(temp)
                } else {
                    None
                };
            self.write("{ ");
            for (i, prop) in non_rest.iter().enumerate() {
                if i > 0 {
                    self.write(", ");
                }
                self.emit_obj_lit_prop(prop);
            }
            self.write(" } = ");
            if let Some(ref temp) = rhs_ref {
                self.write(temp);
            } else {
                self.emit_expr(rhs);
            }
            if let Some(rest) = rest_expr {
                self.write(", ");
                self.emit_expr(rest);
                self.write(" = ");
                self.write(self.helper_prefix());
                self.write("__rest(");
                if let Some(ref temp) = rhs_ref {
                    self.write(temp);
                } else {
                    self.emit_expr(rhs);
                }
                self.write(", [");
                for (i, key) in excluded_keys.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.write("\"");
                    self.write(key);
                    self.write("\"");
                }
                self.write("])");
            }
        }
    }

    /// Emit object rest destructuring with a string temp var as RHS.
    /// Emits: `{ non_rest_props } = temp, rest = __rest(temp, ["keys"])`
    /// (without leading keyword or trailing semicolon)
    pub(crate) fn emit_obj_rest_destr_with_temp(&mut self, props: &[ObjPatProp], temp: &str) {
        let mut non_rest: Vec<usize> = Vec::new();
        let mut rest_idx: Option<usize> = None;
        let mut excluded_keys: Vec<String> = Vec::new();

        for (i, prop) in props.iter().enumerate() {
            match prop {
                ObjPatProp::Rest(_) => rest_idx = Some(i),
                ObjPatProp::KeyValue(key, _) => {
                    if let Some(k) = prop_name_to_string(key) {
                        excluded_keys.push(k);
                    }
                    non_rest.push(i);
                }
                ObjPatProp::Shorthand(name, _) => {
                    excluded_keys.push(name.to_string());
                    non_rest.push(i);
                }
                ObjPatProp::ShorthandAssign(name, _, _) => {
                    excluded_keys.push(name.to_string());
                    non_rest.push(i);
                }
            }
        }

        if !non_rest.is_empty() {
            self.write("{ ");
            for (j, &i) in non_rest.iter().enumerate() {
                if j > 0 {
                    self.write(", ");
                }
                match &props[i] {
                    ObjPatProp::KeyValue(key, val) => {
                        self.emit_prop_name(key);
                        self.write(": ");
                        self.emit_binding_name(val);
                    }
                    ObjPatProp::Shorthand(name, _) => self.write(name),
                    ObjPatProp::ShorthandAssign(name, init, _) => {
                        self.write(name);
                        self.write(" = ");
                        self.emit_expr(init);
                    }
                    ObjPatProp::Rest(_) => unreachable!(),
                }
            }
            self.write(" } = ");
            self.write(temp);
        }

        if let Some(ri) = rest_idx {
            if let ObjPatProp::Rest(rest_pat) = &props[ri] {
                if !non_rest.is_empty() {
                    self.write(", ");
                }
                self.emit_binding_name(rest_pat);
                self.write(" = ");
                self.write(self.helper_prefix());
                self.write("__rest(");
                self.write(temp);
                self.write(", [");
                for (j, key) in excluded_keys.iter().enumerate() {
                    if j > 0 {
                        self.write(", ");
                    }
                    self.write("\"");
                    self.write(key);
                    self.write("\"");
                }
                self.write("])");
            }
        }
    }
}

/// Check if a statement contains an object rest destructuring pattern.
/// Used for __rest helper scanning.
pub(crate) fn source_has_object_rest(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::Var(var_stmt) => var_stmt.declarations.iter().any(|d| {
            pat_has_object_rest(&d.name) || d.init.as_ref().is_some_and(|e| expr_has_object_rest(e))
        }),
        StmtKind::FnDecl(fn_decl) => {
            fn_decl.params.iter().any(|p| pat_has_object_rest(&p.name))
                || fn_decl
                    .body
                    .as_ref()
                    .is_some_and(|b| b.iter().any(|s| source_has_object_rest(s)))
        }
        StmtKind::ClassDecl(class_decl) => class_decl.members.iter().any(|m| match &m.kind {
            ClassMemberKind::Method(method) => {
                method.params.iter().any(|p| pat_has_object_rest(&p.name))
                    || method
                        .body
                        .as_ref()
                        .is_some_and(|b| b.iter().any(|s| source_has_object_rest(s)))
            }
            ClassMemberKind::Constructor(ctor) => {
                ctor.params.iter().any(|p| pat_has_object_rest(&p.name))
                    || ctor
                        .body
                        .as_ref()
                        .is_some_and(|b| b.iter().any(|s| source_has_object_rest(s)))
            }
            ClassMemberKind::GetAccessor(acc) | ClassMemberKind::SetAccessor(acc) => {
                acc.params.iter().any(|p| pat_has_object_rest(&p.name))
                    || acc
                        .body
                        .as_ref()
                        .is_some_and(|b| b.iter().any(|s| source_has_object_rest(s)))
            }
            ClassMemberKind::Property(prop) => prop
                .initializer
                .as_ref()
                .is_some_and(|e| expr_has_object_rest(e)),
            ClassMemberKind::StaticBlock(stmts) => stmts.iter().any(|s| source_has_object_rest(s)),
            _ => false,
        }),
        StmtKind::Export(export_decl) => match &export_decl.kind {
            ExportDeclKind::Decl(inner) | ExportDeclKind::DefaultDecl(inner) => {
                source_has_object_rest(inner)
            }
            ExportDeclKind::Default(e) => expr_has_object_rest(e),
            _ => false,
        },
        StmtKind::Expr(expr) => expr_has_object_rest(expr),
        StmtKind::Block(stmts) => stmts.iter().any(|s| source_has_object_rest(s)),
        StmtKind::If(if_stmt) => {
            source_has_object_rest(&if_stmt.consequent)
                || if_stmt
                    .alternate
                    .as_ref()
                    .is_some_and(|a| source_has_object_rest(a))
        }
        StmtKind::For(for_stmt) => {
            for_stmt.init.as_ref().is_some_and(|init| match init {
                ForInit::Var(v) => v.declarations.iter().any(|d| pat_has_object_rest(&d.name)),
                ForInit::Expr(e) => expr_has_object_rest(e),
            }) || source_has_object_rest(&for_stmt.body)
        }
        StmtKind::ForIn(fi) => {
            for_in_of_left_has_object_rest(&fi.left) || source_has_object_rest(&fi.body)
        }
        StmtKind::ForOf(fo) => {
            for_in_of_left_has_object_rest(&fo.left) || source_has_object_rest(&fo.body)
        }
        StmtKind::While(w) => source_has_object_rest(&w.body),
        StmtKind::DoWhile(dw) => source_has_object_rest(&dw.body),
        StmtKind::Return(ret) => ret.as_ref().is_some_and(|e| expr_has_object_rest(e)),
        StmtKind::Try(t) => {
            t.block.iter().any(|s| source_has_object_rest(s))
                || t.handler.as_ref().is_some_and(|h| {
                    h.param.as_ref().is_some_and(|p| pat_has_object_rest(p))
                        || h.body.iter().any(|s| source_has_object_rest(s))
                })
                || t.finalizer
                    .as_ref()
                    .is_some_and(|f| f.iter().any(|s| source_has_object_rest(s)))
        }
        StmtKind::Switch(sw) => sw
            .cases
            .iter()
            .any(|c| c.consequent.iter().any(|s| source_has_object_rest(s))),
        StmtKind::Labeled(l) => source_has_object_rest(&l.body),
        StmtKind::ModuleDecl(m) => match &m.body {
            Some(ModuleBody::Block(stmts)) => stmts.iter().any(|s| source_has_object_rest(s)),
            _ => false,
        },
        _ => false,
    }
}

/// Check if an expression contains an object rest destructuring pattern.
pub(crate) fn expr_has_object_rest(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::FnExpr(fn_decl) => {
            fn_decl.params.iter().any(|p| pat_has_object_rest(&p.name))
                || fn_decl
                    .body
                    .as_ref()
                    .is_some_and(|b| b.iter().any(|s| source_has_object_rest(s)))
        }
        ExprKind::Arrow(arrow) => {
            arrow.params.iter().any(|p| pat_has_object_rest(&p.name))
                || match &arrow.body {
                    ArrowBody::Block(stmts) => stmts.iter().any(|s| source_has_object_rest(s)),
                    ArrowBody::Expr(e) => expr_has_object_rest(e),
                }
        }
        ExprKind::ClassExpr(class_decl) => class_decl.members.iter().any(|m| match &m.kind {
            ClassMemberKind::Method(method) => {
                method.params.iter().any(|p| pat_has_object_rest(&p.name))
                    || method
                        .body
                        .as_ref()
                        .is_some_and(|b| b.iter().any(|s| source_has_object_rest(s)))
            }
            ClassMemberKind::Constructor(ctor) => {
                ctor.params.iter().any(|p| pat_has_object_rest(&p.name))
                    || ctor
                        .body
                        .as_ref()
                        .is_some_and(|b| b.iter().any(|s| source_has_object_rest(s)))
            }
            _ => false,
        }),
        ExprKind::ObjectLit(props) => props.iter().any(|p| match p {
            ObjLitProp::Method(m) => {
                m.params.iter().any(|p| pat_has_object_rest(&p.name))
                    || m.body.iter().any(|s| source_has_object_rest(s))
            }
            ObjLitProp::Get(a) | ObjLitProp::Set(a) => {
                a.params.iter().any(|p| pat_has_object_rest(&p.name))
                    || a.body.iter().any(|s| source_has_object_rest(s))
            }
            ObjLitProp::Property(p) => expr_has_object_rest(&p.value),
            ObjLitProp::Spread(e, _) => expr_has_object_rest(e),
            _ => false,
        }),
        ExprKind::Assign(assign) => {
            assign_target_has_object_rest(&assign.left) || expr_has_object_rest(&assign.right)
        }
        ExprKind::Call(call) => {
            call.args.iter().any(|a| expr_has_object_rest(a)) || expr_has_object_rest(&call.callee)
        }
        ExprKind::Paren(inner) => expr_has_object_rest(inner),
        ExprKind::Spread(inner) => expr_has_object_rest(inner),
        ExprKind::Cond(cond) => {
            expr_has_object_rest(&cond.consequent) || expr_has_object_rest(&cond.alternate)
        }
        ExprKind::NonNull(inner) => expr_has_object_rest(inner),
        ExprKind::As(a) => expr_has_object_rest(&a.expr),
        ExprKind::Satisfies(s) => expr_has_object_rest(&s.expr),
        ExprKind::TypeAssertion(ta) => expr_has_object_rest(&ta.expr),
        ExprKind::Instantiation(inst) => expr_has_object_rest(&inst.expr),
        _ => false,
    }
}

/// Unwrap parentheses and type assertions from an expression.
/// Used when assigning the RHS to a temp variable — the parens and type
/// assertions are unnecessary in the emitted JS.
fn unwrap_rhs_parens_and_types(expr: &Expr) -> &Expr {
    match &expr.kind {
        ExprKind::Paren(inner) | ExprKind::NonNull(inner) => unwrap_rhs_parens_and_types(inner),
        ExprKind::As(a) => unwrap_rhs_parens_and_types(&a.expr),
        ExprKind::Satisfies(s) => unwrap_rhs_parens_and_types(&s.expr),
        ExprKind::TypeAssertion(ta) => unwrap_rhs_parens_and_types(&ta.expr),
        _ => expr,
    }
}

/// Check if an expression is a simple identifier reference that can
/// safely be evaluated multiple times without side effects.
/// Used to decide whether to allocate a temp for the RHS of a rest
/// destructuring assignment.
fn rhs_is_simple_ident_ref(rhs: &Expr) -> bool {
    match &rhs.kind {
        ExprKind::Ident(_) => true,
        ExprKind::Paren(inner) | ExprKind::NonNull(inner) => rhs_is_simple_ident_ref(inner),
        ExprKind::As(a) => rhs_is_simple_ident_ref(&a.expr),
        ExprKind::Satisfies(s) => rhs_is_simple_ident_ref(&s.expr),
        ExprKind::TypeAssertion(ta) => rhs_is_simple_ident_ref(&ta.expr),
        _ => false,
    }
}

/// Check if an assignment target (left side of `=`) has object rest.
pub(crate) fn assign_target_has_object_rest(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::ObjectLit(props) => props.iter().any(|p| match p {
            ObjLitProp::Spread(_, _) => true,
            ObjLitProp::Property(p) => assign_target_has_object_rest(&p.value),
            _ => false,
        }),
        ExprKind::ArrayLit(elems) => elems
            .iter()
            .flatten()
            .any(|e| assign_target_has_object_rest(e)),
        ExprKind::Assign(assign) if assign.op == AssignOp::Assign => {
            assign_target_has_object_rest(&assign.left)
        }
        ExprKind::Paren(inner) => assign_target_has_object_rest(inner),
        _ => false,
    }
}

/// Whether an assignment target contains an array element whose defaulted
/// target itself contains object rest. These elements must be captured before
/// the default and rest transforms are emitted, otherwise their comma sequence
/// is emitted inside the array assignment target and produces invalid JS.
fn expr_has_defaulted_object_rest_assignment(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Assign(assign) if assign.op == AssignOp::Assign => {
            assign_target_has_object_rest(&assign.left)
                || expr_has_defaulted_object_rest_assignment(&assign.left)
        }
        ExprKind::ArrayLit(elements) => elements
            .iter()
            .flatten()
            .any(|element| expr_has_defaulted_object_rest_assignment(element)),
        ExprKind::ObjectLit(props) => props.iter().any(|prop| match prop {
            ObjLitProp::Property(prop) => expr_has_defaulted_object_rest_assignment(&prop.value),
            ObjLitProp::Spread(expr, _) | ObjLitProp::ShorthandDefault(_, expr, _) => {
                expr_has_defaulted_object_rest_assignment(expr)
            }
            _ => false,
        }),
        ExprKind::Paren(inner) | ExprKind::NonNull(inner) => {
            expr_has_defaulted_object_rest_assignment(inner)
        }
        ExprKind::As(expr) => expr_has_defaulted_object_rest_assignment(&expr.expr),
        ExprKind::Satisfies(expr) => expr_has_defaulted_object_rest_assignment(&expr.expr),
        ExprKind::TypeAssertion(expr) => expr_has_defaulted_object_rest_assignment(&expr.expr),
        _ => false,
    }
}

/// Check if a for-in/of left side has object rest.
fn for_in_of_left_has_object_rest(left: &ForInOfLeft) -> bool {
    match left {
        ForInOfLeft::Pat(p) => pat_has_object_rest(p),
        ForInOfLeft::Var(v) => v.declarations.iter().any(|d| pat_has_object_rest(&d.name)),
        ForInOfLeft::Expr(_) => false,
    }
}

/// Check if a pattern contains an object rest element (`{ a, ...rest }`).
pub(crate) fn pat_has_object_rest(pat: &Pat) -> bool {
    match &pat.kind {
        PatKind::Object(props) => props.iter().any(|p| match p {
            ObjPatProp::Rest(_) => true,
            ObjPatProp::KeyValue(_, value) => pat_has_object_rest(value),
            _ => false,
        }),
        PatKind::Array(elems) => elems.iter().flatten().any(|e| match e {
            ArrayPatElem::Pat(p) => pat_has_object_rest(p),
            ArrayPatElem::Rest(p) => pat_has_object_rest(p),
        }),
        PatKind::Assign(inner, _) => pat_has_object_rest(inner),
        PatKind::Rest(inner) => pat_has_object_rest(inner),
        PatKind::Ident(_) => false,
    }
}

/// Check if a parameter has a **top-level** object pattern with rest.
/// Only the direct pattern is checked -- this is for the parameter transform
/// which replaces the whole parameter with a temp name.
pub(crate) fn param_has_top_level_object_rest(param: &Param) -> bool {
    top_level_object_props(&param.name)
        .is_some_and(|props| props.iter().any(|p| matches!(p, ObjPatProp::Rest(_))))
}

/// Convert a property name to its string representation for the `__rest`
/// excluded keys array.
pub(crate) fn prop_name_to_string(name: &PropName) -> Option<String> {
    match name {
        PropName::Ident(s, _) | PropName::String(s, _) | PropName::Number(s, _) => {
            Some(s.to_string())
        }
        PropName::Computed(_, _) | PropName::Private(_, _) => None,
    }
}

/// Check if an expression is a bare `await` — i.e., it will become `yield` WITHOUT
/// already being wrapped in parentheses. Skips type-erasure layers but NOT Paren.
/// Use this for binary/unary operand wrapping to avoid double-parens.
pub(crate) fn expr_is_bare_await(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Await(_) => true,
        // Unwrap type layers (erased at emit time)
        ExprKind::NonNull(inner) => expr_is_bare_await(inner),
        ExprKind::As(a) => expr_is_bare_await(&a.expr),
        ExprKind::Satisfies(s) => expr_is_bare_await(&s.expr),
        ExprKind::TypeAssertion(ta) => expr_is_bare_await(&ta.expr),
        ExprKind::Instantiation(inst) => expr_is_bare_await(&inst.expr),
        // Paren already provides wrapping — don't add more
        _ => false,
    }
}

pub(crate) fn top_level_object_props(pat: &Pat) -> Option<&[ObjPatProp]> {
    match &pat.kind {
        PatKind::Object(props) => Some(props.as_slice()),
        PatKind::Assign(inner, _) | PatKind::Rest(inner) => top_level_object_props(inner),
        PatKind::Ident(_) | PatKind::Array(_) => None,
    }
}
