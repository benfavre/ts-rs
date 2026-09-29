use super::*;

/// Receiver type for `.call()` in parenthesized OC chain calls.
enum OcCallReceiver {
    /// Receiver is a simple expression (can be emitted directly).
    Simple(Expr),
    /// Receiver is a complex expression (call result) that needs a temp var.
    NeedsTemp,
}

/// Escape a raw template string for use inside a JS string literal.
/// Backslashes are doubled, quotes escaped, and control chars handled.
fn escape_template_raw_for_js_string(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() * 2);
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                // Template raw strings normalize source CR and CRLF to LF.
                out.push_str("\\n");
            }
            '\t' => out.push_str("\\t"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000B}' => out.push_str("\\v"),
            '\u{000C}' => out.push_str("\\f"),
            '\0' => out.push_str("\\0"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            _ => out.push(ch),
        }
    }
    out
}

/// Compute the cooked value of a template element from its raw text.
/// Returns None if the raw text contains invalid escape sequences.
/// Returns Some(cooked_string) with escape sequences processed.
fn compute_template_cooked(raw: &str) -> Option<String> {
    if crate::emit_stmt_helpers::template_element_has_invalid_escape(raw) {
        return None;
    }
    // This returns an already-escaped JavaScript string body. Reuse the
    // untagged cooker so lone UTF-16 surrogates and line terminators never
    // have to pass through Rust's Unicode-scalar-only `char` representation.
    Some(downlevel_template_string(raw))
}

/// Cook an untagged template element and quote it as the body of a JavaScript
/// string literal. Unlike Rust strings, JavaScript strings may contain lone
/// UTF-16 surrogates, so Unicode escapes are emitted directly instead of first
/// being converted through `char`.
fn downlevel_template_string(raw: &str) -> String {
    fn push_char(out: &mut String, ch: char) {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000B}' => out.push_str("\\v"),
            '\u{000C}' => out.push_str("\\f"),
            '\0' => out.push_str("\\0"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            _ => out.push(ch),
        }
    }

    fn push_unicode_escape(out: &mut String, value: u32, followed_by_decimal_digit: bool) {
        if value <= 0x7f {
            match value {
                0 if followed_by_decimal_digit => out.push_str("\\x00"),
                0 => out.push_str("\\0"),
                1..=7 | 14..=31 => out.push_str(&format!("\\u{value:04X}")),
                _ => push_char(
                    out,
                    char::from_u32(value).expect("ASCII is a Unicode scalar"),
                ),
            }
        } else if value <= 0xffff {
            // Preserve lone surrogates as JavaScript UTF-16 escape sequences.
            out.push_str(&format!("\\u{value:04X}"));
        } else {
            let value = value - 0x1_0000;
            let high = 0xd800 + (value >> 10);
            let low = 0xdc00 + (value & 0x3ff);
            out.push_str(&format!("\\u{high:04X}\\u{low:04X}"));
        }
    }

    let mut out = String::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'\\' {
            // Template cooked values normalize both CR and CRLF to LF.
            if bytes[i] == b'\r' {
                i += 1;
                if i < bytes.len() && bytes[i] == b'\n' {
                    i += 1;
                }
                out.push_str("\\n");
                continue;
            }
            let ch = raw[i..].chars().next().expect("valid UTF-8 boundary");
            push_char(&mut out, ch);
            i += ch.len_utf8();
            continue;
        }

        let escape_start = i;
        i += 1;
        if i >= bytes.len() {
            out.push_str("\\\\");
            break;
        }
        match bytes[i] {
            b'\\' => {
                out.push_str("\\\\");
                i += 1;
            }
            b'\'' => {
                out.push('\'');
                i += 1;
            }
            b'"' => {
                out.push_str("\\\"");
                i += 1;
            }
            b'`' => {
                out.push('`');
                i += 1;
            }
            b'n' => {
                out.push_str("\\n");
                i += 1;
            }
            b'r' => {
                out.push_str("\\r");
                i += 1;
            }
            b't' => {
                out.push_str("\\t");
                i += 1;
            }
            b'v' => {
                out.push_str("\\v");
                i += 1;
            }
            b'f' => {
                out.push_str("\\f");
                i += 1;
            }
            b'b' => {
                out.push_str("\\b");
                i += 1;
            }
            b'\n' => i += 1,
            b'\r' => {
                i += 1;
                if i < bytes.len() && bytes[i] == b'\n' {
                    i += 1;
                }
            }
            b'0'..=b'7' => {
                // Legacy octal escapes are syntax errors in template literals,
                // but tsc still cooks them during recovery emit. The first
                // digit determines whether two or three octal digits may be
                // consumed (matching JavaScript's legacy escape grammar).
                let first = bytes[i];
                let max_digits = if first <= b'3' { 3 } else { 2 };
                let mut end = i;
                let mut value = 0u32;
                let mut count = 0;
                while end < bytes.len() && count < max_digits && matches!(bytes[end], b'0'..=b'7') {
                    value = value * 8 + u32::from(bytes[end] - b'0');
                    end += 1;
                    count += 1;
                }
                let followed_by_decimal_digit = end < bytes.len() && bytes[end].is_ascii_digit();
                push_unicode_escape(&mut out, value, followed_by_decimal_digit);
                i = end;
            }
            b'x' => {
                let end = i + 3;
                if end <= bytes.len() {
                    if let Ok(value) = u8::from_str_radix(&raw[i + 1..end], 16) {
                        let followed_by_decimal_digit =
                            end < bytes.len() && bytes[end].is_ascii_digit();
                        push_unicode_escape(&mut out, value as u32, followed_by_decimal_digit);
                        i = end;
                        continue;
                    }
                }
                out.push_str("\\\\");
                i = escape_start + 1;
            }
            b'u' if i + 1 < bytes.len() && bytes[i + 1] == b'{' => {
                let digits_start = i + 2;
                let close = bytes[digits_start..]
                    .iter()
                    .position(|b| *b == b'}')
                    .map(|offset| digits_start + offset);
                if let Some(close) = close {
                    let digits = &raw[digits_start..close];
                    if !digits.is_empty() {
                        if let Ok(value) = u32::from_str_radix(digits, 16) {
                            if value <= 0x10ffff {
                                let next = close + 1;
                                let followed_by_decimal_digit =
                                    next < bytes.len() && bytes[next].is_ascii_digit();
                                push_unicode_escape(&mut out, value, followed_by_decimal_digit);
                                i = close + 1;
                                continue;
                            }
                        }
                    }
                    // Recovery emit matches tsc: keep an invalid escape as
                    // literal text, but escape its leading backslash.
                    out.push_str("\\\\");
                    out.push_str(&raw[i..=close]);
                    i = close + 1;
                } else {
                    out.push_str("\\\\");
                    i = escape_start + 1;
                }
            }
            b'u' => {
                let end = i + 5;
                if end <= bytes.len() {
                    if let Ok(value) = u16::from_str_radix(&raw[i + 1..end], 16) {
                        let followed_by_decimal_digit =
                            end < bytes.len() && bytes[end].is_ascii_digit();
                        push_unicode_escape(&mut out, value as u32, followed_by_decimal_digit);
                        i = end;
                        continue;
                    }
                }
                out.push_str("\\\\");
                i = escape_start + 1;
            }
            _ => {
                // Identity escapes cook to the escaped character.
                let ch = raw[i..].chars().next().expect("valid UTF-8 boundary");
                push_char(&mut out, ch);
                i += ch.len_utf8();
            }
        }
    }
    out
}

/// Cook raw string-literal content and re-emit it for a double-quoted
/// JavaScript string.  Keeping the result as escaped UTF-16 text is important:
/// JavaScript permits lone surrogate code units which Rust `char` cannot
/// represent.  This shares the complete escape grammar used by untagged
/// templates instead of partially decoding through Unicode scalar values.
pub(super) fn cook_string_literal_raw_for_double_quote(raw: &str) -> String {
    cook_string_or_template_body_for_double_quote(raw)
}

/// Cook a string/template body into the escaped text emitted between double
/// quotes. Keep collision admission on the same complete escape decoder as
/// actual template lowering, including lone UTF-16 surrogate handling.
pub(super) fn cook_string_or_template_body_for_double_quote(raw: &str) -> String {
    downlevel_template_string(raw)
}

/// Unwrap Paren and type assertion wrappers to find a private field member access.
/// Returns the inner `MemberExpr` if the expression is `((...(obj.#field)...))` with
/// optional type assertions.
fn unwrap_to_private_member(expr: &Expr) -> Option<&MemberExpr> {
    match &expr.kind {
        ExprKind::Member(mem) if mem.property.starts_with('#') => Some(mem),
        ExprKind::Paren(inner) => unwrap_to_private_member(inner),
        ExprKind::TypeAssertion(ta) => unwrap_to_private_member(&ta.expr),
        ExprKind::As(a) => unwrap_to_private_member(&a.expr),
        ExprKind::Satisfies(s) => unwrap_to_private_member(&s.expr),
        ExprKind::NonNull(inner) => unwrap_to_private_member(inner),
        _ => None,
    }
}

fn unwrap_private_receiver_class_name(expr: &Expr) -> Option<&str> {
    match &expr.kind {
        ExprKind::Ident(name) => Some(name.as_str()),
        ExprKind::Paren(inner) | ExprKind::NonNull(inner) => {
            unwrap_private_receiver_class_name(inner)
        }
        ExprKind::TypeAssertion(ta) => unwrap_private_receiver_class_name(&ta.expr),
        ExprKind::As(a) => unwrap_private_receiver_class_name(&a.expr),
        ExprKind::Satisfies(s) => unwrap_private_receiver_class_name(&s.expr),
        _ => None,
    }
}

pub(super) fn escape_js_string_for_quote(value: &str, quote: char) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '"' if quote == '"' => out.push_str("\\\""),
            '\'' if quote == '\'' => out.push_str("\\'"),
            _ => out.push(ch),
        }
    }
    out
}

/// Rewrite ES2015 braced Unicode escapes in an ordinary string literal for
/// pre-ES2015 targets. This operates on the raw source spelling rather than
/// the parsed value so lone UTF-16 surrogates and parser-recovery literals can
/// be emitted without passing through Rust's Unicode-scalar-only `char` type.
/// Invalid, incomplete, and out-of-range escapes are intentionally preserved.
pub(super) fn downlevel_braced_unicode_escapes_in_string(
    raw: &str,
    source_quote: char,
) -> Option<String> {
    fn push_code_point(out: &mut String, value: u32, quote: char, followed_by_digit: bool) {
        if value <= 0x7f {
            match value {
                0 if followed_by_digit => out.push_str("\\x00"),
                0 => out.push_str("\\0"),
                1..=7 | 14..=31 => out.push_str(&format!("\\u{value:04X}")),
                _ => {
                    let ch = char::from_u32(value).expect("ASCII is a Unicode scalar");
                    match ch {
                        '\\' => out.push_str("\\\\"),
                        '\n' => out.push_str("\\n"),
                        '\r' => out.push_str("\\r"),
                        '\t' => out.push_str("\\t"),
                        '\u{0008}' => out.push_str("\\b"),
                        '\u{000B}' => out.push_str("\\v"),
                        '\u{000C}' => out.push_str("\\f"),
                        '"' if quote == '"' => out.push_str("\\\""),
                        '\'' if quote == '\'' => out.push_str("\\'"),
                        _ => out.push(ch),
                    }
                }
            }
        } else if value <= 0xffff {
            out.push_str(&format!("\\u{value:04X}"));
        } else {
            let value = value - 0x1_0000;
            let high = 0xd800 + (value >> 10);
            let low = 0xdc00 + (value & 0x3ff);
            out.push_str(&format!("\\u{high:04X}\\u{low:04X}"));
        }
    }

    let bytes = raw.as_bytes();
    let has_opening_quote = bytes.first() == Some(&(source_quote as u8));
    let has_closing_quote =
        has_opening_quote && bytes.len() >= 2 && bytes.last() == Some(&(source_quote as u8));
    let body_start = usize::from(has_opening_quote);
    let body_end = raw.len() - usize::from(has_closing_quote);
    let mut output = String::with_capacity(raw.len() + 2);
    output.push('"');
    let mut i = body_start;
    let mut changed = false;

    while i < body_end {
        if bytes[i] != b'\\' {
            let ch = raw[i..body_end]
                .chars()
                .next()
                .expect("string body index is a character boundary");
            if ch == '"' {
                output.push_str("\\\"");
            } else {
                output.push(ch);
            }
            i += ch.len_utf8();
            continue;
        }

        let Some(&escaped) = bytes.get(i + 1).filter(|_| i + 1 < body_end) else {
            output.push_str("\\\\");
            i += 1;
            continue;
        };

        // A doubled backslash is one cooked backslash. Retain two backslashes
        // in the canonical output, and do not treat its second byte as the
        // start of another escape.
        if escaped == b'\\' {
            output.push_str("\\\\");
            i += 2;
            continue;
        }

        if escaped == b'\n' {
            i += 2;
            continue;
        }
        if escaped == b'\r' {
            i += 2 + usize::from(bytes.get(i + 2) == Some(&b'\n'));
            continue;
        }

        if escaped == b'\'' {
            output.push('\'');
            i += 2;
            continue;
        }
        if escaped == b'"' {
            output.push_str("\\\"");
            i += 2;
            continue;
        }

        if escaped == b'x' {
            let digits_end = i + 4;
            if digits_end <= body_end && bytes[i + 2..digits_end].iter().all(u8::is_ascii_hexdigit)
            {
                let value = u32::from_str_radix(&raw[i + 2..digits_end], 16)
                    .expect("two hexadecimal digits fit in u32");
                push_code_point(
                    &mut output,
                    value,
                    '"',
                    bytes.get(digits_end).is_some_and(u8::is_ascii_digit),
                );
                i = digits_end;
                continue;
            }
            // Malformed hexadecimal and Unicode escapes have a literal cooked
            // backslash, which must itself be escaped in generated JavaScript.
            output.push_str("\\\\");
            i += 1;
            continue;
        }

        if escaped != b'u' {
            match escaped {
                b'b' | b'f' | b'n' | b'r' | b't' | b'v' => {
                    output.push('\\');
                    output.push(escaped as char);
                }
                b'0' => push_code_point(
                    &mut output,
                    0,
                    '"',
                    bytes.get(i + 2).is_some_and(u8::is_ascii_digit),
                ),
                // Identity escapes cook to the escaped character. This also
                // covers `\\/`, `\\8`, and `\\9`.
                _ => output.push(escaped as char),
            }
            i += 2;
            continue;
        }

        if bytes.get(i + 2) != Some(&b'{') || i + 2 >= body_end {
            let digits_end = i + 6;
            if digits_end <= body_end && bytes[i + 2..digits_end].iter().all(u8::is_ascii_hexdigit)
            {
                let value = u32::from_str_radix(&raw[i + 2..digits_end], 16)
                    .expect("four hexadecimal digits fit in u32");
                push_code_point(
                    &mut output,
                    value,
                    '"',
                    bytes.get(digits_end).is_some_and(u8::is_ascii_digit),
                );
                i = digits_end;
                continue;
            }
            output.push_str("\\\\");
            i += 1;
            continue;
        }

        let digits_start = i + 3;
        let mut close = digits_start;
        while close < body_end && bytes[close].is_ascii_hexdigit() {
            close += 1;
        }
        if close == digits_start || close >= body_end || bytes[close] != b'}' {
            output.push_str("\\\\");
            i += 1;
            continue;
        }

        let Ok(value) = u32::from_str_radix(&raw[digits_start..close], 16) else {
            output.push_str("\\\\");
            i += 1;
            continue;
        };
        if value > 0x10ffff {
            output.push_str("\\\\");
            i += 1;
            continue;
        }

        let next = close + 1;
        push_code_point(
            &mut output,
            value,
            '"',
            bytes.get(next).is_some_and(u8::is_ascii_digit),
        );
        i = next;
        changed = true;
    }

    if !changed {
        return None;
    }
    // Once a braced escape is transformed, TypeScript prints the entire cooked
    // value canonically as a double-quoted string. This also closes a recovered
    // unterminated literal whose complete escape has a well-defined value.
    output.push('"');
    Some(output)
}

/// Get the span end position of an ObjLitProp.
fn obj_lit_prop_span_end(prop: &ObjLitProp) -> u32 {
    match prop {
        ObjLitProp::Property(p) => p.span.end,
        ObjLitProp::Shorthand(_, span) => span.end,
        ObjLitProp::ShorthandDefault(_, _, span) => span.end,
        ObjLitProp::Spread(_, span) => span.end,
        ObjLitProp::Method(m) => m.span.end,
        ObjLitProp::Get(a) => a.span.end,
        ObjLitProp::Set(a) => a.span.end,
    }
}

impl<'a> Emitter<'a> {
    /// Discover the function-local temps allocated by a concise arrow body
    /// without mutating live output, source-map, comment, or naming state.
    /// The returned names are used to choose the final block layout before the
    /// body is emitted exactly once by the live emitter.
    fn discover_hazard_free_arrow_body_temps(&mut self, body: &Expr) -> Vec<AstString> {
        // Avoid cloning the potentially large generated output and mapping
        // vectors. The probe otherwise needs the complete emitter context so
        // temp allocation follows the same transforms as final emission.
        let live_output = std::mem::take(&mut self.output);
        let live_source_map = self.source_map_gen.take();
        let mut probe = self.clone();
        self.output = live_output;
        self.source_map_gen = live_source_map;

        probe.output = String::new();
        probe.source_map_gen = None;
        probe.emit_hazard_free_arrow_return_expr(body);
        probe.temp_var_names
    }

    fn emit_hazard_free_arrow_return_expr(&mut self, body: &Expr) {
        self.sync_source_map_output_column();
        let wrap_return = super::emit_stmt_helpers::expr_stmt_needs_outer_paren_wrap(body);
        if wrap_return {
            self.write("(");
        }
        let prev_suppress_oc_parens = self.suppress_oc_parens;
        self.suppress_oc_parens = true;
        self.emit_expr(body);
        self.suppress_oc_parens = prev_suppress_oc_parens;
        if wrap_return {
            self.write(")");
        }
    }

    fn emit_expr_with_discarded_value(&mut self, expr: &Expr, value_discarded: bool) {
        let prev = self.update_value_discarded;
        self.update_value_discarded = value_discarded;
        self.emit_expr(expr);
        self.update_value_discarded = prev;
    }

    pub(super) fn emit_expr(&mut self, expr: &Expr) {
        let previous_depth = self.emit_expr_depth;
        self.emit_expr_depth += 1;
        self.emit_expr_inner(expr);
        self.emit_expr_depth = previous_depth;
    }

    fn emit_expr_inner(&mut self, expr: &Expr) {
        // Save and clear the expression-statement position flag so that
        // sub-expressions (recursive emit_expr calls) see it as false.
        let cjs_in_expr_stmt = self.cjs_export_in_expr_stmt;
        self.cjs_export_in_expr_stmt = false;
        let value_discarded = self.update_value_discarded;
        self.update_value_discarded = false;

        // Reset optional-chain paren suppression for non-OC expressions.
        // Only optional chain expressions that are being downleveled should
        // consume this flag; all other expression types reset it so it doesn't
        // leak into sub-expressions.
        let is_oc_downlevel = match &expr.kind {
            ExprKind::Member(mem) => {
                (mem.optional || (!mem.optional && is_oc_chain(&mem.object)))
                    && self.needs_downlevel("optional-chaining")
            }
            ExprKind::ElemAccess(ea) => {
                (ea.optional || (!ea.optional && is_oc_chain(&ea.object)))
                    && self.needs_downlevel("optional-chaining")
            }
            ExprKind::Call(call) => {
                (call.optional || (!call.optional && is_oc_chain(&call.callee)))
                    && self.needs_downlevel("optional-chaining")
            }
            ExprKind::Binary(bin) if bin.op == BinaryOp::NullCoal => {
                self.needs_downlevel("nullish-coalescing")
            }
            ExprKind::Assign(assign)
                if matches!(
                    assign.op,
                    AssignOp::LogAndAssign | AssignOp::LogOrAssign | AssignOp::NullCoalAssign
                ) =>
            {
                self.needs_downlevel("logical-assignment")
            }
            // Type-stripping wrappers: preserve suppress_oc_parens if inner is OC
            ExprKind::NonNull(inner) => {
                is_oc_chain(inner) && self.needs_downlevel("optional-chaining")
            }
            ExprKind::TypeAssertion(ta) => {
                is_oc_chain(&ta.expr) && self.needs_downlevel("optional-chaining")
            }
            ExprKind::As(a) => is_oc_chain(&a.expr) && self.needs_downlevel("optional-chaining"),
            ExprKind::Satisfies(s) => {
                is_oc_chain(&s.expr) && self.needs_downlevel("optional-chaining")
            }
            _ => false,
        };
        if !is_oc_downlevel {
            self.suppress_oc_parens = false;
        }

        if self.rewrite_relative_import_extensions() {
            if let ExprKind::Call(call) = &expr.kind {
                if self.is_dynamic_import_call(call) && !self.should_downlevel_dynamic_import() {
                    self.write("import(");
                    if let Some(arg) = call.args.first() {
                        self.emit_rewritten_relative_import_arg(arg);
                    }
                    for arg in call.args.iter().skip(1) {
                        self.write(", ");
                        self.emit_expr(arg);
                    }
                    self.write(")");
                    return;
                }
                if !call.optional
                    && matches!(call.callee.kind, ExprKind::Ident(ref name) if name == "require")
                {
                    self.write("require(");
                    if let Some(arg) = call.args.first() {
                        self.emit_rewritten_relative_import_arg(arg);
                    }
                    for arg in call.args.iter().skip(1) {
                        self.write(", ");
                        self.emit_expr(arg);
                    }
                    self.write(")");
                    return;
                }
            }
        }

        // Check for const-enum access inlining before anything else.
        if self.try_emit_const_enum_ref(expr) {
            return;
        }

        let invalid_object_start = match &expr.kind {
            ExprKind::ObjectLit(_) => Some(expr.span.start),
            ExprKind::Member(member) if matches!(member.object.kind, ExprKind::ObjectLit(_)) => {
                Some(member.object.span.start)
            }
            _ => None,
        };
        if invalid_object_start
            .is_some_and(|start| self.emit_recovery_invalid_object_member_expressions(start))
        {
            return;
        }

        // Parser recovery for malformed unary expressions like `!@` should
        // keep the unary operator while dropping the error placeholder.
        if let ExprKind::Unary(un) = &expr.kind {
            if matches!(&un.argument.kind, ExprKind::Ident(name) if name == "<error>") {
                self.write(unary_op_str(un.op));
                return;
            }
        }

        // Generator callbacks retain catch binding identity ahead of argument
        // captures and module qualification. Ordinary expressions skip this lookup.
        if self.generator_catch_scope && matches!(expr.kind, ExprKind::Ident(_)) {
            if let Some(binding) = self.lexical_downlevel_plan.binding_for_reference(expr.span) {
                if binding.kind == lexical_downlevel::BindingKind::Catch {
                    let name = binding.emitted_name.clone();
                    self.record_mapping(expr.span);
                    self.write(&name);
                    return;
                }
            }
        }

        if let ExprKind::Ident(name) = &expr.kind {
            if name == "arguments" {
                if let Some(alias) = self.current_arguments_alias.clone() {
                    self.record_mapping(expr.span);
                    self.write(&alias);
                    return;
                }
            }
        }

        // Inside a namespace, qualify references to exported names with the
        // namespace parameter (e.g. `aa` → `NS.aa`).
        if let ExprKind::Ident(name) = &expr.kind {
            let qualify_target = self
                .export_target
                .as_ref()
                .filter(|t| *t != "exports")
                .cloned();
            if let Some(target) = qualify_target {
                let should_qualify = self.namespace_exports.contains(name.as_str())
                    && !self.cjs_param_shadows.contains(name.as_str());
                if should_qualify {
                    self.record_mapping(expr.span);
                    self.write(&target);
                    self.write(".");
                    self.write(name);
                    return;
                }
                // Check ancestor namespace exports (skip if locally shadowed).
                let ancestor_target = if self.ns_local_bindings.contains(name.as_str()) {
                    None
                } else {
                    self.ns_export_stack
                        .iter()
                        .rev()
                        .find(|(_, exports)| exports.contains(name.as_str()))
                        .map(|(t, _)| t.clone())
                };
                if let Some(anc_target) = ancestor_target {
                    self.record_mapping(expr.span);
                    self.write(&anc_target);
                    self.write(".");
                    self.write(name);
                    return;
                }
            }
        }

        // CJS export qualification: `x` → `exports.x`
        // Skip when the name is a parameter that shadows the export in the
        // current function scope.
        if let ExprKind::Ident(name) = &expr.kind {
            if self.export_target.as_ref().is_some_and(|t| t == "exports")
                && self.cjs_var_export_names.contains(name.as_str())
                && !self.cjs_param_shadows.contains(name.as_str())
            {
                self.record_mapping(expr.span);
                self.write_cjs_export_access("exports", name);
                return;
            }
        }

        // CJS import reference rewriting: `Calculator` → `file1_1.Calculator`
        // When `imported` is empty, this is a namespace/require import — use the var directly.
        // Skip when a local variable shadows the enclosing class name.
        if let ExprKind::Ident(name) = &expr.kind {
            let skip_for_class_shadow = self.class_name_locally_shadowed
                && self.current_class_name.as_deref() == Some(name.as_str());
            if !skip_for_class_shadow {
                if let Some((var_name, imported)) = self.cjs_import_map.get(name.as_str()).cloned()
                {
                    self.record_mapping(expr.span);
                    if imported.is_empty() {
                        self.write(&var_name);
                    } else {
                        self.write_cjs_import_access(&var_name, &imported, name);
                    }
                    return;
                }
            }
        }

        // CJS export call rewriting: `pick()` → `(0, exports.pick)()`
        if let ExprKind::Call(call) = &expr.kind {
            if let ExprKind::Ident(name) = &call.callee.kind {
                if self.export_target.as_ref().is_some_and(|t| t == "exports")
                    && self.cjs_var_export_names.contains(name.as_str())
                {
                    self.record_mapping(expr.span);
                    self.write("(0, ");
                    self.write_cjs_export_access("exports", name);
                    self.write(")(");
                    for (i, arg) in call.args.iter().enumerate() {
                        if i > 0 {
                            self.write(", ");
                        }
                        self.suppress_oc_parens = true;
                        self.emit_expr(arg);
                    }
                    self.write(")");
                    return;
                }
            }
        }

        // CJS import call rewriting: `test()` → `(0, file1_1.test)()`
        if let ExprKind::Call(call) = &expr.kind {
            if let ExprKind::Ident(name) = &call.callee.kind {
                if let Some((var_name, imported)) = self.cjs_import_map.get(name.as_str()).cloned()
                {
                    self.record_mapping(expr.span);
                    let system_direct_call = self.is_system() && self.rewrite_ident_with_import_map;
                    // Inside namespace IIFEs the cjs_import_map entries are
                    // namespace aliases (e.g. `b` → `c.b`), not actual module
                    // imports. Don't wrap those in `(0, ...)`.
                    let in_namespace = self.export_target.as_ref().is_some_and(|t| t != "exports");
                    if imported.is_empty() || system_direct_call || in_namespace {
                        // Namespace/direct call: just use the var directly
                        self.write(&var_name);
                        if !imported.is_empty() {
                            let access = Self::cjs_member_access_text(
                                &var_name,
                                &imported,
                                self.is_commonjs()
                                    && self.cjs_string_import_locals.contains(name.as_str()),
                            );
                            // The namespace/direct branch has already written the base.
                            self.write(&access[var_name.len()..]);
                        }
                    } else {
                        self.write("(0, ");
                        self.write_cjs_import_access(&var_name, &imported, name);
                        self.write(")");
                    }
                    self.write("(");
                    for (i, arg) in call.args.iter().enumerate() {
                        if i > 0 {
                            self.write(", ");
                        }
                        self.suppress_oc_parens = true;
                        self.emit_expr(arg);
                    }
                    self.write(")");
                    return;
                }
            }
        }

        // For expressions that need no transformation, copy source text verbatim.
        //
        // Pre-compute whether the source span contains a newline. Many of the
        // checks below (multiline call, multiline arrow, multiline cond, etc.)
        // only apply to multi-line expressions. By checking once, we can skip
        // ~15 individual checks for single-line expressions (the common case).
        let span_is_multiline = {
            let s = expr.span.start as usize;
            let e = expr.span.end as usize;
            s < e && e <= self.source.len() && self.source[s..e].contains('\n')
        };
        let has_cjs_ref =
            !self.cjs_import_map.is_empty() && expr_has_cjs_import_ref(expr, &self.cjs_import_map);
        // `import.meta.{dirname,filename,url}` has no runtime form in CJS;
        // block the source-copy fast path so the structured branch can rewrite.
        let has_cjs_import_meta_rewrite_expr =
            self.import_meta_needs_cjs_rewrite() && expr_has_cjs_import_meta_rewrite(expr);
        let has_const_ref = !self.const_enum_values.is_empty()
            && expr_has_const_enum_ref(expr, &self.const_enum_values);
        let has_missing = self.expr_has_missing_semis(expr);
        let has_error_marker_comment = self.expr_has_error_marker_comment(expr);
        let has_split_call_continuation = self.expr_has_split_call_continuation(expr);
        // A member operator that starts on a later line has formatter-owned
        // continuation indentation.  Copying the member span verbatim would
        // retain arbitrary source padding (and, when the surrounding
        // declaration is emitted structurally, add it to the output indent).
        // Keep this check tied to the Member AST node and its actual operator
        // gap so dots in strings and comments cannot affect it.
        let has_split_member_continuation = match &expr.kind {
            ExprKind::Member(member) => self
                .member_operator_positions(expr.span, member)
                .is_some_and(|(operator_start, _, _)| {
                    let object_end = member.object.span.end as usize;
                    object_end < operator_start
                        && self.source[object_end..operator_start]
                            .chars()
                            .any(|ch| ch == '\n' || ch == '\r')
                }),
            _ => false,
        };
        let has_missing_arg_commas = self.expr_has_missing_arg_commas(expr);
        let has_obj_lit_missing_commas = self.expr_has_obj_lit_missing_commas(expr);
        let has_array_lit_missing_commas = self.expr_has_array_lit_missing_commas(expr);
        let has_optional_shorthand = self.expr_has_optional_shorthand(expr);
        let has_empty_binary_right = {
            fn check_error_binary(e: &Expr) -> bool {
                match &e.kind {
                    ExprKind::Binary(bin) => {
                        matches!(&bin.right.kind, ExprKind::Ident(name) if name.is_empty() || name == "<error>")
                            || matches!(&bin.left.kind, ExprKind::Ident(name) if name.is_empty() || name == "<error>")
                            || check_error_binary(&bin.right)
                            || check_error_binary(&bin.left)
                    }
                    ExprKind::Assign(a) => check_error_binary(&a.right),
                    ExprKind::Paren(inner) => check_error_binary(inner),
                    _ => false,
                }
            }
            check_error_binary(expr)
        };
        let is_multiline_prefix_update = span_is_multiline
            && matches!(&expr.kind, ExprKind::Update(up)
                if matches!(up.op, UpdateOp::PreInc | UpdateOp::PreDec));
        let has_system_live_export_write_expr = self.is_system()
            && self.rewrite_ident_with_import_map
            && !self.system_live_export_names.is_empty()
            && expr_has_live_export_write(expr, &self.system_live_export_names);
        let has_cjs_export_ref = self.export_target.as_ref().is_some_and(|t| t == "exports")
            && !self.cjs_var_export_names.is_empty()
            && expr_has_cjs_export_ref(expr, &self.cjs_var_export_names);
        let has_cjs_live_export_write_expr = !self.suppress_cjs_live_export_wrap
            && !self.cjs_live_export_keys.is_empty()
            && expr_has_live_export_write(expr, &self.cjs_live_export_keys);
        let has_system_dynamic_import = self.is_system() && !self.system_context_fn.is_empty() && {
            let s = expr.span.start as usize;
            let e = expr.span.end as usize;
            s < e && e <= self.source.len() && self.source[s..e].contains("import(")
        };
        let has_rewrite_relative_import_call = self.rewrite_relative_import_extensions()
            && (self.expr_has_dynamic_import_call(expr) || {
                let s = expr.span.start as usize;
                let e = expr.span.end as usize;
                s < e && e <= self.source.len() && self.source[s..e].contains("require(")
            });
        let can_copy_source_span =
            expr.span.end > expr.span.start && (expr.span.end as usize) <= self.source.len();
        // Multi-line Call/New expressions must go through structured emit so
        // TypeScript's single-line call formatting is reproduced. Source-copy
        // would preserve the multi-line source layout.
        // Exception: if any argument spans multiple lines (e.g. multiline array/object),
        // TypeScript preserves the multi-line layout, so we should source-copy.
        let is_multiline_call =
            span_is_multiline && matches!(expr.kind, ExprKind::Call(_) | ExprKind::New(_)) && {
                let s = expr.span.start as usize;
                let e = expr.span.end as usize;
                if s < e && e <= self.source.len() {
                    // Check if any argument is itself multiline
                    let any_arg_multiline = match &expr.kind {
                        ExprKind::Call(c) => c.args.iter().any(|arg| {
                            let as_ = arg.span.start as usize;
                            let ae = arg.span.end as usize;
                            as_ < ae
                                && ae <= self.source.len()
                                && self.source[as_..ae].contains('\n')
                        }),
                        ExprKind::New(n) => n.args.as_ref().is_some_and(|args| {
                            args.iter().any(|arg| {
                                let as_ = arg.span.start as usize;
                                let ae = arg.span.end as usize;
                                as_ < ae
                                    && ae <= self.source.len()
                                    && self.source[as_..ae].contains('\n')
                            })
                        }),
                        _ => false,
                    };
                    // Also check for comments in the call args (TypeScript preserves those)
                    let has_comments =
                        self.source[s..e].contains("//") || self.source[s..e].contains("/*");
                    !any_arg_multiline && !has_comments
                } else {
                    false
                }
            };
        // Multi-line arrow expressions with expression bodies must go through
        // structured emit. TypeScript always inlines the expression body on the
        // same line as `=>`, but source-copy would preserve multi-line layout
        // from the source (e.g. `=> append(\n  arg1,\n  arg2\n)`).
        // Exception: when the source contains comments (// or /*), TypeScript
        // preserves the multi-line format with comments, so we should source-copy.
        let is_multiline_arrow_expr_body = if !span_is_multiline {
            false
        } else if let ExprKind::Arrow(a) = &expr.kind {
            let s = expr.span.start as usize;
            let e = expr.span.end as usize;
            if s < e && e <= self.source.len() {
                let source_text = &self.source[s..e];
                // Check for newline before `=>` (line terminator before
                // arrow token).  TypeScript always collapses these.
                if let Some(arrow_pos) = Self::find_fat_arrow_pos(source_text) {
                    if source_text[..arrow_pos].contains('\n') {
                        true
                    } else if matches!(a.body, ArrowBody::Expr(_)) {
                        !source_text.contains("//") && !source_text.contains("/*")
                    } else {
                        false
                    }
                } else if matches!(a.body, ArrowBody::Expr(_)) {
                    !source_text.contains("//") && !source_text.contains("/*")
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        };
        let is_multiline_array_with_elision = span_is_multiline
            && match &expr.kind {
                ExprKind::ArrayLit(elements) => {
                    let s = expr.span.start as usize;
                    let e = expr.span.end as usize;
                    s < e
                        && e <= self.source.len()
                        && array_literal_top_level_comma_count(self.source, expr.span)
                            > elements.len().saturating_sub(1)
                }
                _ => false,
            };
        let is_multiline_operator_expr =
            span_is_multiline && matches!(expr.kind, ExprKind::Binary(_) | ExprKind::Unary(_)) && {
                let s = expr.span.start as usize;
                let e = expr.span.end as usize;
                s < e && e <= self.source.len() && {
                    let src = &self.source[s..e];
                    !src.contains("//") && !src.contains("/*")
                }
            };
        let is_compact_multiline_binary_expr = span_is_multiline
            && matches!(expr.kind, ExprKind::Binary(_))
            && {
                let s = expr.span.start as usize;
                let e = expr.span.end as usize;
                s < e
                    && e <= self.source.len()
                    && {
                        let src = &self.source[s..e];
                        !src.contains("//")
                    && !src.contains("/*")
                    && src
                        .lines()
                        .skip(1)
                        .filter(|line| !line.trim().is_empty())
                        .all(|line| {
                            !crate::emit_stmt_helpers::line_starts_with_binary_operator(line)
                                && !crate::emit_stmt_helpers::line_is_binary_operator_only(line)
                        })
                    // Reject from compact path when the top-level binary has mixed
                    // operator precedences with the right operand on a new line.
                    // TypeScript re-indents based on precedence in these cases.
                    && !{
                        if let ExprKind::Binary(bin) = &expr.kind {
                            let left_end = bin.left.span.end as usize;
                            let right_start = bin.right.span.start as usize;
                            left_end < right_start
                                && right_start <= self.source.len()
                                && self.source[left_end..right_start].contains('\n')
                                && matches!(&bin.right.kind, ExprKind::Binary(inner) if inner.op != bin.op)
                        } else {
                            false
                        }
                    }
                    // Reject compact path when a continuation line starts with `/`
                    // immediately followed by an alphanumeric char (unspaced division
                    // that looks like a regex literal, e.g. `/notregexp/a.foo()`).
                    // These need structured emit to add proper operator spacing.
                    && !src.lines().skip(1).any(|line| {
                        let t = line.trim_start().as_bytes();
                        t.len() >= 2 && t[0] == b'/' && t[1].is_ascii_alphanumeric()
                    })
                    }
            };
        let array_has_object_literal_elem = matches!(
            &expr.kind,
            ExprKind::ArrayLit(elements)
                if elements
                    .iter()
                    .flatten()
                    .any(|el| matches!(el.kind, ExprKind::ObjectLit(_)))
        );
        let has_adjacent_spread_comment = self.span_has_adjacent_spread_comment(expr.span);
        // Empty argument lists with comments need structured emit so TypeScript's
        // formatting (space before comment, multi-line closing paren) is reproduced.
        let has_empty_args_comments = match &expr.kind {
            ExprKind::Call(c) if c.args.is_empty() => {
                let s = expr.span.start as usize;
                let e = expr.span.end as usize;
                if s < e && e <= self.source.len() {
                    let src = &self.source[s..e];
                    src.contains("/*") || src.contains("//")
                } else {
                    false
                }
            }
            ExprKind::New(n) if n.args.as_ref().is_some_and(|a| a.is_empty()) => {
                let s = expr.span.start as usize;
                let e = expr.span.end as usize;
                if s < e && e <= self.source.len() {
                    let src = &self.source[s..e];
                    src.contains("/*") || src.contains("//")
                } else {
                    false
                }
            }
            _ => false,
        };
        let has_newline_before_elem_bracket = {
            // Helper: check if an ElemAccess has a newline between object and bracket
            fn check_elem_access_newline(
                ea: &ElemAccessExpr,
                outer_span_end: u32,
                source: &str,
            ) -> bool {
                if !matches!(
                    ea.object.kind,
                    ExprKind::StrLit(_) | ExprKind::NoSubstTemplate(_) | ExprKind::NumLit(_)
                ) {
                    return false;
                }
                let obj_end = ea.object.span.end as usize;
                let expr_end = outer_span_end as usize;
                if obj_end < expr_end && expr_end <= source.len() {
                    let tail = &source[obj_end..expr_end];
                    if let Some(open_bracket) = tail.find('[') {
                        tail[..open_bracket].contains('\n')
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            match &expr.kind {
                ExprKind::ElemAccess(ea) => {
                    check_elem_access_newline(ea, expr.span.end, self.source)
                }
                // Also check through Assign where LHS is an ElemAccess
                ExprKind::Assign(assign) => {
                    if let ExprKind::ElemAccess(ea) = &assign.left.kind {
                        check_elem_access_newline(ea, expr.span.end, self.source)
                    } else {
                        false
                    }
                }
                _ => false,
            }
        };
        let has_space_before_elem_bracket = self.expr_has_space_before_elem_bracket(expr);
        let is_multiline_string_literal =
            span_is_multiline && matches!(&expr.kind, ExprKind::StrLit(_));
        let has_trailing_space_string_literal = matches!(&expr.kind, ExprKind::StrLit(_)) && {
            let s = expr.span.start as usize;
            let e = expr.span.end as usize;
            s < e
                && e <= self.source.len()
                && self.source[s..e]
                    .chars()
                    .last()
                    .is_some_and(|ch| ch == ' ' || ch == '\t')
        };
        let has_new_empty_array_call_callee_recovery = matches!(&expr.kind, ExprKind::New(new_expr) if {
            self.new_expr_has_empty_array_call_callee_recovery_shape(new_expr)
        });
        let has_error_placeholder_recovery_emit = self.file_has_recovery_errors
            && crate::emit_stmt_helpers::expr_has_error_member(expr)
            && matches!(expr.kind, ExprKind::Unary(_) | ExprKind::NonNull(_));
        let has_multiline_tagged_template_gap = self.expr_has_multiline_tagged_template_gap(expr);
        let is_recovered_arrow_source = self.arrow_needs_structured_recovery_emit(expr)
            || self.call_has_recovered_arrow_arg(expr);
        let is_multiline_cond_expr = span_is_multiline && matches!(&expr.kind, ExprKind::Cond(_));
        let has_member_inner_comments = self.expr_has_member_inner_comments(expr);
        // Arrow functions with `/* */` comments between `)` and `=>`
        // need structured emit so TypeScript strips those comments.
        let arrow_has_pre_arrow_comment = match &expr.kind {
            ExprKind::Arrow(a) if a.return_type.is_none() => {
                let s = expr.span.start as usize;
                let e = expr.span.end as usize;
                if s < e && e <= self.source.len() {
                    let src = &self.source[s..e];
                    if let Some(arrow_pos) = Self::find_fat_arrow_pos(src) {
                        // Check for `/*` between params close paren and `=>`
                        let before_arrow = &src[..arrow_pos];
                        if let Some(close_paren) = before_arrow.rfind(')') {
                            before_arrow[close_paren..].contains("/*")
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            _ => false,
        };
        let has_paren_inner_comments = match &expr.kind {
            ExprKind::Paren(inner) => {
                self.has_comments_in_range(expr.span.start + 1, inner.span.start)
                    || self.has_comments_in_range(inner.span.end, expr.span.end)
            }
            _ => false,
        };
        let is_error_ident =
            matches!(&expr.kind, ExprKind::Ident(name) if name == "<error>" || name.is_empty());
        let has_error_tagged_template_tag = matches!(
            &expr.kind,
            ExprKind::TaggedTemplate(tagged)
                if matches!(&tagged.tag.kind, ExprKind::Ident(name) if name == "<error>" || name.is_empty())
        );
        let can_copy_source_fast_path = !is_error_ident
            && !(self.current_arguments_alias.is_some() && expr_has_lexical_arguments(expr))
            && self.active_lexical_loop_helpers.is_empty()
            && !has_error_tagged_template_tag
            && !is_multiline_prefix_update
            && !expr_needs_transform(expr)
            && !self.expr_needs_downlevel(expr)
            && !self.disambig_wrap_leftmost_fn
            && !has_cjs_ref
            && !has_cjs_import_meta_rewrite_expr
            && !has_const_ref
            && !has_missing
            && !has_missing_arg_commas
            && !has_obj_lit_missing_commas
            && !has_array_lit_missing_commas
            && !has_optional_shorthand
            && !has_empty_binary_right
            && !has_error_marker_comment
            && !has_split_call_continuation
            && !has_split_member_continuation
            && !has_system_live_export_write_expr
            && !has_system_dynamic_import
            && !has_rewrite_relative_import_call
            && !has_cjs_live_export_write_expr
            && !has_cjs_export_ref
            && can_copy_source_span
            && !self.export_target.as_ref().is_some_and(|t| t != "exports")
            && !matches!(expr.kind, ExprKind::ObjectLit(_))
            && !Self::numlit_needs_normalize(expr)
            && !is_multiline_paren_comma(expr, self.source)
            && !new_has_error_callee(expr)
            && !self.expr_has_unclosed_delimiter(expr)
            && !is_multiline_call
            && !is_multiline_arrow_expr_body
            && !is_multiline_array_with_elision
            && !array_has_object_literal_elem
            && !has_adjacent_spread_comment
            && !has_empty_args_comments
            && !has_newline_before_elem_bracket
            && !has_space_before_elem_bracket
            && !is_multiline_string_literal
            && !has_trailing_space_string_literal
            && !has_new_empty_array_call_callee_recovery
            && !has_error_placeholder_recovery_emit
            && !has_multiline_tagged_template_gap
            && !is_recovered_arrow_source
            && !is_multiline_cond_expr
            && !expr_contains_class_expr(expr)
            && !has_member_inner_comments
            && !has_paren_inner_comments
            && !arrow_has_pre_arrow_comment
            && !self
                .lexical_downlevel_plan
                .has_renamed_reference_in(expr.span)
            // Inside function scopes, yield expressions with missing space before `(`
            // need structured emit: `yield(foo)` → `yield (foo)`.
            // Also delegate yields with no argument (`yield*`) need structured emit
            // to add space before `;`: `yield*;` → `yield* ;`.
            // Only force structured emit when the source text actually has the
            // spacing issue, to avoid breaking yield expressions with inline comments.
            && !(match &expr.kind {
                ExprKind::Yield(delegate, arg) => {
                    // Delegate yields: force structured emit for proper spacing.
                    // At top level (fn_scope_depth == 0): `yield * expr` / `yield * ;`
                    // Inside functions: `yield* expr` / `yield* ;`
                    // At top level, source-copy + normalizer would collapse `yield *`→`yield*`,
                    // so we force structured emit instead.
                    // Inside functions, delegate WITH arg keeps source-copy to preserve comments.
                    if *delegate && (arg.is_none() || self.fn_scope_depth == 0) {
                        true
                    } else if self.fn_scope_depth > 0 {
                        let s = expr.span.start as usize;
                        let e = expr.span.end as usize;
                        s < e && e <= self.source.len()
                            && self.source[s..e].starts_with("yield(")
                    } else {
                        false
                    }
                }
                _ => false,
            })
            && !(self.static_this_alias.is_some() && {
                let s = expr.span.start as usize;
                let e = expr.span.end as usize;
                s < e && e <= self.source.len() && self.source[s..e].contains("this")
            })
            && !(self.static_super_base_alias.is_some() && {
                let s = expr.span.start as usize;
                let e = expr.span.end as usize;
                s < e && e <= self.source.len() && self.source[s..e].contains("super")
            })
            && !(self.async_super_active && {
                let s = expr.span.start as usize;
                let e = expr.span.end as usize;
                s < e && e <= self.source.len() && self.source[s..e].contains("super")
            })
            && !(self.legacy_constructor_super.is_some() && {
                let s = expr.span.start as usize;
                let e = expr.span.end as usize;
                s < e
                    && e <= self.source.len()
                    && (self.source[s..e].contains("super")
                        || self.source[s..e].contains("this"))
            })
            // Disqualify source-copy when a keyword expression (await/yield) is
            // written with a Unicode escape (e.g. `\u0061wait`).  TypeScript's
            // structured emit writes the keyword literally; we must match that.
            && !(matches!(expr.kind, ExprKind::Await(_) | ExprKind::Yield(..)) && {
                let s = expr.span.start as usize;
                let e = expr.span.end as usize;
                s < e && e <= self.source.len() && self.source[s..e].starts_with("\\u")
            });
        if can_copy_source_fast_path
            && span_is_multiline
            && self.in_arrow_body_expr
            && matches!(expr.kind, ExprKind::Binary(_))
        {
            self.emit_compact_multiline_arrow_binary_expr(expr.span);
            return;
        }
        if can_copy_source_fast_path && is_compact_multiline_binary_expr {
            self.emit_compact_multiline_operator_expr(expr.span);
            return;
        }
        if can_copy_source_fast_path && !is_multiline_operator_expr {
            if span_is_multiline
                && self.in_arrow_body_expr
                && matches!(expr.kind, ExprKind::Binary(_))
            {
                self.emit_compact_multiline_arrow_binary_expr(expr.span);
                return;
            }
            if self.emit_compact_multiline_paren_assign_expr(expr) {
                return;
            } else if matches!(expr.kind, ExprKind::ArrayLit(_)) {
                let out_start = self.output.len();
                self.copy_expr_span(expr.span);
                // Only break `, ,` onto separate lines if the source array
                // was multiline.  Single-line sparse arrays like `[1, 2, ,]`
                // should stay on one line. Avoid materializing the slice into
                // a String unless we actually need to rewrite it.
                let needs_rewrite = {
                    let slice = &self.output[out_start..];
                    slice.contains(", ,") && slice.contains('\n')
                };
                if needs_rewrite {
                    let inner_indent = "    ".repeat((self.indent + 1) as usize);
                    let fixed =
                        self.output[out_start..].replace(", ,", &format!(",\n{},", inner_indent));
                    self.output.truncate(out_start);
                    self.output.push_str(&fixed);
                }
            } else {
                self.copy_expr_span(expr.span);
            }
            return;
        }

        // Record source map mapping for transformed expression start.
        self.record_mapping(expr.span);

        match &expr.kind {
            ExprKind::Ident(name) => {
                // Skip parser-recovery error placeholders.
                if name == "<error>" {
                    return;
                }
                // When a local variable shadows the enclosing class name,
                // skip both cjs_import_map and static alias substitution.
                if let Some(renamed) = self
                    .lexical_downlevel_plan
                    .emitted_name_for_reference(expr.span)
                    .map(str::to_owned)
                {
                    self.write(&renamed);
                } else if self.class_name_locally_shadowed
                    && self.current_class_name.as_deref() == Some(name.as_str())
                {
                    self.write(name);
                } else if self.rewrite_ident_with_import_map
                    && self.cjs_import_map.contains_key(name.as_str())
                {
                    self.emit_value_name_ref(name);
                } else if self.current_class_static_alias.is_some()
                    && self.current_class_name.as_deref() == Some(name.as_str())
                {
                    // Inside a class with a static alias, replace class name references
                    // with the alias (e.g. `ClassName` → `_a`).
                    let alias = self.current_class_static_alias.clone().unwrap();
                    self.write(&alias);
                } else {
                    self.write(name);
                }
            }
            ExprKind::NumLit(n) => {
                self.write(&Self::normalize_numeric_literal(n));
            }
            ExprKind::BigIntLit(n) => {
                // TypeScript lowercases hex digits in bigint literals: 0xFFFFn → 0xffffn
                // and converts binary/octal to decimal. We lowercase here; binary/octal
                // conversion happens when the expression goes through structured emit.
                self.write(&Self::normalize_bigint_literal(n));
            }
            ExprKind::StrLit(s) => {
                // JSX multiline quoted attributes can produce raw spans with
                // physical newlines (invalid in JS string literals). Re-escape
                // those while preserving the original quote style.
                let span_start = expr.span.start as usize;
                let span_end = expr.span.end as usize;
                if expr.span == Span::new(0, 0) {
                    self.write("\"");
                    self.write(&escape_js_string_for_quote(s, '"'));
                    self.write("\"");
                    return;
                }
                if span_start < span_end && span_end <= self.source.len() {
                    let raw = &self.source[span_start..span_end];
                    if raw.contains('\n') || raw.contains('\r') {
                        let quote = raw
                            .as_bytes()
                            .first()
                            .copied()
                            .filter(|q| *q == b'"' || *q == b'\'')
                            .map(char::from)
                            .unwrap_or('"');
                        self.write(if quote == '\'' { "'" } else { "\"" });
                        self.write(&escape_js_string_for_quote(s, quote));
                        self.write(if quote == '\'' { "'" } else { "\"" });
                        return;
                    }
                    if self.effective_target() < ScriptTarget::ES2015 && raw.contains("\\u{") {
                        let quote = raw
                            .as_bytes()
                            .first()
                            .copied()
                            .filter(|q| *q == b'"' || *q == b'\'')
                            .map(char::from)
                            .unwrap_or('"');
                        if let Some(downleveled) =
                            downlevel_braced_unicode_escapes_in_string(raw, quote)
                        {
                            self.write(&downleveled);
                            return;
                        }
                    }
                }
                // Use span to preserve original quote style in normal cases.
                self.copy_span(expr.span);
            }
            ExprKind::BoolLit(v) => self.write(if *v { "true" } else { "false" }),
            ExprKind::NullLit => self.write("null"),
            ExprKind::NoSubstTemplate(s) => {
                if self.effective_target() < ScriptTarget::ES2015 {
                    self.write("\"");
                    self.write(&downlevel_template_string(s));
                    self.write("\"");
                } else {
                    self.copy_span(expr.span);
                }
            }
            ExprKind::Template(tpl) => self.emit_template(tpl),
            ExprKind::TaggedTemplate(tagged) => {
                // In JavaScript files, `tag<T>\`...\`` is a pair of binary
                // comparisons, not a TypeScript tagged template with type
                // arguments. Handle it before tagged-call receiver rewriting
                // and lower only the template operand for pre-ES2015 targets.
                // Otherwise ES5 emit would turn a comparison into a call and
                // evaluate `tag`.
                if self.is_js_file && tagged.type_args.is_some() {
                    self.emit_expr(&tagged.tag);
                    if let Some(type_args) = &tagged.type_args {
                        self.emit_js_type_args_as_binary(type_args);
                    }
                    self.emit_template(&tagged.quasi);
                    return;
                }

                // CJS import rewriting for tagged template tags:
                // `css` `` `...` `` → `(0, react_1.css)` `` `...` ``
                // Tagged templates need the `(0, ...)` wrapper (like function
                // calls) to avoid binding `this` to the module object.
                let cjs_tag = if let ExprKind::Ident(name) = &tagged.tag.kind {
                    self.cjs_import_map.get(name.as_str()).cloned()
                } else {
                    None
                };
                let cjs_tag_string = if let ExprKind::Ident(name) = &tagged.tag.kind {
                    self.cjs_string_import_locals.contains(name.as_str())
                } else {
                    false
                };
                if let Some((var_name, imported)) = cjs_tag {
                    if !imported.is_empty() {
                        let in_namespace =
                            self.export_target.as_ref().is_some_and(|t| t != "exports");
                        if in_namespace {
                            let access = Self::cjs_member_access_text(
                                &var_name,
                                &imported,
                                self.is_commonjs() && cjs_tag_string,
                            );
                            self.write(&access);
                        } else {
                            self.write("(0, ");
                            let access = Self::cjs_member_access_text(
                                &var_name,
                                &imported,
                                self.is_commonjs() && cjs_tag_string,
                            );
                            self.write(&access);
                            self.write(")");
                        }
                    } else {
                        self.write(&var_name);
                    }
                } else if let ExprKind::Member(mem) = &tagged.tag.kind {
                    if mem.optional && mem.property == "<error>" {
                        // `a?.\`b\`` — optional chain on tagged template.
                        // TypeScript strips the `?.` and emits just the tag object.
                        self.emit_expr(&mem.object);
                    } else if self.needs_downlevel("private-fields")
                        && mem.property.starts_with('#')
                    {
                        // Private field tagged template: this.#field`...`
                        // → __classPrivateFieldGet(receiver, _C_field, "f").bind(receiver) `...`
                        // For non-simple receivers, need a temp variable.
                        let field_name = normalize_unicode_escapes(&mem.property[1..]);
                        let is_simple_receiver =
                            matches!(&mem.object.kind, ExprKind::This | ExprKind::Ident(_));
                        // Determine kind and variable name
                        let private_info: Option<(String, &'static str, Option<String>)> = if self
                            .current_class_private_fields
                            .contains(field_name.as_str())
                        {
                            let field_var = if let Some(ref cn) = self.current_class_name {
                                let norm_cn = normalize_unicode_escapes(cn);
                                format!("_{}_{}", norm_cn, field_name)
                            } else {
                                format!("_{}", field_name)
                            };
                            let is_static = self
                                .current_class_static_private_fields
                                .contains(field_name.as_str());
                            if is_static {
                                // Static field: 4-arg form with class alias as brand
                                let alias = self.current_class_static_alias_or_default();
                                Some((alias, "\"f\"", Some(field_var)))
                            } else {
                                Some((field_var, "\"f\"", None))
                            }
                        } else if let Some(method_var) = self
                            .current_class_private_methods
                            .get(field_name.as_str())
                            .cloned()
                        {
                            let brand_var = self.current_class_instances_var();
                            Some((brand_var, "\"m\"", Some(method_var.to_string())))
                        } else if let Some((getter_var, _)) = self
                            .current_class_private_accessors
                            .get(field_name.as_str())
                            .cloned()
                        {
                            let is_static = self
                                .current_class_static_private_accessors
                                .contains(field_name.as_str());
                            if is_static {
                                let alias = self.current_class_static_alias_or_default();
                                Some((alias, "\"a\"", getter_var.map(|v| v.to_string())))
                            } else {
                                let brand_var = self.current_class_instances_var();
                                Some((brand_var, "\"a\"", getter_var.map(|v| v.to_string())))
                            }
                        } else if let Some(method_var) = self
                            .current_class_static_private_methods
                            .get(field_name.as_str())
                            .cloned()
                        {
                            // Static private method: use class alias as brand
                            let alias = self.current_class_static_alias_or_default();
                            Some((alias, "\"m\"", Some(method_var.to_string())))
                        } else {
                            None
                        };
                        if let Some((var_name, kind_str, method_fn)) = private_info {
                            self.needs_private_field_get = true;
                            if is_simple_receiver {
                                self.write(self.helper_prefix());
                                self.write("__classPrivateFieldGet(");
                                self.emit_expr(&mem.object);
                                self.write(", ");
                                self.write(&var_name);
                                self.write(", ");
                                self.write(&kind_str);
                                if let Some(ref mf) = method_fn {
                                    self.write(", ");
                                    self.write(mf);
                                }
                                self.write(").bind(");
                                self.emit_expr(&mem.object);
                                self.write(")");
                            } else {
                                // Complex receiver: __classPrivateFieldGet((_a = expr), ...).bind(_a)
                                if kind_str == "\"f\"" {
                                    self.split_multiline_function_body_temp_decls = true;
                                }
                                let temp = self.next_temp_var();
                                self.write(self.helper_prefix());
                                self.write("__classPrivateFieldGet((");
                                self.write(&temp);
                                self.write(" = ");
                                self.emit_expr(&mem.object);
                                self.write("), ");
                                self.write(&var_name);
                                self.write(", ");
                                self.write(&kind_str);
                                if let Some(ref mf) = method_fn {
                                    self.write(", ");
                                    self.write(mf);
                                }
                                self.write(").bind(");
                                self.write(&temp);
                                self.write(")");
                            }
                        } else {
                            self.emit_expr(&tagged.tag);
                        }
                    } else if !mem.optional
                        && matches!(&mem.object.kind, ExprKind::Super)
                        && self.static_super_base_alias.is_some()
                    {
                        // super.method`...` → Reflect.get(_classSuper, "method", _classThis).bind(_classThis) `...`
                        let recv = self.static_super_receiver_alias.clone().unwrap_or_default();
                        self.emit_expr(&tagged.tag);
                        self.write(".bind(");
                        self.write(&recv);
                        self.write(")");
                    } else {
                        self.emit_expr(&tagged.tag);
                    }
                } else if let ExprKind::ElemAccess(ea) = &tagged.tag.kind {
                    if !ea.optional
                        && matches!(&ea.object.kind, ExprKind::Super)
                        && self.static_super_base_alias.is_some()
                    {
                        // super["method"]`...` / super[expr]`...` → Reflect.get(...).bind(_classThis) `...`
                        let recv = self.static_super_receiver_alias.clone().unwrap_or_default();
                        self.emit_expr(&tagged.tag);
                        self.write(".bind(");
                        self.write(&recv);
                        self.write(")");
                    } else {
                        self.emit_expr(&tagged.tag);
                    }
                } else {
                    self.emit_expr(&tagged.tag);
                }
                // ES5 and earlier cannot represent tagged template syntax at
                // all. ES2015-ES2017 only need the helper for invalid escapes.
                let needs_lowering = self.effective_target() < ScriptTarget::ES2015
                    || (self.needs_downlevel("tagged-template")
                        && crate::emit_stmt_helpers::tagged_template_needs_lowering(&tagged.quasi));

                if needs_lowering {
                    self.needs_make_template_object_helper = true;
                    // Emit: tag(__makeTemplateObject([cooked], [raw]), expr1, ...)
                    self.write("(");
                    let prefix = self.helper_prefix().to_string();
                    self.write(&prefix);
                    self.write("__makeTemplateObject(");
                    // Cooked array
                    self.write("[");
                    for (i, q) in tagged.quasi.quasis.iter().enumerate() {
                        if i > 0 {
                            self.write(", ");
                        }
                        if let Some(cooked) = compute_template_cooked(&q.raw) {
                            self.write("\"");
                            self.write(&cooked);
                            self.write("\"");
                        } else {
                            self.write("void 0");
                        }
                    }
                    self.write("], [");
                    // Raw array
                    for (i, q) in tagged.quasi.quasis.iter().enumerate() {
                        if i > 0 {
                            self.write(",");
                            if let Some(previous_expr) = tagged.quasi.exprs.get(i - 1) {
                                self.emit_tagged_template_raw_boundary_comments(
                                    previous_expr.span.end,
                                    q.span.start,
                                    false,
                                );
                            } else {
                                self.write(" ");
                            }
                        }
                        self.write("\"");
                        self.write(&escape_template_raw_for_js_string(&q.raw));
                        self.write("\"");
                        if let Some(next_expr) = tagged.quasi.exprs.get(i) {
                            self.emit_tagged_template_raw_boundary_comments(
                                q.span.end,
                                next_expr.span.start,
                                true,
                            );
                        }
                    }
                    self.write("])");
                    // Expressions become additional arguments
                    for (i, e) in tagged.quasi.exprs.iter().enumerate() {
                        self.write(", ");
                        let leading_start = tagged
                            .quasi
                            .quasis
                            .get(i)
                            .map_or(e.span.start, |quasi| quasi.span.end);
                        self.emit_template_comments_in_range(leading_start, e.span.start);
                        self.emit_expr(e);
                        if let Some(quasi) = tagged.quasi.quasis.get(i + 1) {
                            self.emit_template_comments_in_range(e.span.end, quasi.span.start);
                        }
                    }
                    self.write(")");
                } else {
                    self.write(" ");
                    // This is still a tagged template. Emit its outer syntax
                    // directly so it cannot be mistaken for an untagged
                    // template expression.
                    self.emit_template_syntax(&tagged.quasi);
                }
            }
            ExprKind::RegexpLit(_) => {
                self.copy_span(expr.span);
            }
            ExprKind::ArrayLit(elements) => {
                if self
                    .lexical_downlevel_plan
                    .array_spreads
                    .contains_key(&expr.span.into())
                {
                    let lowered = self.lower_array_spread(elements);
                    self.emit_expr(&lowered);
                    return;
                }
                // When array elements span multiple lines in source,
                // TypeScript adds one indent level so the closing `}]`
                // aligns at the initializer context level.
                let is_multiline = {
                    let s = expr.span.start as usize;
                    let e = expr.span.end as usize;
                    let span_has_newline =
                        s < e && e <= self.source.len() && self.source[s..e].contains('\n');
                    let element_span_has_newline = elements.iter().flatten().any(|elem| {
                        let es = elem.span.start as usize;
                        let ee = elem.span.end as usize;
                        es < ee && ee <= self.source.len() && self.source[es..ee].contains('\n')
                    });
                    // Recovery spans can miss trailing lines of a multiline array
                    // (`[{ ... }]`). If a top-level object literal element would
                    // emit as multiline, treat the array as multiline too.
                    let object_elem_forces_multiline = elements.iter().flatten().any(|elem| {
                        let ExprKind::ObjectLit(props) = &elem.kind else {
                            return false;
                        };
                        if props.is_empty() {
                            return false;
                        }
                        let es = elem.span.start as usize;
                        let ee = elem.span.end as usize;
                        !(es < ee && ee <= self.source.len() && !self.source[es..ee].contains('\n'))
                    });
                    span_has_newline || element_span_has_newline || object_elem_forces_multiline
                };
                let source_comma_count =
                    array_literal_top_level_comma_count(self.source, expr.span);
                let base_separator_count = elements.len().saturating_sub(1);
                let is_elision_slot = |slot: &Option<Box<Expr>>| {
                    slot.is_none()
                        || slot
                            .as_ref()
                            .is_some_and(|e| matches!(e.kind, ExprKind::Omitted))
                };
                let has_elision = elements.iter().any(is_elision_slot)
                    || source_comma_count > base_separator_count;
                let preserve_close_on_next_line = is_multiline
                    && !has_elision
                    && elements.len() == 1
                    && matches!(
                        elements.first().and_then(|e| e.as_ref()).map(|e| &e.kind),
                        Some(ExprKind::Spread(_))
                    )
                    && self.span_has_adjacent_spread_comment_linebreak(expr.span);
                let array_out_start = self.output.len();
                self.write("[");
                if is_multiline && has_elision {
                    // Recovery/pretty-print path for multiline arrays with holes.
                    // Emit one logical slot per line so elisions are preserved as
                    // standalone commas (`[, ,]` → line-separated commas).
                    self.indent += 1;
                    self.newline();
                    for (i, elem) in elements.iter().enumerate() {
                        if let Some(ref e) = elem {
                            self.emit_leading_comments(e.span.start);
                            self.suppress_oc_parens = true;
                            self.emit_expr(e);
                        }
                        let emit_comma = i + 1 < elements.len();
                        if emit_comma {
                            self.write(",");
                        }
                        if i + 1 < elements.len() && !self.at_line_start {
                            self.newline();
                        }
                    }
                    let extra_commas = source_comma_count.saturating_sub(base_separator_count);
                    // When there are extra commas (trailing commas), check if
                    // the last element is a real (non-elision) element.  If so,
                    // append the first trailing comma on the same line as the
                    // last element instead of on a new line, so we get:
                    //   lastElem,      (not: lastElem\n    ,)
                    let last_is_real = elements.last().is_some_and(|slot| {
                        slot.is_some()
                            && !slot
                                .as_ref()
                                .is_some_and(|e| matches!(e.kind, ExprKind::Omitted))
                    });
                    for idx in 0..extra_commas {
                        if idx == 0 && last_is_real {
                            // Append trailing comma on same line as last element
                            self.write(",");
                        } else {
                            if !self.at_line_start {
                                self.newline();
                            }
                            self.write(",");
                        }
                    }
                    let close_pos = expr.span.end;
                    let out_before_comments = self.output.len();
                    let next_comment_is_line_start = if self.next_comment_idx < self.comments.len()
                        && self.comments[self.next_comment_idx].pos < close_pos
                    {
                        let c_start = self.comments[self.next_comment_idx].pos as usize;
                        if c_start <= self.source.len() {
                            let line_start = if c_start == 0 {
                                0
                            } else {
                                self.source[..c_start]
                                    .rfind('\n')
                                    .map(|p| p + 1)
                                    .unwrap_or(0)
                            };
                            self.source[line_start..c_start].trim().is_empty()
                        } else {
                            false
                        }
                    } else {
                        false
                    };
                    if next_comment_is_line_start && !self.at_line_start {
                        self.newline();
                    }
                    self.emit_leading_comments(close_pos);
                    if self.output.len() == out_before_comments {
                        if let Some((comment, on_new_line)) =
                            multiline_array_elision_tail_comment(self.source, expr.span)
                        {
                            if on_new_line {
                                if !self.at_line_start {
                                    self.newline();
                                }
                            } else if !self.at_line_start {
                                self.write(" ");
                            }
                            self.write(&comment);
                        }
                    }
                    self.advance_comment_pos(close_pos);
                    self.indent -= 1;
                    if !self.at_line_start {
                        self.newline();
                    }
                    self.write("]");
                    return;
                }
                // Check if the first element starts on the same line as `[` in
                // the source.  TypeScript keeps the first element on the same
                // line as `[` when the source has it that way (e.g.,
                // `[function () {` stays on one line).
                let first_on_same_line = is_multiline
                    && elements
                        .first()
                        .and_then(|e| e.as_ref())
                        .is_some_and(|first_elem| {
                            let bracket_pos = expr.span.start as usize;
                            let first_pos = first_elem.span.start as usize;
                            bracket_pos < first_pos
                                && first_pos <= self.source.len()
                                && !self.source[bracket_pos..first_pos].contains('\n')
                        });
                if is_multiline {
                    self.indent += 1;
                    if !first_on_same_line {
                        self.newline();
                    }
                }
                let has_trailing_comma_in_source = {
                    let s = expr.span.start as usize;
                    let e = expr.span.end as usize;
                    if s < e && e <= self.source.len() {
                        let text = &self.source[s..e];
                        // Check if there's a comma immediately before `]`
                        // (with only whitespace between).
                        if let Some(close_idx) = text.rfind(']') {
                            let before = text[..close_idx].trim_end();
                            before.ends_with(',')
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                };
                let has_trailing_elision = !is_multiline
                    && (elements.last().is_some_and(|slot| is_elision_slot(slot))
                        || has_trailing_comma_in_source);
                for (i, elem) in elements.iter().enumerate() {
                    if i > 0 {
                        self.write(",");
                        if is_multiline {
                            // Check if the next element is on the same line as
                            // the previous element's end in the source.  When
                            // `}, {` appears on one line, keep them together.
                            let prev_end = elements[i - 1]
                                .as_ref()
                                .map(|e| e.span.end as usize)
                                .unwrap_or(0);
                            let next_start =
                                elem.as_ref().map(|e| e.span.start as usize).unwrap_or(0);
                            let same_line = prev_end > 0
                                && next_start > prev_end
                                && next_start <= self.source.len()
                                && !self.source[prev_end..next_start].contains('\n');
                            if same_line {
                                self.write(" ");
                            } else {
                                self.newline();
                            }
                        } else {
                            self.write(" ");
                        }
                    }
                    if let Some(ref e) = elem {
                        if is_multiline {
                            self.emit_leading_comments(e.span.start);
                        }
                        let needs_array_class_indent = !is_multiline
                            && matches!(
                                &e.kind,
                                ExprKind::ClassExpr(cd)
                                    if cd.name.is_none()
                                        && (cd.members.is_empty()
                                            || class_has_static_initializers(cd)
                                            || !cd.decorators.is_empty()
                                            || class_has_member_decorators(cd))
                            );
                        if needs_array_class_indent {
                            self.indent += 1;
                        }
                        self.suppress_oc_parens = true;
                        if self.emitting_destructuring_assignment_pattern {
                            self.emit_destructuring_assignment_target(e);
                        } else {
                            self.emit_expr(e);
                        }
                        if needs_array_class_indent {
                            self.indent -= 1;
                        }
                    }
                }
                // When the last element is an elision (None) on a single-line
                // array, TypeScript emits a trailing comma to preserve length.
                if has_trailing_elision {
                    self.write(",");
                    // Emit any inline block comments between the trailing
                    // comma and `]`.
                    let close_pos = expr.span.end;
                    let has_inline_comment = self.next_comment_idx < self.comments.len()
                        && self.comments[self.next_comment_idx].pos < close_pos
                        && self.comments[self.next_comment_idx].is_multiline;
                    if has_inline_comment {
                        self.write(" ");
                        self.emit_leading_comments(close_pos);
                        // Remove trailing space added by emit_leading_comments
                        // since `]` follows immediately.
                        if self.output.ends_with(' ') {
                            self.output.pop();
                        }
                    }
                }
                if is_multiline {
                    self.indent -= 1;
                    // When the first element was on the same line as `[`,
                    // TypeScript also puts `]` on the same line as the last
                    // element's closing (e.g., `}]`).  Don't emit a newline
                    // UNLESS the source has `]` on its own line.
                    let source_close_on_own_line = first_on_same_line && {
                        let s = expr.span.start as usize;
                        let e = expr.span.end as usize;
                        if s < e && e <= self.source.len() {
                            let text = &self.source[s..e];
                            if let Some(close_idx) = text.rfind(']') {
                                // Check if there's a newline between the last
                                // non-whitespace content and `]`.
                                let before = &text[..close_idx];
                                before.trim_end().ends_with('\n')
                                    || before
                                        .rfind(|c: char| !c.is_whitespace())
                                        .map(|p| before[p + 1..close_idx].contains('\n'))
                                        .unwrap_or(false)
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    };
                    if (!first_on_same_line || source_close_on_own_line) && !self.at_line_start {
                        self.newline();
                    }
                }
                if preserve_close_on_next_line && !self.at_line_start {
                    self.newline();
                }
                self.write("]");
                if is_multiline {
                    let segment = self.output[array_out_start..].to_string();
                    if segment.contains(", ,") {
                        let inner_indent = "    ".repeat((self.indent + 1) as usize);
                        let fixed = segment.replace(", ,", &format!(",\n{},", inner_indent));
                        self.output.truncate(array_out_start);
                        self.output.push_str(&fixed);
                    }
                }
            }
            ExprKind::ObjectLit(props) => {
                // Filter out error-recovery methods whose body was parsed
                // without a `{` in the source (e.g. `{ foo(); }` where `;`
                // appears instead of a block body). TypeScript strips these
                // members from the emit output.
                let filtered_storage;
                let props = if props.iter().any(|p| self.is_error_recovery_obj_method(p)) {
                    filtered_storage = props
                        .iter()
                        .filter(|p| !self.is_error_recovery_obj_method(p))
                        .cloned()
                        .collect::<Vec<_>>();
                    &filtered_storage
                } else {
                    props
                };
                let has_spread = props.iter().any(|p| matches!(p, ObjLitProp::Spread(_, _)));
                if self.try_emit_computed_object_literal_downlevel(props, expr) {
                    return;
                } else if has_spread && self.needs_downlevel("object-spread") {
                    self.emit_object_spread_downlevel(props, expr.span.end);
                } else {
                    if props.is_empty() {
                        // Check if the empty object has comments inside braces.
                        // TypeScript preserves comments (e.g. `{ // i: '' }`) as
                        // multi-line objects even when there are no properties.
                        let start = expr.span.start as usize;
                        let end = expr.span.end as usize;
                        let has_inner_comment = if start < end && end <= self.source.len() {
                            let src = &self.source[start..end];
                            src.contains("//") || {
                                // Check for /* ... */ but not the empty `{}`
                                if let Some(open) = src.find('{') {
                                    let inner = &src[open + 1..];
                                    inner.contains("/*")
                                } else {
                                    false
                                }
                            }
                        } else {
                            false
                        };
                        if has_inner_comment && self.options.remove_comments != Some(true) {
                            // Emit as multi-line with comments preserved
                            let src = &self.source[start..end];
                            let open = src.find('{').unwrap_or(0);
                            let close = src.rfind('}').unwrap_or(src.len());
                            let between = &src[open + 1..close];
                            self.writeln("{");
                            // Extract and emit comments from between braces
                            for line in between.lines() {
                                let trimmed = line.trim();
                                if !trimmed.is_empty() {
                                    self.write(trimmed);
                                    self.newline();
                                }
                            }
                            self.write("}");
                            // Advance comment tracking past the object literal's span
                            // so these comments aren't re-emitted by the general system.
                            let span_end = expr.span.end;
                            while self.next_comment_idx < self.comments.len()
                                && self.comments[self.next_comment_idx].pos < span_end
                            {
                                self.next_comment_idx += 1;
                            }
                            self.comment_emit_pos = self.comment_emit_pos.max(span_end);
                        } else {
                            self.write("{}");
                        }
                        return;
                    }
                    // Check if source was single-line.
                    let start = expr.span.start as usize;
                    let end = expr.span.end as usize;
                    let source_single_line = start < end
                        && end <= self.source.len()
                        && !self.source[start..end].contains('\n');
                    let inline_single_method_opener = !source_single_line
                        && props.len() == 1
                        && matches!(
                            props.first(),
                            Some(ObjLitProp::Method(_) | ObjLitProp::Get(_) | ObjLitProp::Set(_))
                        )
                        && {
                            let first_start = obj_lit_prop_span(&props[0]).start as usize;
                            start < first_start
                                && first_start <= self.source.len()
                                && !self.source[start..first_start].contains('\n')
                        };
                    // Detect trailing comma after last property in source
                    let has_trailing_comma =
                        if !props.is_empty() && end > 0 && end <= self.source.len() {
                            let last_prop = &props[props.len() - 1];
                            let lp_end = match last_prop {
                                ObjLitProp::Property(p) => p.span.end as usize,
                                ObjLitProp::Method(m) => m.span.end as usize,
                                ObjLitProp::Get(a) | ObjLitProp::Set(a) => a.span.end as usize,
                                ObjLitProp::Spread(_, sp) => sp.end as usize,
                                ObjLitProp::Shorthand(_, sp) => sp.end as usize,
                                ObjLitProp::ShorthandDefault(_, _, sp) => sp.end as usize,
                            };
                            if lp_end > 0 && lp_end < end {
                                let between = &self.source[lp_end..end];
                                // Only look for commas outside of comments
                                let code = if let Some(idx) = between.find("//") {
                                    &between[..idx]
                                } else {
                                    between
                                };
                                code.contains(',')
                            } else {
                                false
                            }
                        } else {
                            false
                        };
                    // For error-recovered objects with keyword shorthand
                    // properties, use hybrid format: first property inline
                    // with `{`, continuation indented. Matches TypeScript's
                    // recovery: `{ a: 1\nreturn; }` → `{ a: 1,\n    return:  };`
                    let has_keyword_shorthand_prop = props.iter().any(|p| {
                        matches!(p, ObjLitProp::Shorthand(name, _)
                            if crate::emit_stmt::is_js_reserved_keyword(name))
                    });
                    let force_hybrid = !source_single_line
                        && has_keyword_shorthand_prop
                        && props.len() <= 4
                        && props.len() >= 2;
                    if force_hybrid {
                        self.write("{ ");
                        self.indent += 1;
                        self.emit_obj_lit_prop(&props[0]);
                        self.write(",");
                        self.newline();
                        for (i, prop) in props[1..].iter().enumerate() {
                            self.emit_obj_lit_prop(prop);
                            if i < props.len() - 2 {
                                self.write(",");
                                self.newline();
                            }
                        }
                        self.indent -= 1;
                        self.write(" }");
                    } else if source_single_line {
                        self.write("{ ");
                        self.indent += 1;
                        for (i, prop) in props.iter().enumerate() {
                            self.emit_obj_lit_prop(prop);
                            if i < props.len() - 1 {
                                self.write(", ");
                            } else if has_trailing_comma {
                                self.write(",");
                            }
                        }
                        self.indent -= 1;
                        self.write(" }");
                    } else {
                        if inline_single_method_opener {
                            self.write("{ ");
                        } else {
                            self.writeln("{");
                        }
                        self.indent += 1;
                        for (i, prop) in props.iter().enumerate() {
                            let prop_span = obj_lit_prop_span(prop);
                            let next_prop_start = if i + 1 < props.len() {
                                obj_lit_prop_span(&props[i + 1]).start
                            } else {
                                expr.span.end
                            };
                            self.emit_leading_comments(prop_span.start);
                            self.emit_obj_lit_prop(prop);
                            let needs_comma = i < props.len() - 1 || has_trailing_comma;
                            let trailing_multiline_comment = self.comments[self.next_comment_idx..]
                                .iter()
                                .find(|c| {
                                    c.is_multiline
                                        && c.pos >= prop_span.end
                                        && c.pos < next_prop_start
                                        && {
                                            let ps = prop_span.end as usize;
                                            let cs = c.pos as usize;
                                            ps <= cs
                                                && cs <= self.source.len()
                                                && !self.source[ps..cs]
                                                    .chars()
                                                    .any(|ch| ch == '\n' || ch == '\r')
                                        }
                                })
                                .and_then(|c| {
                                    let cs = c.pos as usize;
                                    let ce = c.end as usize;
                                    if cs < ce && ce <= self.source.len() {
                                        Some((
                                            self.source[cs..ce].trim_end_matches('\r').to_string(),
                                            c.end,
                                        ))
                                    } else {
                                        None
                                    }
                                });
                            // Check if a block comment appears before the
                            // comma in the source (e.g. `'text' /*c*/,`).
                            // In that case, emit the comment before the
                            // comma. Otherwise use the normal comma-first
                            // order (e.g. `"b", // comment`).
                            let comment_before_comma = needs_comma
                                && self.trailing_block_comment_before_comma(prop_span.end);
                            if comment_before_comma {
                                self.newline();
                                self.append_trailing_comment(prop_span);
                                // Insert comma before the trailing \n
                                if self.output.ends_with('\n') {
                                    self.output.pop();
                                    self.output.push(',');
                                    self.output.push('\n');
                                }
                            } else if needs_comma && trailing_multiline_comment.is_some() {
                                let (comment_text, comment_end) =
                                    trailing_multiline_comment.unwrap();
                                self.write(", ");
                                self.write(&comment_text);
                                self.newline();
                                self.advance_comment_pos(comment_end);
                            } else if needs_comma {
                                self.writeln(",");
                                self.append_trailing_comment(prop_span);
                            } else {
                                self.newline();
                                self.append_trailing_comment(prop_span);
                            }
                            self.advance_comment_pos(prop_span.end);
                        }
                        self.indent -= 1;
                        self.write("}");
                    }
                }
            }
            ExprKind::FnExpr(fn_decl) => {
                let prev_in_parameter_initializer = self.in_parameter_initializer;
                self.in_parameter_initializer = false;
                // Function expressions have their own `this`; class static aliasing
                // (`this` -> `_a`) applies only to lexical `this` contexts.
                let saved_static_this_alias = self.static_this_alias.take();
                let saved_arguments_alias = self.current_arguments_alias.take();
                // When a type assertion is stripped from an expression statement
                // that has a call/member chain (e.g. `<T>function(){}()`),
                // wrap only the function expression in parens: `(function(){})()`
                let disambig_wrap = self.disambig_wrap_leftmost_fn;
                if disambig_wrap {
                    self.write("(");
                    self.disambig_wrap_leftmost_fn = false;
                }
                let dl_async_fn =
                    fn_decl.is_async && !fn_decl.is_generator && self.needs_downlevel("async");
                let dl_async_gen_fn = fn_decl.is_async
                    && fn_decl.is_generator
                    && self.needs_downlevel("async-generator");
                let async_gen_move_params =
                    dl_async_gen_fn && fn_decl.params.iter().any(Self::param_needs_async_lift);
                let async_move_params = dl_async_fn
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
                let downlevel_simple_params = !fn_decl.is_async
                    && !fn_decl.is_generator
                    && self.can_downlevel_simple_param_initializers(&fn_decl.params);
                if fn_decl.is_async && !(dl_async_fn || dl_async_gen_fn) {
                    self.write("async ");
                }
                self.write("function");
                if fn_decl.is_generator && !dl_async_gen_fn {
                    self.write("*");
                }
                if let Some(ref name) = fn_decl.name {
                    self.write(" ");
                    self.write(name);
                }
                // TypeScript always has a space before ( for anonymous functions
                if fn_decl.name.is_none() {
                    self.write(" ");
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
                } else if downlevel_simple_params {
                    self.emit_params_without_initializers(&fn_decl.params);
                    vec![]
                } else {
                    self.emit_params(&fn_decl.params);
                    vec![]
                };
                self.write(") ");
                // Skip comments inside erased return type annotation
                if let Some(ref rt) = fn_decl.return_type {
                    self.advance_comment_pos(rt.span.end);
                }
                if let Some(ref body) = fn_decl.body {
                    let async_arguments_alias = if dl_async_fn && stmts_have_lexical_arguments(body)
                    {
                        Some(self.next_arguments_capture_name())
                    } else {
                        None
                    };
                    if dl_async_fn || dl_async_gen_fn {
                        if dl_async_gen_fn {
                            let inner_name = fn_decl.name.as_ref().map(|n| format!("{n}_1"));
                            self.awaiter_enclosing_span = Some(expr.span);
                            self.emit_async_generator_body(
                                body,
                                inner_name.as_deref(),
                                async_gen_move_params.then_some(fn_decl.params.as_slice()),
                            );
                        } else if !rest_infos.is_empty() {
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
                            self.write("return ");
                            self.write(self.helper_prefix());
                            self.write("__awaiter(this, void 0, void 0, function* () ");
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
                            self.awaiter_enclosing_span = Some(expr.span);
                            self.emit_awaiter_body_with_params(
                                body,
                                Some(&fn_decl.params),
                                &fn_decl.params,
                                async_arguments_alias.as_deref(),
                            );
                        } else {
                            self.awaiter_enclosing_span = Some(expr.span);
                            self.emit_awaiter_body_with_params(
                                body,
                                None,
                                &fn_decl.params,
                                async_arguments_alias.as_deref(),
                            );
                        }
                    } else if !rest_infos.is_empty() {
                        self.writeln("{");
                        self.indent += 1;
                        for (_, temp_name, pat, _) in &rest_infos {
                            self.emit_rest_param_destructuring(temp_name, pat);
                        }
                        self.emit_rest_lifted_defaults();
                        for s in body {
                            self.emit_stmt(s);
                        }
                        self.indent -= 1;
                        self.write("}");
                    } else if downlevel_simple_params {
                        self.emit_block_for_decl_body_with_param_initializers(
                            &fn_decl.params,
                            body,
                            expr.span,
                        );
                    } else {
                        self.emit_block_for_decl_body(body, expr.span);
                    }
                }
                if disambig_wrap {
                    self.write(")");
                }
                self.current_arguments_alias = saved_arguments_alias;
                self.static_this_alias = saved_static_this_alias;
                self.in_parameter_initializer = prev_in_parameter_initializer;
            }
            ExprKind::Arrow(arrow) => {
                let saved_arrow_arguments_alias = self.current_arguments_alias.clone();
                let prev_in_parameter_initializer = self.in_parameter_initializer;
                self.in_parameter_initializer = false;
                let prev_in_async = self.in_async_function;
                self.in_async_function = arrow.is_async;
                // Track parameter names that shadow CJS/namespace exports so a
                // reference to `x` inside this arrow body isn't qualified as
                // `exports.x` / `NS.x` when `x` is one of this arrow's params.
                // (Arrows previously did no such tracking, so `(v) => (n) => n < v`
                // with an exported `const v` mis-qualified the param to `exports.v`.)
                // Union with the enclosing scope's shadows so a nested arrow still
                // sees its outer params; only bookkeep when exports exist to shadow.
                let prev_arrow_param_shadows = if !self.cjs_var_export_names.is_empty()
                    || !self.namespace_exports.is_empty()
                {
                    let snapshot = self.cjs_param_shadows.clone();
                    for p in &arrow.params {
                        if !self.cjs_var_export_names.is_empty() {
                            Self::collect_param_shadow_names(
                                &p.name,
                                &self.cjs_var_export_names,
                                &mut self.cjs_param_shadows,
                            );
                        }
                        if !self.namespace_exports.is_empty() {
                            Self::collect_param_shadow_names(
                                &p.name,
                                &self.namespace_exports,
                                &mut self.cjs_param_shadows,
                            );
                        }
                    }
                    Some(snapshot)
                } else {
                    None
                };
                let dl_async_arrow = arrow.is_async && self.needs_downlevel("async");
                let needs_rest = self.params_need_rest_transform(&arrow.params);
                let downlevel_simple_arrow =
                    self.can_downlevel_simple_arrow_params(expr.span, arrow);
                let lower_hazard_free_es5_arrow = !prev_in_parameter_initializer
                    && self.can_downlevel_hazard_free_es5_arrow(expr.span, arrow);
                let lower_lexical_loop_arrow = !arrow.is_async
                    && self.needs_lexical_downlevel()
                    && !self.active_lexical_loop_helpers.is_empty();
                if lower_lexical_loop_arrow {
                    self.write("function (");
                    if downlevel_simple_arrow {
                        self.emit_params_without_initializers(&arrow.params);
                    } else {
                        self.emit_params(&arrow.params);
                    }
                    self.write(") ");
                    let saved_alias = self.lexical_arrow_this_alias.clone();
                    let active_plan = self
                        .active_lexical_loop_helpers
                        .last()
                        .and_then(|id| self.lexical_downlevel_plan.loops.get(*id));
                    if active_plan.is_some_and(|plan| plan.captures_this) {
                        self.lexical_arrow_this_alias = Some(self.lexical_loop_this_alias());
                    }
                    match &arrow.body {
                        ArrowBody::Expr(body) => {
                            self.write("{ return ");
                            self.emit_expr(body);
                            self.write("; }");
                        }
                        ArrowBody::Block(stmts) if downlevel_simple_arrow => {
                            self.emit_block_for_decl_body_with_param_initializers(
                                &arrow.params,
                                stmts,
                                expr.span,
                            );
                        }
                        ArrowBody::Block(stmts) => {
                            self.emit_block_with_span(stmts, Some(arrow.span))
                        }
                    }
                    self.lexical_arrow_this_alias = saved_alias;
                    if let Some(snapshot) = prev_arrow_param_shadows {
                        self.cjs_param_shadows = snapshot;
                    }
                    self.in_async_function = prev_in_async;
                    return;
                }
                if lower_hazard_free_es5_arrow {
                    self.write("function (");
                    self.emit_params_without_initializers(&arrow.params);
                    self.write(") ");
                    match &arrow.body {
                        ArrowBody::Block(stmts) => {
                            self.emit_block_for_decl_body(stmts, expr.span);
                        }
                        ArrowBody::Expr(body) => {
                            // A concise arrow body is a function scope.  Isolate
                            // both its temp declarations and naming sequence from
                            // the surrounding file/function while rendering it.
                            let prev_temp_var_counter = self.temp_var_counter;
                            let prev_temp_var_names = std::mem::take(&mut self.temp_var_names);
                            self.temp_var_counter = 0;
                            let retain_comments = self.options.remove_comments != Some(true)
                                || self.preserve_comments;
                            let has_detached_comments = retain_comments
                                && self.comments.iter().any(|comment| {
                                    comment.pos >= expr.span.start && comment.end <= body.span.start
                                });
                            let local_temp_names = self.discover_hazard_free_arrow_body_temps(body);
                            if has_detached_comments {
                                if local_temp_names.is_empty() {
                                    self.write("{");
                                    self.indent += 1;
                                    self.emit_detached_arrow_body_comments(body.span.start);
                                    self.write("return ");
                                    self.emit_hazard_free_arrow_return_expr(body);
                                    self.writeln(";");
                                    self.indent -= 1;
                                    self.write("}");
                                } else {
                                    // TS emits a generated concise-body temp on the
                                    // opening line, then keeps the detached comment
                                    // and return at the enclosing indentation:
                                    // `{ var _a; \n// comment\nreturn value; }`.
                                    self.write("{ var ");
                                    self.write(&local_temp_names.join(", "));
                                    self.write("; ");
                                    self.emit_detached_arrow_body_comments(body.span.start);
                                    self.write("return ");
                                    self.emit_hazard_free_arrow_return_expr(body);
                                    self.write("; }");
                                }
                            } else {
                                self.write("{ ");
                                if !local_temp_names.is_empty() {
                                    self.write("var ");
                                    self.write(&local_temp_names.join(", "));
                                    self.write("; ");
                                }
                                self.write("return ");
                                self.emit_hazard_free_arrow_return_expr(body);
                                self.write("; }");
                            }
                            debug_assert_eq!(self.temp_var_names, local_temp_names);
                            self.temp_var_counter = prev_temp_var_counter;
                            self.temp_var_names = prev_temp_var_names;
                        }
                    }
                    self.in_async_function = prev_in_async;
                    if let Some(snapshot) = prev_arrow_param_shadows {
                        self.cjs_param_shadows = snapshot;
                    }
                    self.in_parameter_initializer = prev_in_parameter_initializer;
                    return;
                }
                if downlevel_simple_arrow {
                    self.write("function (");
                    self.emit_params_without_initializers(&arrow.params);
                    self.write(") ");
                    if let ArrowBody::Block(stmts) = &arrow.body {
                        self.emit_block_for_decl_body_with_param_initializers(
                            &arrow.params,
                            stmts,
                            expr.span,
                        );
                    }
                    self.in_async_function = prev_in_async;
                    if let Some(snapshot) = prev_arrow_param_shadows {
                        self.cjs_param_shadows = snapshot;
                    }
                    return;
                }
                if arrow.is_async && !dl_async_arrow {
                    self.write("async ");
                }
                if dl_async_arrow {
                    // Downlevel: convert async arrow to arrow + __awaiter
                    // At module level (fn_scope_depth == 0), use void 0 for thisArg.
                    // Inside a function, use this (arrow captures enclosing this).
                    let this_arg = if self.fn_scope_depth > 0 || prev_in_parameter_initializer {
                        "this"
                    } else {
                        "void 0"
                    };
                    let has_param_initializer =
                        arrow.params.iter().any(|p| p.initializer.is_some());
                    let has_non_ident_param = arrow
                        .params
                        .iter()
                        .any(|p| !matches!(p.name.kind, PatKind::Ident(_)));
                    let arrow_async_lift =
                        !needs_rest && (has_param_initializer || has_non_ident_param);
                    let arrow_async_lift_defaults = arrow_async_lift && has_param_initializer;
                    let lift_temps = if arrow_async_lift && !arrow_async_lift_defaults {
                        Self::generate_async_lift_temp_names(&arrow.params)
                    } else {
                        Vec::new()
                    };
                    let prefix_temps = if arrow_async_lift_defaults {
                        Self::generate_async_lift_prefix_temp_names(&arrow.params)
                    } else {
                        Vec::new()
                    };
                    let arrow_needs_arguments_alias = self.current_arguments_alias.is_none()
                        && match &arrow.body {
                            ArrowBody::Expr(e) => expr_has_lexical_arguments(e),
                            ArrowBody::Block(stmts) => stmts_have_lexical_arguments(stmts),
                        };
                    let es5_plan = (!arrow_async_lift
                        && !needs_rest
                        && (self.needs_generator_helper
                            || self.options.no_emit_helpers == Some(true)))
                    .then(|| self.plan_es5_async_arrow(arrow))
                    .flatten();
                    // Nested lexical receivers and arguments keep the existing
                    // arrow wrapper until lexical captures have been hoisted.
                    let lower_outer_arrow = es5_plan.is_some()
                        && this_arg == "void 0"
                        && !arrow_has_lexical_environment_hazard(arrow);
                    self.write(if lower_outer_arrow { "function (" } else { "(" });
                    let rest_infos = if arrow_async_lift_defaults {
                        for (i, t) in prefix_temps.iter().enumerate() {
                            if i > 0 {
                                self.write(", ");
                            }
                            self.write(t);
                        }
                        if !prefix_temps.is_empty() {
                            self.write(", ");
                        }
                        self.write("...args_1");
                        vec![]
                    } else if arrow_async_lift {
                        // Destructured param lift keeps explicit temp parameter names.
                        for (i, t) in lift_temps.iter().enumerate() {
                            if i > 0 {
                                self.write(", ");
                            }
                            self.write(t);
                        }
                        vec![]
                    } else if needs_rest {
                        self.emit_params_with_rest_transform(&arrow.params)
                    } else {
                        self.emit_params(&arrow.params);
                        vec![]
                    };
                    self.write(if lower_outer_arrow {
                        ") { return "
                    } else {
                        ") => "
                    });
                    // Skip comments inside erased return type annotation.
                    // Advance past the `=>` token to also skip comments between
                    // the return type and `=>`.
                    if let Some(ref rt) = arrow.return_type {
                        let expr_start = expr.span.start as usize;
                        let expr_end = expr.span.end as usize;
                        if expr_start < expr_end && expr_end <= self.source.len() {
                            let source_text = &self.source[expr_start..expr_end];
                            if let Some(arrow_pos) = Self::find_fat_arrow_pos(source_text) {
                                let abs_arrow_end = expr_start + arrow_pos + 2;
                                self.advance_comment_pos(abs_arrow_end as u32);
                            } else {
                                self.advance_comment_pos(rt.span.end);
                            }
                        } else {
                            self.advance_comment_pos(rt.span.end);
                        }
                    }
                    let emit_awaiter_call = |emitter: &mut Self| {
                        emitter.write(emitter.helper_prefix());
                        if arrow_async_lift_defaults {
                            emitter.write("__awaiter(");
                            emitter.write(this_arg);
                            emitter.write(", [");
                            for (i, t) in prefix_temps.iter().enumerate() {
                                if i > 0 {
                                    emitter.write(", ");
                                }
                                emitter.write(t);
                            }
                            if !prefix_temps.is_empty() {
                                emitter.write(", ");
                            }
                            emitter.write("...args_1], void 0, function* (");
                            emitter.emit_params(&arrow.params);
                            emitter.write(") ");
                        } else if arrow_async_lift {
                            emitter.write("__awaiter(");
                            emitter.write(this_arg);
                            emitter.write(", [");
                            for (i, t) in lift_temps.iter().enumerate() {
                                if i > 0 {
                                    emitter.write(", ");
                                }
                                emitter.write(t);
                            }
                            emitter.write("], void 0, function* (");
                            emitter.emit_params(&arrow.params);
                            emitter.write(") ");
                        } else {
                            emitter.write("__awaiter(");
                            emitter.write(this_arg);
                            emitter.write(if es5_plan.is_some() {
                                ", void 0, void 0, function () "
                            } else {
                                ", void 0, void 0, function* () "
                            });
                        }
                        if let Some(plan) = &es5_plan {
                            emitter.emit_es5_async_generator_layout(
                                plan,
                                &arrow.params,
                                matches!(arrow.body, ArrowBody::Expr(_)),
                            );
                        } else if !rest_infos.is_empty() {
                            // Rest transform forces multi-line block body
                            emitter.writeln("{");
                            emitter.indent += 1;
                            for (_, temp_name, pat, _) in &rest_infos {
                                emitter
                                    .emit_rest_param_destructuring_await_to_yield(temp_name, pat);
                            }
                            emitter.emit_rest_lifted_defaults();
                            match &arrow.body {
                                ArrowBody::Expr(e) => {
                                    emitter.write("return ");
                                    emitter.emit_expr_await_to_yield(e);
                                    emitter.writeln(";");
                                }
                                ArrowBody::Block(stmts) => {
                                    for s in stmts {
                                        emitter.emit_leading_comments(s.span.start);
                                        emitter.emit_stmt_await_to_yield(s);
                                        emitter.advance_comment_pos(s.span.end);
                                    }
                                }
                            }
                            emitter.indent -= 1;
                            emitter.write("}");
                        } else {
                            match &arrow.body {
                                ArrowBody::Expr(e) => {
                                    // Single-expression body: inline on one line
                                    emitter.write("{ return ");
                                    emitter.emit_expr_await_to_yield(e);
                                    emitter.write("; }")
                                }
                                ArrowBody::Block(stmts) if stmts.is_empty() => {
                                    // Check if source body was multi-line
                                    let start = expr.span.start as usize;
                                    let end = expr.span.end as usize;
                                    let is_ml = if start < end && end <= emitter.source.len() {
                                        let fn_text = &emitter.source[start..end];
                                        fn_text
                                            .rfind('{')
                                            .map_or(false, |bp| fn_text[bp..].contains('\n'))
                                    } else {
                                        false
                                    };
                                    if is_ml {
                                        emitter.writeln("{");
                                        emitter.write("}");
                                    } else {
                                        emitter.write("{ }");
                                    }
                                }
                                ArrowBody::Block(stmts) => {
                                    // Collapse to single-line when source was single-line.
                                    let start = expr.span.start as usize;
                                    let end = expr.span.end as usize;
                                    let is_sl = if start < end && end <= emitter.source.len() {
                                        let text = &emitter.source[start..end];
                                        text.rfind('{')
                                            .map_or(false, |bp| !text[bp..].contains('\n'))
                                    } else {
                                        false
                                    };
                                    if is_sl && !stmts.is_empty() {
                                        emitter.awaiter_enclosing_span = None;
                                        emitter.write("{ ");
                                        let pc = emitter.preserve_const_enums_effective();
                                        for s in stmts {
                                            if stmt_is_erased(s, pc) {
                                                continue;
                                            }
                                            emitter.emit_stmt_await_to_yield(s);
                                        }
                                        if emitter.output.ends_with('\n') {
                                            emitter.output.pop();
                                            emitter.at_line_start = false;
                                        }
                                        emitter.write(" }");
                                    } else {
                                        emitter.emit_block_with_await_to_yield(stmts);
                                    }
                                }
                            }
                        }
                        emitter.write(")");
                    };
                    if arrow_needs_arguments_alias {
                        let alias = self.next_arguments_capture_name();
                        self.writeln("{");
                        self.indent += 1;
                        self.write("var ");
                        self.write(&alias);
                        self.writeln(" = arguments;");
                        self.current_arguments_alias = Some(alias);
                        self.write("return ");
                        emit_awaiter_call(self);
                        self.writeln(";");
                        self.indent -= 1;
                        self.write("}");
                    } else {
                        emit_awaiter_call(self);
                    }
                    if lower_outer_arrow {
                        self.write("; }");
                    }
                } else {
                    // Non-async arrow
                    let rest_infos = if needs_rest {
                        self.write("(");
                        let infos = self.emit_params_with_rest_transform(&arrow.params);
                        self.write(")");
                        infos
                    } else {
                        let single_param = arrow.params.len() == 1
                            && !arrow.params[0].dotdotdot
                            && arrow.params[0].type_ann.is_none()
                            && arrow.params[0].initializer.is_none()
                            && matches!(arrow.params[0].name.kind, PatKind::Ident(_));
                        if single_param {
                            // When type params are present (stripped in output),
                            // always emit parens around the single parameter.
                            let has_type_params =
                                arrow.type_params.as_ref().is_some_and(|tp| !tp.is_empty());
                            // Check source text to see if parens were present
                            let start = expr.span.start as usize;
                            let end = expr.span.end as usize;
                            let source_has_parens = has_type_params
                                || arrow.is_async
                                || if start < end && end <= self.source.len() {
                                    let src = &self.source[start..end];
                                    // Skip "async " prefix if present
                                    let src = if arrow.is_async {
                                        src.trim_start()
                                            .strip_prefix("async")
                                            .unwrap_or(src)
                                            .trim_start()
                                    } else {
                                        src.trim_start()
                                    };
                                    src.starts_with('(')
                                } else {
                                    false
                                };
                            if source_has_parens {
                                self.write("(");
                                // Emit inline comments before parameter
                                // e.g. `(/** @type {...} */ prop)` → preserve the JSDoc comment
                                self.emit_inline_comments_before(arrow.params[0].name.span.start);
                                self.emit_binding_name(&arrow.params[0].name);
                                self.write(")");
                            } else {
                                self.emit_binding_name(&arrow.params[0].name);
                            }
                        } else {
                            self.write("(");
                            self.emit_params(&arrow.params);
                            self.write(")");
                        }
                        vec![]
                    };
                    self.write(" => ");
                    // Skip comments inside erased return type annotation.
                    // We must advance past the `=>` token, not just the return
                    // type end, because comments between the return type and `=>`
                    // (e.g. `): number /* */ => a`) should also be skipped.
                    // Skip comments between params/return-type and `=>`.
                    {
                        let expr_start = expr.span.start as usize;
                        let expr_end = expr.span.end as usize;
                        if expr_start < expr_end && expr_end <= self.source.len() {
                            let source_text = &self.source[expr_start..expr_end];
                            if let Some(arrow_pos) = Self::find_fat_arrow_pos(source_text) {
                                // Advance past `=>` (2 chars) to skip any comments
                                // between `)` / return type and `=>`
                                let abs_arrow_end = expr_start + arrow_pos + 2;
                                self.advance_comment_pos(abs_arrow_end as u32);
                            } else if let Some(ref rt) = arrow.return_type {
                                self.advance_comment_pos(rt.span.end);
                            }
                        } else if let Some(ref rt) = arrow.return_type {
                            self.advance_comment_pos(rt.span.end);
                        }
                    }
                    if !rest_infos.is_empty() {
                        // Rest transform: expression body becomes block body.
                        // Scope deferred temps so each arrow body gets its own
                        // `_a, _b, ...` sequence (TypeScript resets per scope).
                        let prev_temp_counter = self.temp_var_counter;
                        let deferred_start = self.inline_deferred_temp_placeholders.len();
                        let param_rest_temps = self.fn_param_rest_temp_count;
                        self.fn_param_rest_temp_count = 0;
                        self.temp_var_counter =
                            self.class_scope_temp_reserved.max(param_rest_temps);
                        self.writeln("{");
                        self.indent += 1;
                        for (_, temp_name, pat, _) in &rest_infos {
                            self.emit_rest_param_destructuring(temp_name, pat);
                        }
                        self.emit_rest_lifted_defaults();
                        match &arrow.body {
                            ArrowBody::Expr(e) => {
                                self.write("return ");
                                self.emit_expr(e);
                                self.writeln(";");
                            }
                            ArrowBody::Block(stmts) => {
                                for s in stmts {
                                    self.emit_stmt(s);
                                }
                            }
                        }
                        self.indent -= 1;
                        self.write("}");
                        self.resolve_scoped_inline_deferred_temps(deferred_start);
                        self.temp_var_counter = prev_temp_counter;
                    } else {
                        match &arrow.body {
                            ArrowBody::Expr(e) => {
                                // Emit detached comments (e.g. /// doc comments)
                                // between `=>` and the body expression.
                                self.emit_detached_arrow_body_comments(e.span.start);
                                // Track temp var allocation to detect when a concise
                                // arrow body needs conversion to block body.
                                let temp_var_start = self.temp_var_names.len();
                                let output_start = self.output.len();
                                if arrow_body_emits_as_object_lit(e) {
                                    self.write("(");
                                    self.emit_expr(e);
                                    self.write(")");
                                } else if super::emit_stmt_helpers::expr_stmt_needs_outer_paren_wrap(
                                    e,
                                ) {
                                    self.write("(");
                                    self.suppress_stmt_paren_inner = true;
                                    self.emit_expr(e);
                                    self.suppress_stmt_paren_inner = false;
                                    self.write(")");
                                } else {
                                    let prev = self.in_arrow_body_expr;
                                    self.in_arrow_body_expr = true;
                                    self.suppress_oc_parens = true;
                                    self.emit_expr(e);
                                    self.in_arrow_body_expr = prev;
                                }
                                // If temp vars were allocated during expression emit
                                // (e.g. for optional chaining downlevel), convert the
                                // concise arrow body to a block body with local var decls.
                                if self.temp_var_names.len() > temp_var_start {
                                    let emitted = self.output[output_start..].to_string();
                                    self.output.truncate(output_start);
                                    let new_vars: Vec<AstString> =
                                        self.temp_var_names[temp_var_start..].to_vec();
                                    self.temp_var_names.truncate(temp_var_start);
                                    self.write("{ var ");
                                    self.write(&new_vars.join(", "));
                                    self.write("; return ");
                                    self.output.push_str(&emitted);
                                    self.write("; }");
                                }
                            }
                            ArrowBody::Block(stmts) => {
                                self.emit_block_for_decl_body(stmts, expr.span);
                            }
                        }
                    }
                }
                if self.effective_target() < ScriptTarget::ES2015 {
                    self.current_arguments_alias = saved_arrow_arguments_alias;
                }
                self.in_async_function = prev_in_async;
                if let Some(prev) = prev_arrow_param_shadows {
                    self.cjs_param_shadows = prev;
                }
                self.in_parameter_initializer = prev_in_parameter_initializer;
            }
            ExprKind::ClassExpr(class_decl) => {
                // Standard (TC39) decorator on the class itself — emit IIFE wrapper
                let can_emit_simple_standard_decorator_wrapper = !class_decl.decorators.is_empty()
                    && !self.should_preserve_decorators()
                    && self.options.experimental_decorators != Some(true)
                    && class_expr_can_emit_simple_standard_decorator_wrapper(class_decl);
                let can_emit_narrow_standard_decorator_expr = !self.should_preserve_decorators()
                    && self.options.experimental_decorators != Some(true)
                    && class_can_emit_narrow_standard_decorator_member_expr(class_decl);
                let needs_static_iife = class_has_static_initializers(class_decl)
                    && self.needs_downlevel("static-blocks");
                // In legacy class-fields mode, wrap only when key evaluations
                // cannot be fully merged into computed method names.
                let needs_computed_instance_iife =
                    self.class_expr_needs_legacy_computed_iife(class_decl);
                let needs_private_iife = self.class_expr_needs_private_iife(class_decl);
                if can_emit_simple_standard_decorator_wrapper {
                    let assigned_name = self.class_expr_binding_name.take();
                    if self.can_emit_downlevel_using_decorated_class_expr(class_decl) {
                        self.emit_downlevel_using_decorated_class_expr_wrapper(
                            class_decl,
                            assigned_name.as_ref(),
                        );
                    } else {
                        self.emit_simple_standard_decorated_class_expr_wrapper(
                            class_decl,
                            assigned_name.as_ref(),
                        );
                    }
                } else if can_emit_narrow_standard_decorator_expr {
                    let binding_name = self.class_expr_binding_name.take();
                    self.emit_narrow_standard_decorated_class_expr_iife(
                        class_decl,
                        binding_name.as_ref(),
                    );
                } else if needs_static_iife || needs_computed_instance_iife || needs_private_iife {
                    let binding_name = self.class_expr_binding_name.take();
                    // When this class expr is a concise arrow body, the arrow
                    // handler converts it to `{ var _a; return <expr>; }` — a
                    // return statement doesn't need the outer parens.
                    let wrap = !self.in_arrow_body_expr;
                    self.emit_class_expr_iife_with_wrap(class_decl, wrap, binding_name.as_ref());
                } else {
                    let saved_class_expression_emit = self.in_class_expression_emit;
                    self.in_class_expression_emit = true;
                    self.emit_class_decl(class_decl);
                    self.in_class_expression_emit = saved_class_expression_emit;
                    // Class bodies end with `}\n` — strip the newline when the
                    // class expression is part of a larger expression (e.g.
                    // `typeof class {} === "function"`).
                    self.strip_trailing_newline();
                }
            }
            ExprKind::Call(call) => {
                if self
                    .lexical_downlevel_plan
                    .invocation_spreads
                    .contains_key(&expr.span.into())
                {
                    let temporary =
                        crate::call_spread::needs_receiver_temp(&call.callee, self.source).then(
                            || Expr {
                                kind: ExprKind::Ident(self.next_temp_var().into()),
                                span: Span::new(0, 0),
                            },
                        );
                    let lowered = self.lower_spread_call(call, temporary);
                    self.emit_expr(&lowered);
                    return;
                }
                if matches!(call.callee.kind, ExprKind::Super)
                    && self.legacy_constructor_super.is_some()
                {
                    let super_name = self.legacy_constructor_super.clone().unwrap();
                    if let Some(this_alias) = self.legacy_this_alias.clone() {
                        self.write(&this_alias);
                        self.write(" = ");
                    }
                    self.write(&super_name);
                    self.write(".call(this");
                    for arg in &call.args {
                        self.write(", ");
                        self.emit_expr(arg);
                    }
                    self.write(") || this");
                } else if self.is_system()
                    && !self.system_context_fn.is_empty()
                    && self.is_dynamic_import_call(call)
                {
                    // System modules: import(x) → context_N.import(x)
                    let ctx = self.system_context_fn.clone();
                    self.write(&ctx);
                    self.write(".import(");
                    self.emit_system_import_arg(&call.args[0]);
                    self.write(")");
                } else if self.should_downlevel_dynamic_import()
                    && self.is_dynamic_import_call(call)
                {
                    let arg = call
                        .args
                        .first()
                        .map(|v| &**v)
                        .filter(|a| !self.is_phantom_arg(a));
                    if self.is_umd() {
                        self.emit_dynamic_import_call_umd(arg);
                    } else if self.is_amd() {
                        self.emit_dynamic_import_call_amd(arg);
                    } else {
                        self.emit_dynamic_import_call_commonjs(arg);
                    }
                } else if self.rewrite_relative_import_extensions()
                    && self.is_dynamic_import_call(call)
                {
                    self.write("import(");
                    if let Some(arg) = call.args.first() {
                        self.emit_rewritten_relative_import_arg(arg);
                    }
                    for arg in call.args.iter().skip(1) {
                        self.write(", ");
                        self.emit_expr(arg);
                    }
                    self.write(")");
                } else if self.rewrite_relative_import_extensions()
                    && !call.optional
                    && matches!(call.callee.kind, ExprKind::Ident(ref name) if name == "require")
                {
                    self.write("require(");
                    if let Some(arg) = call.args.first() {
                        self.emit_rewritten_relative_import_arg(arg);
                    }
                    for arg in call.args.iter().skip(1) {
                        self.write(", ");
                        self.emit_expr(arg);
                    }
                    self.write(")");
                } else if call.optional && self.needs_downlevel("optional-chaining") {
                    self.emit_optional_call_downlevel(call);
                } else if !call.optional
                    && self.needs_downlevel("optional-chaining")
                    && is_oc_chain(&call.callee)
                {
                    // Non-optional call whose callee is in an OC chain.
                    // Inline the call inside the ternary's truthy branch.
                    // `o2?.b(args)` → `o2 === null || o2 === void 0 ? void 0 : o2.b(args)`
                    // `o2?.["b"](args)` → `o2 === null || o2 === void 0 ? void 0 : o2["b"](args)`
                    self.emit_oc_call_inline(call);
                } else {
                    if self.needs_downlevel("private-fields") {
                        if let ExprKind::Member(mem) = &call.callee.kind {
                            if mem.property.starts_with('#') {
                                let method_name = normalize_unicode_escapes(&mem.property[1..]);
                                // Determine private kind (method or field)
                                let private_call_info: Option<(
                                    String,
                                    &'static str,
                                    Option<String>,
                                )> = if let Some(method_var) = self
                                    .current_class_private_methods
                                    .get(method_name.as_str())
                                    .cloned()
                                {
                                    let brand_var = self.current_class_instances_var();
                                    Some((brand_var, "\"m\"", Some(method_var.to_string())))
                                } else if let Some((getter_var, _)) = self
                                    .current_class_private_accessors
                                    .get(method_name.as_str())
                                    .cloned()
                                {
                                    // Accessor call: __classPrivateFieldGet(obj, _inst/"a", getter).call(obj)
                                    let is_static_acc = self
                                        .current_class_static_private_accessors
                                        .contains(method_name.as_str());
                                    let brand_var = if is_static_acc {
                                        self.current_class_static_alias_or_default()
                                    } else {
                                        self.current_class_instances_var()
                                    };
                                    Some((brand_var, "\"a\"", getter_var.map(|v| v.to_string())))
                                } else if let Some(method_var) = self
                                    .current_class_static_private_methods
                                    .get(method_name.as_str())
                                    .cloned()
                                {
                                    // Static method: __classPrivateFieldGet(obj, _a, "m", _A_m).call(obj)
                                    let alias = self.current_class_static_alias_or_default();
                                    Some((alias, "\"m\"", Some(method_var.to_string())))
                                } else if self
                                    .current_class_private_fields
                                    .contains(method_name.as_str())
                                {
                                    let field_var = self.private_field_var_name(&method_name);
                                    let is_static = self
                                        .current_class_static_private_fields
                                        .contains(method_name.as_str());
                                    if is_static {
                                        let alias = self.current_class_static_alias_or_default();
                                        // Static field: (alias, "f", field_var)
                                        Some((alias, "\"f\"", Some(field_var)))
                                    } else {
                                        Some((field_var, "\"f\"", None))
                                    }
                                } else {
                                    None
                                };
                                if let Some((var_name, kind_str, method_fn)) = private_call_info {
                                    self.needs_private_field_get = true;
                                    let is_static_field = self
                                        .current_class_static_private_fields
                                        .contains(method_name.as_str())
                                        || self
                                            .current_class_static_private_methods
                                            .contains_key(method_name.as_str())
                                        || self
                                            .current_class_static_private_accessors
                                            .contains(method_name.as_str());
                                    let is_simple = matches!(
                                        &mem.object.kind,
                                        ExprKind::This | ExprKind::Ident(_)
                                    );
                                    let recv_temp = if is_simple {
                                        None
                                    } else {
                                        Some(self.next_temp_var())
                                    };
                                    self.write(self.helper_prefix());
                                    self.write("__classPrivateFieldGet(");
                                    if let Some(ref t) = recv_temp {
                                        self.write("(");
                                        self.write(t);
                                        self.write(" = ");
                                        self.emit_expr(&mem.object);
                                        self.write(")");
                                    } else if is_static_field {
                                        let alias = self.current_class_static_alias_or_default();
                                        self.emit_private_field_receiver(&mem.object, &alias);
                                    } else {
                                        self.emit_expr(&mem.object);
                                    }
                                    self.write(", ");
                                    self.write(&var_name);
                                    self.write(", ");
                                    self.write(&kind_str);
                                    if let Some(ref mf) = method_fn {
                                        self.write(", ");
                                        self.write(mf);
                                    }
                                    self.write(").call(");
                                    if let Some(ref t) = recv_temp {
                                        self.write(t);
                                    } else if is_static_field {
                                        let alias = self.current_class_static_alias_or_default();
                                        self.emit_private_field_receiver(&mem.object, &alias);
                                    } else {
                                        self.emit_expr(&mem.object);
                                    }
                                    if !call.args.is_empty() {
                                        self.write(", ");
                                    }
                                    for (i, arg) in call.args.iter().enumerate() {
                                        if i > 0 {
                                            self.write(", ");
                                        }
                                        self.suppress_oc_parens = true;
                                        self.emit_expr(arg);
                                    }
                                    self.write(")");
                                    return;
                                }
                            }
                        }
                    }
                    // Async super hoisting: super.x() → _super.x.call(this, args)
                    if self.async_super_active {
                        if let ExprKind::Member(ref mem) = call.callee.kind {
                            if matches!(&mem.object.kind, ExprKind::Super) {
                                let sfx = self.async_super_suffix.clone();
                                self.write("_super");
                                self.write(&sfx);
                                self.write(".");
                                self.write(&mem.property);
                                self.write(".call(this");
                                for arg in &call.args {
                                    self.write(", ");
                                    self.emit_expr(arg);
                                }
                                self.write(")");
                                return;
                            }
                        }
                        if let ExprKind::ElemAccess(ref ea) = call.callee.kind {
                            if matches!(&ea.object.kind, ExprKind::Super) {
                                let sfx = self.async_super_suffix.clone();
                                self.write("_superIndex");
                                self.write(&sfx);
                                self.write("(");
                                self.emit_expr(&ea.index);
                                if self.async_super_has_write {
                                    self.write(").value.call(this");
                                } else {
                                    self.write(").call(this");
                                }
                                for arg in &call.args {
                                    self.write(", ");
                                    self.emit_expr(arg);
                                }
                                self.write(")");
                                return;
                            }
                        }
                    }
                    // super.method() / super["method"]() in static context →
                    // Reflect.get(base, "method", receiver).call(receiver, args)
                    if let Some(receiver_alias) = self.static_super_receiver_alias.clone() {
                        let is_super_call = match &call.callee.kind {
                            ExprKind::Member(mem) => {
                                !mem.optional
                                    && matches!(&mem.object.kind, ExprKind::Super)
                                    && mem.property != "<error>"
                                    && !mem.property.starts_with('#')
                                    && self.static_super_base_alias.is_some()
                            }
                            ExprKind::ElemAccess(ea) => {
                                !ea.optional
                                    && matches!(&ea.object.kind, ExprKind::Super)
                                    && self.static_super_base_alias.is_some()
                            }
                            _ => false,
                        };
                        if is_super_call {
                            // Emit the callee (which will produce Reflect.get(...))
                            self.emit_expr(&call.callee);
                            // Append .call(receiver, args)
                            self.write(".call(");
                            self.write(&receiver_alias);
                            for (i, arg) in call.args.iter().enumerate() {
                                self.write(", ");
                                if i > 0 {
                                    // already wrote comma before first arg
                                }
                                self.emit_expr(arg);
                            }
                            self.write(")");
                            return;
                        }
                    }
                    let prev_call_ctx = self.in_call_callee_context;
                    self.in_call_callee_context = true;
                    // Bare `super()` call in static context: don't transform
                    // `super` → `Reflect.get(...)` — just emit `super()`.
                    let saved_super_base = if matches!(&call.callee.kind, ExprKind::Super) {
                        self.static_super_base_alias.take()
                    } else {
                        None
                    };
                    self.emit_expr(&call.callee);
                    if let Some(alias) = saved_super_base {
                        self.static_super_base_alias = Some(alias);
                    }
                    self.in_call_callee_context = prev_call_ctx;
                    // In JS files, type arguments are not TS type params — they're
                    // binary `<`/`>` operators. Emit them with spaces.
                    if self.is_js_file {
                        if let Some(ref type_args) = call.type_args {
                            self.emit_js_type_args_as_binary(type_args);
                        }
                    }
                    if call.optional {
                        self.write("?.");
                    }
                    self.write("(");
                    let mut close_on_newline = false;
                    if call.args.is_empty() {
                        close_on_newline = self.emit_empty_args_comments(expr.span);
                    }
                    // Check for block comments between `(` and the first arg.
                    // When present, switch to multiline call formatting to
                    // preserve the comment (e.g. `/** @this {Test} */`).
                    let multiline_comment_call = if !call.args.is_empty() {
                        self.call_has_leading_arg_block_comment(call, expr.span)
                    } else {
                        None
                    };
                    let mut first = true;
                    for arg in call.args.iter() {
                        // Skip omitted expressions (error recovery: `foo(a,,b)` → `foo(a, b)`)
                        if matches!(arg.kind, ExprKind::Omitted) {
                            continue;
                        }
                        // Skip error-placeholder arguments (e.g. bare `\` from parser recovery).
                        // Don't skip for dynamic import() — callee span covers the whole
                        // import(...) text and dropping placeholder args would produce import()().
                        if expr_is_error_placeholder(arg)
                            && !matches!(&call.callee.kind, ExprKind::Ident(n) if n == "import")
                        {
                            continue;
                        }
                        // Skip error-recovery arguments that are just `}` or `)`.
                        {
                            let a_start = arg.span.start as usize;
                            let a_end = arg.span.end as usize;
                            if a_start < a_end && a_end <= self.source.len() {
                                let arg_src = self.source[a_start..a_end].trim();
                                if arg_src == "}" || arg_src == ")" {
                                    continue;
                                }
                            }
                        }
                        if !first {
                            self.write(", ");
                        }
                        // For multiline comment calls, emit the comment before
                        // the first real argument.
                        if first {
                            if let Some(ref comment) = multiline_comment_call {
                                self.newline();
                                self.write(comment);
                                self.newline();
                            }
                        }
                        first = false;
                        self.suppress_oc_parens = true;
                        self.emit_expr(arg);
                    }
                    if multiline_comment_call.is_some() {
                        // Multiline call: closing `)` is already inline
                        // after the last arg (e.g. `}, this)`)
                    } else if close_on_newline {
                        self.newline();
                    }
                    self.write(")");
                }
            }
            ExprKind::New(new_expr) => {
                if self
                    .lexical_downlevel_plan
                    .invocation_spreads
                    .contains_key(&expr.span.into())
                {
                    let temporary = crate::new_spread::needs_constructor_temp(&new_expr.callee)
                        .then(|| Expr {
                            kind: ExprKind::Ident(self.next_temp_var().into()),
                            span: Span::new(0, 0),
                        });
                    let lowered = self.lower_spread_new(new_expr, temporary);
                    self.emit_expr(&lowered);
                    return;
                }
                if self.emit_recovery_new_empty_array_call_callee(new_expr) {
                    return;
                }
                self.write("new ");
                let prev_new_ctx = self.in_new_callee_context;
                self.in_new_callee_context = true;
                // Wrap private field callee in parens for correct new precedence:
                // `new this.#field()` → `new (__classPrivateFieldGet(...))()`
                let needs_new_paren = matches!(
                    &new_expr.callee.kind,
                    ExprKind::Member(mem) if self.private_member_needs_new_paren(mem)
                );
                if needs_new_paren {
                    self.write("(");
                }
                // Skip error-placeholder callees — their source span may
                // overlap with enclosing delimiters (e.g. `new)` → `)`)
                if !matches!(&new_expr.callee.kind, ExprKind::Ident(n) if n == "<error>") {
                    self.emit_expr(&new_expr.callee);
                }
                if needs_new_paren {
                    self.write(")");
                }
                self.in_new_callee_context = prev_new_ctx;
                if let Some(ref args) = new_expr.args {
                    self.write("(");
                    let mut close_on_newline = false;
                    if args.is_empty() {
                        close_on_newline = self.emit_empty_args_comments(expr.span);
                    }
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            self.write(", ");
                        }
                        self.suppress_oc_parens = true;
                        self.emit_expr(arg);
                    }
                    if close_on_newline {
                        self.newline();
                    }
                    self.write(")");
                }
            }
            ExprKind::Member(mem) => {
                // CJS replacement for `import.meta.{dirname,filename,url}`.
                // When emitting CommonJS, `import.meta` has no runtime form, so
                // rewrite the access to the Node equivalents.
                if !mem.optional && self.import_meta_needs_cjs_rewrite() {
                    if let ExprKind::MetaProp(mp) = &mem.object.kind {
                        if mp.meta.as_str() == "import" && mp.property.as_str() == "meta" {
                            match mem.property.as_str() {
                                "dirname" => {
                                    self.write("__dirname");
                                    return;
                                }
                                "filename" => {
                                    self.write("__filename");
                                    return;
                                }
                                "url" => {
                                    self.write("require(\"url\").pathToFileURL(__filename).href");
                                    return;
                                }
                                _ => {}
                            }
                        }
                    }
                }
                // Async super hoisting: super.x → _super.x
                if self.async_super_active
                    && !mem.optional
                    && matches!(&mem.object.kind, ExprKind::Super)
                {
                    let sfx = self.async_super_suffix.clone();
                    self.write("_super");
                    self.write(&sfx);
                    self.write(".");
                    self.write(&mem.property);
                    return;
                }
                if !mem.optional && matches!(&mem.object.kind, ExprKind::Super) {
                    if let (Some(base_alias), Some(receiver_alias)) = (
                        self.static_super_base_alias.clone(),
                        self.static_super_receiver_alias.clone(),
                    ) {
                        if mem.property != "<error>" && !mem.property.starts_with('#') {
                            if self.super_reflect_destructure_target {
                                // Destructuring target: emit setter proxy
                                self.write("({ set value(_a) { Reflect.set(");
                                self.write(&base_alias);
                                self.write(", \"");
                                self.write(&mem.property);
                                self.write("\", _a, ");
                                self.write(&receiver_alias);
                                self.write("); } }).value");
                            } else {
                                self.write("Reflect.get(");
                                self.write(&base_alias);
                                self.write(", ");
                                self.write("\"");
                                self.write(&mem.property);
                                self.write("\"");
                                self.write(", ");
                                self.write(&receiver_alias);
                                self.write(")");
                            }
                            return;
                        }
                    }
                }
                if mem.property.starts_with('#') && self.needs_downlevel("private-fields") {
                    let field_name = normalize_unicode_escapes(&mem.property[1..]);
                    if let Some(method_var) = self
                        .current_class_private_methods
                        .get(field_name.as_str())
                        .cloned()
                    {
                        self.needs_private_field_get = true;
                        let brand_var = if let Some(ref cn) = self.current_class_name {
                            let norm_cn = normalize_unicode_escapes(cn);
                            format!("_{}_instances", norm_cn)
                        } else {
                            "_instances".to_string()
                        };
                        self.write(self.helper_prefix());
                        self.write("__classPrivateFieldGet(");
                        self.emit_expr(&mem.object);
                        self.write(", ");
                        self.write(&brand_var);
                        self.write(", \"m\", ");
                        self.write(&method_var);
                        self.write(")");
                    } else if let Some(method_var) = self
                        .current_class_static_private_methods
                        .get(field_name.as_str())
                        .cloned()
                    {
                        // Static private method: __classPrivateFieldGet(obj, _a, "m", _A_m)
                        self.needs_private_field_get = true;
                        let alias = self
                            .current_class_static_alias
                            .clone()
                            .unwrap_or_else(|| "_a".to_string());
                        self.write(self.helper_prefix());
                        self.write("__classPrivateFieldGet(");
                        self.emit_private_field_receiver(&mem.object, &alias);
                        self.write(", ");
                        self.write(&alias);
                        self.write(", \"m\", ");
                        self.write(&method_var);
                        self.write(")");
                    } else if let Some((getter_var, _setter_var)) = self
                        .current_class_private_accessors
                        .get(field_name.as_str())
                        .cloned()
                    {
                        self.needs_private_field_get = true;
                        let is_static_acc = self
                            .current_class_static_private_accessors
                            .contains(field_name.as_str());
                        let brand_var = if is_static_acc {
                            self.current_class_static_alias
                                .clone()
                                .unwrap_or_else(|| "_a".to_string())
                        } else if let Some(ref cn) = self.current_class_name {
                            let norm_cn = normalize_unicode_escapes(cn);
                            format!("_{}_instances", norm_cn)
                        } else {
                            "_instances".to_string()
                        };
                        self.write(self.helper_prefix());
                        self.write("__classPrivateFieldGet(");
                        if is_static_acc {
                            self.emit_private_field_receiver(&mem.object, &brand_var);
                        } else {
                            self.emit_expr(&mem.object);
                        }
                        self.write(", ");
                        self.write(&brand_var);
                        self.write(", \"a\"");
                        if let Some(ref gv) = getter_var {
                            self.write(", ");
                            self.write(gv);
                        }
                        self.write(")");
                    } else if self
                        .current_class_private_fields
                        .contains(field_name.as_str())
                    {
                        let field_var = if let Some(ref cn) = self.current_class_name {
                            let norm_cn = normalize_unicode_escapes(cn);
                            format!("_{}_{}", norm_cn, field_name)
                        } else {
                            format!("_{}", field_name)
                        };
                        self.needs_private_field_get = true;
                        let is_static = self
                            .current_class_static_private_fields
                            .contains(field_name.as_str());
                        if is_static {
                            // Static: __classPrivateFieldGet(recv, _a, "f", _C_field)
                            let alias = self
                                .current_class_static_alias
                                .clone()
                                .unwrap_or_else(|| "_a".to_string());
                            self.write(self.helper_prefix());
                            self.write("__classPrivateFieldGet(");
                            self.emit_private_field_receiver(&mem.object, &alias);
                            self.write(", ");
                            self.write(&alias);
                            self.write(", \"f\", ");
                            self.write(&field_var);
                            self.write(")");
                        } else {
                            // Instance: __classPrivateFieldGet(obj, _C_field, "f")
                            self.write(self.helper_prefix());
                            self.write("__classPrivateFieldGet(");
                            self.emit_expr(&mem.object);
                            self.write(", ");
                            self.write(&field_var);
                            self.write(", \"f\")");
                        }
                    } else {
                        let mut found_enclosing = false;
                        if let Some((class_name, norm_cn, kind)) =
                            self.enclosing_private_member_info(field_name.as_str())
                        {
                            let alias = self
                                .cjs_import_map
                                .get(class_name.as_str())
                                .map(|(name, _)| name.to_string())
                                .unwrap_or_else(|| "_a".to_string());
                            self.needs_private_field_get = true;
                            self.write(self.helper_prefix());
                            self.write("__classPrivateFieldGet(");
                            match kind {
                                EnclosingPrivateMemberKind::Field { is_static } => {
                                    if is_static {
                                        self.emit_private_enclosing_static_receiver(
                                            &mem.object,
                                            &alias,
                                            class_name.as_str(),
                                        );
                                        self.write(", ");
                                        self.write(&alias);
                                        self.write(", \"f\", _");
                                        self.write(&norm_cn);
                                        self.write("_");
                                        self.write(&field_name);
                                    } else {
                                        self.emit_expr(&mem.object);
                                        self.write(", _");
                                        self.write(&norm_cn);
                                        self.write("_");
                                        self.write(&field_name);
                                        self.write(", \"f\"");
                                    }
                                }
                                EnclosingPrivateMemberKind::Method { is_static } => {
                                    if is_static {
                                        self.emit_private_enclosing_static_receiver(
                                            &mem.object,
                                            &alias,
                                            class_name.as_str(),
                                        );
                                        self.write(", ");
                                        self.write(&alias);
                                        self.write(", \"m\", _");
                                        self.write(&norm_cn);
                                        self.write("_");
                                        self.write(&field_name);
                                    } else {
                                        self.emit_expr(&mem.object);
                                        self.write(", _");
                                        self.write(&norm_cn);
                                        self.write("_instances, \"m\", _");
                                        self.write(&norm_cn);
                                        self.write("_");
                                        self.write(&field_name);
                                    }
                                }
                                EnclosingPrivateMemberKind::Accessor { is_static } => {
                                    if is_static {
                                        self.emit_private_enclosing_static_receiver(
                                            &mem.object,
                                            &alias,
                                            class_name.as_str(),
                                        );
                                        self.write(", ");
                                        self.write(&alias);
                                        self.write(", \"a\", _");
                                        self.write(&norm_cn);
                                        self.write("_");
                                        self.write(&field_name);
                                        self.write("_get");
                                    } else {
                                        self.emit_expr(&mem.object);
                                        self.write(", _");
                                        self.write(&norm_cn);
                                        self.write("_instances, \"a\", _");
                                        self.write(&norm_cn);
                                        self.write("_");
                                        self.write(&field_name);
                                        self.write("_get");
                                    }
                                }
                            }
                            self.write(")");
                            found_enclosing = true;
                        }
                        if !found_enclosing {
                            self.emit_expr(&mem.object);
                            self.write(".");
                            if !mem.property.starts_with('#') {
                                self.write(&mem.property);
                            }
                        }
                    }
                } else if mem.optional && self.needs_downlevel("optional-chaining") {
                    self.emit_optional_member_downlevel(mem, expr.span);
                } else if !mem.optional
                    && self.needs_downlevel("optional-chaining")
                    && is_oc_chain(&mem.object)
                {
                    // Non-optional member access continuing an optional chain.
                    // Emit the object with suppress_oc_parens so the OC ternary
                    // has no outer parens, then append .property — JS precedence
                    // places it in the ternary's alt branch.
                    // `a?.b.d` → `a === null || a === void 0 ? void 0 : a.b.d`
                    let suppress = self.suppress_oc_parens;
                    self.suppress_oc_parens = false;
                    if !suppress {
                        self.write("(");
                    }
                    self.suppress_oc_parens = true;
                    self.emit_expr(&mem.object);
                    if mem.property != "<error>" {
                        self.write(".");
                        self.write(&mem.property);
                    }
                    if !suppress {
                        self.write(")");
                    }
                } else {
                    let prev_member_ctx = self.in_member_object_context;
                    self.in_member_object_context = true;
                    self.emit_expr(&mem.object);
                    self.in_member_object_context = prev_member_ctx;
                    let preserve_comments =
                        self.options.remove_comments != Some(true) || self.preserve_comments;
                    let positions = self.member_operator_positions(expr.span, mem);
                    let obj_end = mem.object.span.end as usize;
                    let mut source_has_newline_before_dot = false;
                    let mut has_comment_before_dot = false;
                    let mut needs_space_before_operator = false;
                    let mut after_dot_text = "";
                    if let Some((dot_start, dot_end, prop_start)) = positions {
                        let before_dot_text = &self.source[obj_end..dot_start];
                        after_dot_text = &self.source[dot_end..prop_start];
                        let line_start = self.source[..dot_start]
                            .rfind('\n')
                            .map(|pos| pos + 1)
                            .unwrap_or(0);
                        source_has_newline_before_dot = line_start > mem.object.span.start as usize
                            && !Self::line_fragment_has_code(&self.source[line_start..dot_start]);
                        has_comment_before_dot =
                            before_dot_text.contains("/*") || before_dot_text.contains("//");
                        if has_comment_before_dot && preserve_comments {
                            let continuation_indent =
                                if source_has_newline_before_dot { 1 } else { 0 };
                            needs_space_before_operator = self.emit_member_gap_before_operator(
                                before_dot_text,
                                continuation_indent,
                            );
                        }
                    }
                    if source_has_newline_before_dot
                        && (!has_comment_before_dot || !preserve_comments)
                    {
                        self.newline();
                        // Write 4 extra spaces so the dot aligns at one indent level
                        // beyond the current self.indent (write() will add indent * 4).
                        self.write("    ");
                    }
                    if needs_space_before_operator {
                        self.write(" ");
                    }
                    if mem.optional {
                        self.write("?.");
                    } else if !source_has_newline_before_dot
                        && !(has_comment_before_dot && preserve_comments)
                        && self.member_object_needs_double_dot(&mem.object)
                    {
                        // Numeric literals like `1` need `1..foo` to avoid
                        // ambiguity with decimal points. Hex/oct/bin literals
                        // like `0xff` end with letters and don't need this.
                        // When there's a newline or comment before the dot,
                        // the `.` is unambiguous, so `..` is not needed.
                        self.write("..");
                    } else {
                        self.write(".");
                    }
                    if preserve_comments {
                        let continuation_indent = if source_has_newline_before_dot { 2 } else { 1 };
                        self.emit_member_gap_after_operator(
                            after_dot_text,
                            continuation_indent,
                            true,
                            true,
                        );
                    }
                    // Skip `<error>` property names from parser error recovery
                    // (incomplete member expressions like `expr.\n`).
                    if mem.property != "<error>" {
                        self.write(&mem.property);
                    }
                }
            }
            ExprKind::ElemAccess(ea) => {
                // Async super hoisting: super["x"] → _superIndex("x") / _superIndex("x").value
                if self.async_super_active
                    && !ea.optional
                    && matches!(&ea.object.kind, ExprKind::Super)
                {
                    let sfx = self.async_super_suffix.clone();
                    self.write("_superIndex");
                    self.write(&sfx);
                    self.write("(");
                    self.emit_expr(&ea.index);
                    if self.async_super_has_write {
                        self.write(").value");
                    } else {
                        self.write(")");
                    }
                    return;
                }
                if !ea.optional && matches!(&ea.object.kind, ExprKind::Super) {
                    if let (Some(base_alias), Some(receiver_alias)) = (
                        self.static_super_base_alias.clone(),
                        self.static_super_receiver_alias.clone(),
                    ) {
                        if self.super_reflect_destructure_target {
                            // Destructuring target: emit setter proxy
                            self.write("({ set value(_a) { Reflect.set(");
                            self.write(&base_alias);
                            self.write(", ");
                            self.emit_expr(&ea.index);
                            self.write(", _a, ");
                            self.write(&receiver_alias);
                            self.write("); } }).value");
                        } else {
                            self.write("Reflect.get(");
                            self.write(&base_alias);
                            self.write(", ");
                            self.emit_expr(&ea.index);
                            self.write(", ");
                            self.write(&receiver_alias);
                            self.write(")");
                        }
                        return;
                    }
                }
                if ea.optional && self.needs_downlevel("optional-chaining") {
                    self.emit_optional_elem_access_downlevel(ea);
                } else if !ea.optional
                    && self.needs_downlevel("optional-chaining")
                    && is_oc_chain(&ea.object)
                {
                    // Non-optional element access continuing an optional chain.
                    // Inline the [prop] into the ternary's truthy branch.
                    self.emit_oc_elem_access_inline(ea, expr);
                } else {
                    let prev_member_ctx = self.in_member_object_context;
                    self.in_member_object_context = true;
                    self.emit_expr(&ea.object);
                    self.in_member_object_context = prev_member_ctx;
                    if ea.optional {
                        self.write("?.");
                    }
                    self.write("[");
                    self.emit_expr(&ea.index);
                    // A comment between the index and `]` that began its own
                    // source line is emitted as a leading comment, and tsc
                    // separates it from the bracket with a space
                    // (`/*3*/ ]`), unlike a trailing comment on the index
                    // line (`/*3*/]`).
                    if self.output.ends_with("*/") {
                        let tail = self.source_between(ea.index.span.end, expr.span.end);
                        let comment_starts_line = tail
                            .find("/*")
                            .map(|i| tail[..i].contains('\n'))
                            .unwrap_or(false);
                        if comment_starts_line {
                            self.write(" ");
                        }
                    }
                    self.write("]");
                }
            }
            ExprKind::Cond(cond) => {
                // Check if `?` is on a new line relative to the test expr.
                // TypeScript preserves multiline layout when `?` / `:` are
                // on separate lines from the test.
                // Detect multiline ternary layout.
                // `q_same_line` = `?` is on the same line as test end:
                //     `test ?\n  consequent`
                // `q_next_line` = `?` is on the next line after test:
                //     `test\n  ? consequent`
                let (q_same_line, q_next_line) = {
                    let test_end = cond.test.span.end as usize;
                    let cons_start = cond.consequent.span.start as usize;
                    if test_end < cons_start && cons_start <= self.source.len() {
                        let gap = &self.source[test_end..cons_start];
                        if let Some(nl_pos) = gap.find('\n') {
                            let before_nl = &gap[..nl_pos];
                            let after_nl = &gap[nl_pos + 1..];
                            (before_nl.contains('?'), after_nl.contains('?'))
                        } else {
                            (false, false)
                        }
                    } else {
                        (false, false)
                    }
                };
                let q_on_new_line = q_same_line || q_next_line;
                let colon_on_new_line = {
                    let cons_end = cond.consequent.span.end as usize;
                    let alt_start = cond.alternate.span.start as usize;
                    cons_end < alt_start
                        && alt_start <= self.source.len()
                        && self.source[cons_end..alt_start].contains('\n')
                };
                self.emit_expr(&cond.test);
                if q_same_line {
                    // `?` stays on the same line as the test: `test ?\n  consequent`
                    self.write(" ?");
                    self.writeln("");
                    self.indent += 1;
                } else if q_next_line {
                    // `?` moves to the next line: `test\n  ? consequent`
                    self.writeln("");
                    self.indent += 1;
                    self.write("? ");
                } else {
                    self.write(" ? ");
                }
                self.suppress_oc_parens = true;
                let prev_suppress_paren = self.suppress_continuation_paren_depth;
                if q_on_new_line || colon_on_new_line {
                    self.suppress_continuation_paren_depth = true;
                }
                self.emit_expr(&cond.consequent);
                // Preserve trailing comment after consequent (e.g. `? x // comment`)
                if colon_on_new_line && self.options.remove_comments != Some(true) {
                    let cons_end = cond.consequent.span.end as usize;
                    let alt_start = cond.alternate.span.start as usize;
                    if cons_end < alt_start && alt_start <= self.source.len() {
                        let gap = &self.source[cons_end..alt_start];
                        if let Some(nl) = gap.find('\n') {
                            let before_nl = gap[..nl].trim_start();
                            // Copy trailing comment text (e.g. `// string`)
                            if before_nl.starts_with("//") {
                                self.write(" ");
                                self.write(before_nl);
                            }
                        }
                    }
                }
                if colon_on_new_line {
                    self.writeln("");
                    if !q_on_new_line {
                        self.indent += 1;
                    }
                    self.write(": ");
                } else {
                    self.write(" : ");
                }
                self.suppress_oc_parens = true;
                self.emit_expr(&cond.alternate);
                self.suppress_continuation_paren_depth = prev_suppress_paren;
                // Preserve trailing comment after alternate when it's the end
                // of a multiline ternary (e.g. `: x == 10; // boolean`)
                if (q_on_new_line || colon_on_new_line)
                    && self.options.remove_comments != Some(true)
                {
                    let alt_end = cond.alternate.span.end as usize;
                    let expr_end = expr.span.end as usize;
                    if alt_end < expr_end && expr_end <= self.source.len() {
                        let after = &self.source[alt_end..expr_end];
                        let trimmed = after.trim_start();
                        if trimmed.starts_with("//") {
                            self.write(" ");
                            self.write(trimmed);
                        }
                    }
                }
                if q_on_new_line {
                    self.indent -= 1;
                } else if colon_on_new_line {
                    self.indent -= 1;
                }
            }
            ExprKind::Binary(bin) => {
                if bin.op == BinaryOp::NullCoal && self.needs_downlevel("nullish-coalescing") {
                    self.emit_nullish_coalescing_downlevel(bin);
                } else if bin.op == BinaryOp::Exp && self.needs_downlevel("exponentiation") {
                    self.write("Math.pow(");
                    // Re-emit leading comments before the left operand inside
                    // Math.pow().  TypeScript duplicates them here, but only
                    // when the left operand is NOT wrapped in parentheses.
                    if !matches!(&bin.left.kind, ExprKind::Paren(_)) {
                        self.emit_exp_operand_comments(bin.left.span.start);
                    }
                    self.emit_expr(&bin.left);
                    self.write(", ");
                    self.emit_expr(&bin.right);
                    self.write(")");
                } else if bin.op == BinaryOp::In
                    && self.needs_downlevel("private-fields")
                    && matches!(&bin.left.kind, ExprKind::Ident(name) if name.starts_with('#'))
                {
                    // Transform `#field in obj` → `__classPrivateFieldIn(_ClassName_field, obj)`
                    if let ExprKind::Ident(name) = &bin.left.kind {
                        let field_name = &name[1..]; // strip #
                        let field_var = if let Some(ref cn) = self.current_class_name {
                            format!("_{}_{}", cn, field_name)
                        } else {
                            format!("_{}", field_name)
                        };
                        self.needs_private_field_in = true;
                        self.write(self.helper_prefix());
                        self.write("__classPrivateFieldIn(");
                        self.write(&field_var);
                        self.write(", ");
                        self.emit_expr(&bin.right);
                        self.write(")");
                    }
                } else {
                    // Check if the source has a newline between the operator
                    // and the right operand.  When it does, preserve the
                    // multi-line layout instead of collapsing onto one line.
                    let (op_on_new_line, operator_starts_line) = {
                        let left_end = bin.left.span.end as usize;
                        let right_start = bin.right.span.start as usize;
                        if left_end < right_start && right_start <= self.source.len() {
                            let gap = &self.source[left_end..right_start];
                            // First, check if the gap contains a newline at all.
                            let saw_newline = gap.contains('\n') || gap.contains('\r');
                            // Then, check if the operator starts on a new line
                            // (i.e., the first non-whitespace after a newline is
                            // the operator token).
                            let mut operator_starts_line = false;
                            if saw_newline {
                                let mut after_newline = false;
                                let mut i = 0usize;
                                while i < gap.len() {
                                    let ch = gap.as_bytes()[i];
                                    if ch == b'\n' || ch == b'\r' {
                                        after_newline = true;
                                        i += 1;
                                        continue;
                                    }
                                    if ch == b' ' || ch == b'\t' {
                                        i += 1;
                                        continue;
                                    }
                                    if after_newline {
                                        operator_starts_line =
                                            gap[i..].starts_with(binary_op_str(bin.op));
                                    }
                                    break;
                                }
                            }
                            (saw_newline, operator_starts_line)
                        } else {
                            (false, false)
                        }
                    };
                    let is_empty_left = matches!(&bin.left.kind, ExprKind::Ident(name) if name.is_empty() || name == "<error>")
                        || matches!(&bin.left.kind, ExprKind::Omitted);
                    self.emit_expr(&bin.left);
                    // When left operand is empty (error recovery), skip
                    // multiline layout — emit operator and right on same line.
                    if operator_starts_line && !is_empty_left {
                        // Check if the right operand is on the same line as
                        // the operator. If so, emit them together on one
                        // continuation line (e.g. `/ notregexp / a.foo()`).
                        // Otherwise keep the 3-line layout with operator on
                        // its own line and RHS double-indented.
                        let right_on_same_line = {
                            let left_end = bin.left.span.end as usize;
                            let right_start = bin.right.span.start as usize;
                            if left_end < right_start && right_start <= self.source.len() {
                                let gap = &self.source[left_end..right_start];
                                // Count newlines: if there's exactly one (before the
                                // operator), right is on the same line as operator
                                gap.chars().filter(|&c| c == '\n').count() <= 1
                            } else {
                                false
                            }
                        };
                        if right_on_same_line {
                            self.writeln("");
                            self.indent += 1;
                            self.write(binary_op_str(bin.op));
                            self.write(" ");
                            self.emit_expr(&bin.right);
                            self.indent -= 1;
                        } else {
                            self.writeln("");
                            self.indent += 1;
                            self.write(binary_op_str(bin.op));
                            self.writeln("");
                            self.indent += 1;
                            self.emit_expr(&bin.right);
                            self.indent -= 2;
                        }
                    } else if op_on_new_line && !is_empty_left {
                        self.write(" ");
                        self.write(binary_op_str(bin.op));
                        // When the right operand is empty (error recovery),
                        // don't add indentation or emit the empty operand.
                        // Just leave the operator at the end of the line.
                        let is_empty_right = matches!(&bin.right.kind, ExprKind::Ident(name) if name.is_empty() || name == "<error>")
                            || matches!(&bin.right.kind, ExprKind::Omitted);
                        if !is_empty_right {
                            self.writeln("");
                            self.indent += 1;
                            self.emit_expr(&bin.right);
                            self.indent -= 1;
                        }
                    } else {
                        // For empty left operand (error recovery), write extra
                        // space to represent the empty expression position.
                        // `var v = || b;` → `var v =  || b;` (two spaces)
                        // But skip space entirely when BOTH operands are empty:
                        // `]/;` → `/;` not ` / ;`
                        let both_empty2 = is_empty_left
                            && matches!(&bin.right.kind, ExprKind::Ident(name) if name.is_empty() || name == "<error>");
                        if !both_empty2 {
                            self.write(" ");
                        }
                        let op_str = binary_op_str(bin.op);
                        self.write(op_str);
                        // Preserve inline block comments between operator and
                        // right operand (e.g. `1 >/**/> 2` → `1 >  /**/ > 2`).
                        // Only look for comments AFTER the operator in the source gap.
                        let left_end = bin.left.span.end as usize;
                        let right_start = bin.right.span.start as usize;
                        let mut emitted_inline_comment = false;
                        if left_end < right_start && right_start <= self.source.len() {
                            let gap = &self.source[left_end..right_start];
                            if let Some(op_pos) = gap.find(op_str) {
                                let after_op = &gap[op_pos + op_str.len()..];
                                if let Some(cs) = after_op.find("/*") {
                                    if let Some(ce) = after_op[cs..].find("*/").map(|p| cs + p + 2)
                                    {
                                        let comment = &after_op[cs..ce];
                                        // Write: trailing space, gap space,
                                        // comment (no trailing space — outer
                                        // binary provides it).
                                        self.write("  ");
                                        self.write(comment);
                                        emitted_inline_comment = true;
                                    }
                                }
                            }
                        }
                        // Skip trailing space only when BOTH left and right are
                        // empty (error recovery: `]/` → `/;` not `/ ;`).
                        // When only right is empty, keep the space (`1 >> ;`).
                        let both_empty = is_empty_left
                            && matches!(&bin.right.kind, ExprKind::Ident(name) if name.is_empty() || name == "<error>");
                        if !emitted_inline_comment && !both_empty {
                            self.write(" ");
                        }
                        self.emit_expr(&bin.right);
                    }
                }
            }
            ExprKind::Unary(un) => {
                let op = unary_op_str(un.op);
                self.write(op);
                let needs_space = un.op == UnaryOp::Typeof
                    || un.op == UnaryOp::Void
                    || un.op == UnaryOp::Delete
                    || matches!(
                        (&un.op, &un.argument.kind),
                        (UnaryOp::Pos, ExprKind::Unary(inner)) if inner.op == UnaryOp::Pos
                    )
                    || matches!(
                        (&un.op, &un.argument.kind),
                        (UnaryOp::Neg, ExprKind::Unary(inner)) if inner.op == UnaryOp::Neg
                    );
                if needs_space {
                    self.write(" ");
                }
                self.emit_expr(&un.argument);
            }
            ExprKind::Update(up) => {
                if self.is_system()
                    && self.rewrite_ident_with_import_map
                    && !self.suppress_system_live_export_wrap
                {
                    if let ExprKind::Ident(name) = &up.argument.kind {
                        if self.system_live_export_names.contains(name.as_str()) {
                            let sys_fn = self.system_exports_fn.clone();
                            self.write(&sys_fn);
                            self.write("(\"");
                            self.write(name);
                            self.write("\", ");
                            let prev_wrap = self.suppress_system_live_export_wrap;
                            self.suppress_system_live_export_wrap = true;
                            match up.op {
                                UpdateOp::PreInc | UpdateOp::PreDec => {
                                    let op = match up.op {
                                        UpdateOp::PreInc => "++",
                                        UpdateOp::PreDec => "--",
                                        _ => "",
                                    };
                                    self.write(op);
                                    self.emit_expr(&up.argument);
                                }
                                UpdateOp::PostInc | UpdateOp::PostDec => {
                                    let op = match up.op {
                                        UpdateOp::PostInc => "++",
                                        UpdateOp::PostDec => "--",
                                        _ => "",
                                    };
                                    self.write("(");
                                    self.emit_expr(&up.argument);
                                    self.write(op);
                                    self.write(", ");
                                    self.emit_expr(&up.argument);
                                    self.write(")");
                                }
                            }
                            self.suppress_system_live_export_wrap = prev_wrap;
                            self.write(")");
                            return;
                        }
                    }
                }
                // CJS live export wrapping:
                //   Statement context:
                //     `++x;`  → `exports.x = ++x;`
                //     `x++;`  → `exports.x = (x++, x);`
                //   Sub-expression context (return value matters):
                //     `y <= ++x`  → `y <= (exports.x = ++x)`
                //     `y <= x++`  → `y <= (exports.x = (_a = x++, x), _a)`
                if !self.suppress_cjs_live_export_wrap {
                    if let ExprKind::Ident(name) = &up.argument.kind {
                        if let Some(chain) = self.cjs_live_export_chain.get(name.as_str()).cloned()
                        {
                            let is_postfix = matches!(up.op, UpdateOp::PostInc | UpdateOp::PostDec);
                            let needs_outer_parens = !cjs_in_expr_stmt;
                            let needs_temp = is_postfix && !cjs_in_expr_stmt;

                            let temp_name = if needs_temp {
                                Some(self.next_cjs_file_level_temp_var())
                            } else {
                                None
                            };

                            // Sub-expression: open outer paren for precedence
                            if needs_outer_parens {
                                self.write("(");
                            }
                            for exported in &chain {
                                self.write_cjs_export_access("exports", exported);
                                self.write(" = ");
                            }
                            let prev = self.suppress_cjs_live_export_wrap;
                            self.suppress_cjs_live_export_wrap = true;
                            match up.op {
                                UpdateOp::PreInc | UpdateOp::PreDec => {
                                    let op_str = match up.op {
                                        UpdateOp::PreInc => "++",
                                        UpdateOp::PreDec => "--",
                                        _ => unreachable!(),
                                    };
                                    self.write(op_str);
                                    self.emit_expr(&up.argument);
                                }
                                UpdateOp::PostInc | UpdateOp::PostDec => {
                                    let op_str = match up.op {
                                        UpdateOp::PostInc => "++",
                                        UpdateOp::PostDec => "--",
                                        _ => unreachable!(),
                                    };
                                    // Inner group: `(_a = x++, x)` or `(x++, x)`
                                    self.write("(");
                                    if let Some(ref temp) = temp_name {
                                        self.write(temp);
                                        self.write(" = ");
                                    }
                                    self.emit_expr(&up.argument);
                                    self.write(op_str);
                                    self.write(", ");
                                    self.emit_expr(&up.argument);
                                    self.write(")");
                                }
                            }
                            // Postfix sub-expression: append `, _a` before outer close
                            if let Some(ref temp) = temp_name {
                                self.write(", ");
                                self.write(temp);
                            }
                            if needs_outer_parens {
                                self.write(")");
                            }
                            self.suppress_cjs_live_export_wrap = prev;
                            return;
                        }
                    }
                }
                // Private field update: ++obj.#field / obj.#field++
                if self.needs_downlevel("private-fields") {
                    if let Some(mem) = unwrap_to_private_member(&up.argument) {
                        let field_name = normalize_unicode_escapes(&mem.property[1..]);
                        if self
                            .current_class_private_fields
                            .contains(field_name.as_str())
                        {
                            self.emit_private_field_update_downlevel(
                                up,
                                mem,
                                &field_name,
                                value_discarded,
                            );
                            return;
                        }
                        if let Some((
                            state_var,
                            kind_str,
                            get_extra_arg,
                            _set_extra_arg,
                            is_static,
                        )) = self.private_member_write_info_for_member(mem)
                        {
                            if kind_str == "\"m\"" {
                                if let Some(get_extra_arg) = get_extra_arg.as_deref() {
                                    self.emit_private_method_update_downlevel(
                                        up,
                                        mem,
                                        &state_var,
                                        get_extra_arg,
                                        is_static,
                                        value_discarded,
                                    );
                                    return;
                                }
                            }
                        }
                    }
                }
                // Super member/element access update in decorated class static context:
                // super.x++ → Reflect.set(_classSuper, "x", (_a = Reflect.get(...), _a++, _a), ...)
                if self.emit_super_reflect_update(up) {
                    return;
                }
                let op = match up.op {
                    UpdateOp::PreInc | UpdateOp::PostInc => "++",
                    UpdateOp::PreDec | UpdateOp::PostDec => "--",
                };
                match up.op {
                    UpdateOp::PreInc | UpdateOp::PreDec => {
                        self.write(op);
                        self.emit_expr(&up.argument);
                    }
                    UpdateOp::PostInc | UpdateOp::PostDec => {
                        self.emit_expr(&up.argument);
                        self.write(op);
                    }
                }
            }
            ExprKind::Paren(inner) => {
                // In JS files, TypeAssertion and Instantiation are NOT type-level
                // constructs, so skip the type-stripping optimisation for them.
                let _is_js_type_like = self.is_js_file
                    && matches!(
                        inner.kind,
                        ExprKind::TypeAssertion(_) | ExprKind::Instantiation(_)
                    );
                if let Some(inside) = inner.kind.type_layer_inner() {
                    let previous_suppression = self.suppress_computed_object_downlevel;
                    self.suppress_computed_object_downlevel |=
                        Self::type_layer_subject_is_object(inner);
                    {
                        // After stripping the type assertion, check if the inner
                        // expression can safely stand without parens. Keep parens
                        // for expressions that would change meaning or cause
                        // syntax errors: new, typeof, unary, function, class,
                        // arrow, object literal, binary, conditional, etc.
                        let stripped = strip_type_layers(inside);
                        let is_safe_primary = matches!(
                            stripped.kind,
                            ExprKind::Ident(_)
                                | ExprKind::Member(_)
                                | ExprKind::ElemAccess(_)
                                | ExprKind::Call(_)
                                | ExprKind::New(_)
                                | ExprKind::ArrayLit(_)
                                | ExprKind::This
                                | ExprKind::Super
                                | ExprKind::Paren(_)
                                | ExprKind::Template(_)
                                | ExprKind::TaggedTemplate(_)
                                | ExprKind::NumLit(_)
                                | ExprKind::BigIntLit(_)
                                | ExprKind::StrLit(_)
                                | ExprKind::BoolLit(_)
                                | ExprKind::NullLit
                                | ExprKind::RegexpLit(_)
                                | ExprKind::NoSubstTemplate(_)
                                | ExprKind::MetaProp(_)
                                | ExprKind::Update(_)
                                | ExprKind::ClassExpr(_)
                                | ExprKind::JsxElement(_)
                                | ExprKind::JsxSelfClosing(_)
                                | ExprKind::JsxFragment(_)
                        ) || (!self.update_value_discarded
                            && !self.in_call_callee_context
                            && !self.in_new_callee_context
                            && matches!(stripped.kind, ExprKind::FnExpr(_)))
                            || (self.in_member_object_context
                                && matches!(stripped.kind, ExprKind::ObjectLit(_)));
                        // `new X` needs parens when used as member access object
                        // because `new X.b` differs from `(new X).b`.
                        // `A()` needs parens when callee of `new`:
                        // `new (A())` creates new of A()'s result, `new A()` creates new A.
                        // Instantiation expressions always get parens from the
                        // Instantiation handler, so Paren(Instantiation(..))
                        // must keep the outer parens to match TypeScript output.
                        // Empty type args (`expr<>`) don't add parens, so the
                        // outer Paren should also be stripped.
                        let is_instantiation = matches!(
                            inner.kind,
                            ExprKind::Instantiation(ref inst) if !inst.type_args.is_empty()
                        );
                        // NonNull is just `!` suffix which gets stripped, so
                        // explicit source parens `(expr!)` should always be
                        // preserved as `(expr)`.
                        let is_non_null = matches!(inner.kind, ExprKind::NonNull(_));
                        let needs_downlevel_rest_parens =
                            self.needs_downlevel("object-spread") && expr_has_object_rest(inside);
                        let needs_parens = !is_safe_primary
                            || is_instantiation
                            || is_non_null
                            || needs_downlevel_rest_parens
                            || (self.in_member_object_context
                                && matches!(stripped.kind, ExprKind::New(_)))
                            || (self.in_new_callee_context
                                && matches!(stripped.kind, ExprKind::Call(_)))
                            || (self.in_export_default_context
                                && matches!(
                                    stripped.kind,
                                    ExprKind::ClassExpr(_) | ExprKind::FnExpr(_)
                                ));
                        // When there are comments between the `(` and the inner
                        // expression, preserve the parens and use source-copy to
                        // keep comment positioning intact.
                        let has_inner_comments =
                            self.has_comments_in_range(expr.span.start + 1, stripped.span.start);
                        if has_inner_comments {
                            let paren_start = expr.span.start as usize;
                            let inner_start = stripped.span.start as usize;
                            let between = &self.source[paren_start + 1..inner_start];
                            // If the paren-to-expression span contains a newline,
                            // the parens may serve as ASI protection (e.g. in
                            // `return (\n/* comment */ expr)`), so preserve them.
                            // Exception: arrow body expressions (`=> expr`) don't
                            // have ASI after `=>`, so multiline parens can be stripped.
                            let multiline = between.contains('\n') && !self.in_arrow_body_expr;
                            if needs_parens || multiline {
                                // Emit `(`, copy leading comments, then emit the
                                // inner expression structurally. This avoids
                                // preserving source-level whitespace between `*/`
                                // and the expression that TypeScript's structured
                                // emit would not include.
                                let last_comment_end =
                                    between.rfind("*/").map(|p| paren_start + 1 + p + 2);
                                if let Some(comment_end) = last_comment_end {
                                    // Copy from `(` through end of last `*/`
                                    let copy_span = Span::new(expr.span.start, comment_end as u32);
                                    self.copy_expr_span(copy_span);
                                    // Add space between `*/` and the expression
                                    // only when multi-line.
                                    let span_text = &self.source[paren_start..comment_end];
                                    if span_text.contains('\n') {
                                        self.write(" ");
                                    }
                                    self.emit_expr_with_discarded_value(inside, value_discarded);
                                    self.write(")");
                                } else {
                                    // Fallback: copy the whole range
                                    let copy_span = Span::new(expr.span.start, stripped.span.end);
                                    self.copy_expr_span(copy_span);
                                    self.write(")");
                                }
                            } else {
                                // Comments present but parens not needed: emit
                                // the comments without surrounding parentheses.
                                let last_block_end =
                                    between.rfind("*/").map(|p| paren_start + 1 + p + 2);
                                let last_line_end = between.rfind("//").and_then(|p| {
                                    let abs = paren_start + 1 + p;
                                    // Find end of the line comment
                                    let rest = &self.source[abs..inner_start];
                                    rest.find('\n').map(|nl| abs + nl)
                                });
                                if let Some(comment_end) = last_block_end {
                                    // Copy from after `(` through end of last `*/`
                                    let copy_span =
                                        Span::new(expr.span.start + 1, comment_end as u32);
                                    self.copy_expr_span(copy_span);
                                    self.write(" ");
                                    self.emit_expr_with_discarded_value(inside, value_discarded);
                                } else if let Some(line_end) = last_line_end {
                                    // Line comment: emit newline + comment + newline
                                    let comment_start =
                                        paren_start + 1 + between.find("//").unwrap();
                                    let comment_text =
                                        self.source[comment_start..line_end].trim_end();
                                    self.newline();
                                    self.write(comment_text);
                                    self.newline();
                                    self.emit_expr_with_discarded_value(inside, value_discarded);
                                } else {
                                    self.emit_expr_with_discarded_value(inside, value_discarded);
                                }
                            }
                        } else if needs_parens {
                            if self.suppress_stmt_paren_inner {
                                // The expression-statement handler already
                                // wrote `(` and will write `)` at the end.
                                self.suppress_stmt_paren_inner = false;
                                self.emit_expr_with_discarded_value(inside, value_discarded);
                            } else {
                                self.write("(");
                                self.emit_expr_with_discarded_value(inside, value_discarded);
                                self.write(")");
                            }
                        } else {
                            self.emit_expr_with_discarded_value(inside, value_discarded);
                        }
                    }
                    self.suppress_computed_object_downlevel = previous_suppression;
                } else if let ExprKind::Paren(inner2) = &inner.kind {
                    // Nested parens: skip the outer layer when the innermost
                    // non-paren expression is a type-erasing construct (the type
                    // erasure handler decides parens).  Otherwise, preserve the
                    // outer paren (TypeScript keeps user-authored paren levels).
                    let innermost = {
                        let mut e = inner2.as_ref();
                        while let ExprKind::Paren(p) = &e.kind {
                            e = p;
                        }
                        e
                    };
                    // In JS files, TypeAssertion/Instantiation are not type
                    // constructs — just emit normal parens.
                    let innermost_is_js_type = self.is_js_file
                        && matches!(
                            innermost.kind,
                            ExprKind::TypeAssertion(_) | ExprKind::Instantiation(_)
                        );
                    if !innermost_is_js_type
                        && matches!(innermost.kind, ExprKind::Instantiation(ref inst) if !inst.type_args.is_empty())
                    {
                        // Instantiation expressions add their own parens when
                        // stripping type args, and the innermost user-paren
                        // deduplicates with that.  Any extra user-paren layers
                        // must be preserved: ((Box<number>)) -> ((Box)).
                        self.write("(");
                        self.emit_expr_with_discarded_value(inner, value_discarded);
                        self.write(")");
                    } else if !innermost_is_js_type
                        && matches!(
                            innermost.kind,
                            ExprKind::TypeAssertion(_)
                                | ExprKind::As(_)
                                | ExprKind::Satisfies(_)
                                | ExprKind::NonNull(_)
                        )
                    {
                        // Type erasure: defer to the inner Paren handler.
                        self.emit_expr_with_discarded_value(inner, value_discarded);
                    } else {
                        self.write("(");
                        self.emit_paren_comments_in_range(expr.span.start + 1, inner.span.start);
                        self.emit_expr_with_discarded_value(inner, value_discarded);
                        self.emit_paren_comments_in_range(inner.span.end, expr.span.end);
                        self.write(")");
                    }
                } else if let ExprKind::ClassExpr(cd) = &inner.kind {
                    // Class expression IIFE already includes its own parens.
                    if !self.use_define_for_class_fields() && class_has_static_initializers(cd) {
                        self.emit_expr_with_discarded_value(inner, value_discarded);
                    } else {
                        self.write("(");
                        self.emit_paren_comments_in_range(expr.span.start + 1, inner.span.start);
                        self.suppress_oc_parens = true;
                        self.emit_expr_with_discarded_value(inner, value_discarded);
                        self.emit_paren_comments_in_range(inner.span.end, expr.span.end);
                        self.write(")");
                    }
                } else if let ExprKind::ObjectLit(props) = &inner.kind {
                    // The computed-object transform supplies the parentheses
                    // required for its comma sequence. Consume this innermost
                    // user-authored layer instead of wrapping the transform a
                    // second time; recursively nested Paren nodes still retain
                    // each of their outer layers.
                    let has_paren_comments = self
                        .has_comments_in_range(expr.span.start + 1, inner.span.start)
                        || self.has_comments_in_range(inner.span.end, expr.span.end);
                    if !has_paren_comments
                        && self.try_emit_computed_object_literal_downlevel_with_wrap(
                            props, inner, true,
                        )
                    {
                        return;
                    }
                    self.write("(");
                    self.emit_paren_comments_in_range(expr.span.start + 1, inner.span.start);
                    self.suppress_oc_parens = true;
                    self.emit_expr_with_discarded_value(inner, value_discarded);
                    self.emit_paren_comments_in_range(inner.span.end, expr.span.end);
                    self.write(")");
                } else {
                    self.write("(");
                    self.emit_paren_comments_in_range(expr.span.start + 1, inner.span.start);
                    self.suppress_oc_parens = true;
                    self.emit_expr_with_discarded_value(inner, value_discarded);
                    self.emit_paren_comments_in_range(inner.span.end, expr.span.end);
                    self.write(")");
                }
            }
            ExprKind::Assign(assign) => {
                if self.is_system()
                    && self.rewrite_ident_with_import_map
                    && !self.suppress_system_live_export_wrap
                {
                    if let ExprKind::Ident(name) = &assign.left.kind {
                        if self.system_live_export_names.contains(name.as_str()) {
                            let sys_fn = self.system_exports_fn.clone();
                            self.write(&sys_fn);
                            self.write("(\"");
                            self.write(name);
                            self.write("\", ");
                            let prev_wrap = self.suppress_system_live_export_wrap;
                            self.suppress_system_live_export_wrap = true;
                            self.emit_assign_expr_core(assign, value_discarded);
                            self.suppress_system_live_export_wrap = prev_wrap;
                            self.write(")");
                            return;
                        }
                    }
                }
                // CJS live export wrapping: `foo = 3` → `exports.foo = foo = 3`
                if !self.suppress_cjs_live_export_wrap {
                    if let ExprKind::Ident(name) = &assign.left.kind {
                        if let Some(chain) = self.cjs_live_export_chain.get(name.as_str()).cloned()
                        {
                            for exported in &chain {
                                self.write_cjs_export_access("exports", exported);
                                self.write(" = ");
                            }
                            let prev = self.suppress_cjs_live_export_wrap;
                            self.suppress_cjs_live_export_wrap = true;
                            self.emit_assign_expr_core(assign, value_discarded);
                            self.suppress_cjs_live_export_wrap = prev;
                            return;
                        }
                    }
                }
                self.emit_assign_expr_core(assign, value_discarded);
            }
            ExprKind::Spread(inner) => {
                self.write("...");
                let dot_end = expr.span.start as usize + 3;
                if !self.emit_compact_spread_comment(dot_end) {
                    self.emit_compact_comment_between(dot_end, inner.span.start as usize, true);
                }
                self.suppress_oc_parens = true;
                self.emit_expr(inner);
            }
            ExprKind::Yield(delegate, arg) => {
                self.write("yield");
                if *delegate {
                    if self.fn_scope_depth > 0 {
                        // Inside function: TypeScript structured emit uses yield* (no space)
                        if arg.is_none() {
                            self.write("* "); // yield*<space> before ;
                        } else {
                            self.write("*"); // yield*expr
                        }
                    } else {
                        // Top level (error recovery): preserve source-like spacing
                        if arg.is_none() {
                            self.write(" * "); // yield * before ;
                        } else {
                            self.write(" *"); // yield * expr
                        }
                    }
                }
                if let Some(ref a) = arg {
                    self.write(" ");
                    self.suppress_oc_parens = true;
                    self.emit_expr(a);
                }
            }
            ExprKind::Await(inner) => {
                // In downlevel async targets, TypeScript emits `yield` for
                // `AwaitExpression` nodes even in parser-recovery non-async
                // function bodies. Also in downleveled static block IIFEs.
                let should_emit_yield_for_await = self.in_static_block_await_to_yield
                    || (self.needs_downlevel("async")
                        && (self.in_async_function || self.fn_scope_depth > 0));
                if matches!(inner.kind, ExprKind::Omitted) {
                    let elided_invalid_type_prefix = {
                        let start = expr.span.start as usize;
                        let end = (expr.span.end as usize).min(self.source.len());
                        start < end && self.source[start..end].contains('<')
                    };
                    if should_emit_yield_for_await {
                        // Downlevel: `await` (no arg) → `yield ` (trailing space matches TypeScript)
                        self.write("yield ");
                    } else if self.in_async_function {
                        // Async context: `await` keyword with no argument — trailing space
                        self.write("await ");
                    } else if elided_invalid_type_prefix {
                        // `await <T, U>` recovery removes `<T` and leaves the
                        // comma expression. Preserve the blank occupied by the
                        // rejected operand: `await , U > ...`.
                        self.write("await ");
                    } else {
                        // Non-async context: `await` used as identifier — no trailing space
                        self.write("await");
                    }
                } else if should_emit_yield_for_await {
                    self.write("yield ");
                    self.suppress_oc_parens = true;
                    self.emit_expr(inner);
                } else {
                    // Check original source: if no space between `await` and next char
                    // (e.g. `await(...)` where await is an imported identifier used as
                    // a function call), preserve the no-space formatting.
                    let await_end = expr.span.start as usize + 5; // "await".len()
                    let has_space =
                        await_end >= self.source.len() || self.source.as_bytes()[await_end] != b'(';
                    if has_space {
                        self.write("await ");
                    } else {
                        self.write("await");
                    }
                    // UMD dynamic import() produces a ternary (__syncRequire ? ... : ...)
                    // which has lower precedence than `await`, so it needs wrapping parens.
                    let needs_umd_parens = self.is_umd()
                        && self.should_downlevel_dynamic_import()
                        && matches!(&inner.kind, ExprKind::Call(call) if self.is_dynamic_import_call(call));
                    if needs_umd_parens {
                        self.write("(");
                    }
                    self.suppress_oc_parens = true;
                    self.emit_expr(inner);
                    if needs_umd_parens {
                        self.write(")");
                    }
                }
            }
            ExprKind::Delete(inner) => {
                // When deleting an optional chain that needs downlevel transform,
                // `delete o?.b` → `o === null || o === void 0 ? true : delete o.b`
                // (not `delete o === null || ...` which would delete the comparison result).
                // Also handles `delete (o?.b)` where the OC is inside parens.
                let unwrapped = {
                    let mut e = inner.as_ref();
                    while let ExprKind::Paren(p) = &e.kind {
                        e = p.as_ref();
                    }
                    e
                };
                let inner_is_oc = match &unwrapped.kind {
                    ExprKind::Member(mem) => {
                        (mem.optional || (!mem.optional && is_oc_member_chain(&mem.object)))
                            && self.needs_downlevel("optional-chaining")
                    }
                    ExprKind::ElemAccess(ea) => {
                        ea.optional && self.needs_downlevel("optional-chaining")
                    }
                    ExprKind::Call(call) => {
                        call.optional && self.needs_downlevel("optional-chaining")
                    }
                    _ => false,
                };
                if inner_is_oc {
                    self.oc_delete_mode = true;
                    self.suppress_oc_parens = true;
                    // If the OC was wrapped in parens, preserve outer parens around the
                    // downlevel ternary: `delete (o?.b)` → `(o === null || ... ? true : delete o.b)`
                    let has_paren = !std::ptr::eq(inner.as_ref(), unwrapped);
                    if has_paren {
                        self.write("(");
                    }
                    self.emit_expr(unwrapped);
                    if has_paren {
                        self.write(")");
                    }
                    self.oc_delete_mode = false;
                } else {
                    self.write("delete ");
                    self.suppress_oc_parens = true;
                    self.emit_expr(inner);
                }
            }
            ExprKind::Typeof(inner) => {
                self.write("typeof ");
                self.emit_expr(inner);
            }
            ExprKind::Void(inner) => {
                self.write("void ");
                if !matches!(&inner.kind, ExprKind::Ident(n) if n == "<error>") {
                    self.suppress_oc_parens = true;
                    self.emit_expr(inner);
                }
            }
            ExprKind::As(as_expr) => {
                if self.preserve_type_annotations {
                    self.emit_expr_with_computed_object_downlevel_suppressed(&as_expr.expr);
                    self.write(" as ");
                    let start = as_expr.type_node.span.start as usize;
                    let end = as_expr.type_node.span.end as usize;
                    if end <= self.source.len() {
                        self.write(&self.source[start..end]);
                    }
                } else {
                    self.emit_expr_with_computed_object_downlevel_suppressed(&as_expr.expr);
                }
            }
            ExprKind::TypeAssertion(ta_expr) => {
                if self.preserve_type_annotations || self.is_js_file {
                    // In JS files, `<Type>expr` is not a TS type assertion — preserve it.
                    self.write("<");
                    let start = ta_expr.type_node.span.start as usize;
                    let end = ta_expr.type_node.span.end as usize;
                    if end <= self.source.len() {
                        self.write(&self.source[start..end]);
                    }
                    self.write(">");
                    self.emit_expr_with_computed_object_downlevel_suppressed(&ta_expr.expr)
                } else if self.in_new_callee_context {
                    // Recovery: `new <T> expr` is not a normal erasable type assertion.
                    // TypeScript preserves the assertion text in callee position.
                    self.write(" < ");
                    self.emit_jsdoc_recovery_type_node(&ta_expr.type_node);
                    self.write(" > ");
                    self.emit_expr_with_computed_object_downlevel_suppressed(&ta_expr.expr);
                } else {
                    self.emit_expr_with_computed_object_downlevel_suppressed(&ta_expr.expr);
                }
            }
            ExprKind::Instantiation(inst_expr) => {
                if self.is_js_file {
                    // In JS files, `expr<T>` is not an instantiation expression —
                    // it's binary `<` and `>` operators.
                    self.emit_expr_with_computed_object_downlevel_suppressed(&inst_expr.expr);
                    self.emit_js_type_args_as_binary(&inst_expr.type_args);
                } else if type_args_need_jsdoc_recovery(&inst_expr.type_args) {
                    self.emit_expr_with_computed_object_downlevel_suppressed(&inst_expr.expr);
                    self.emit_jsdoc_recovery_type_args(&inst_expr.type_args);
                } else if self.in_member_object_context || inst_expr.type_args.is_empty() {
                    // Instantiation expressions: `expr<T>` -> emit inst_expr.expr wrapped in parens
                    // TypeScript always wraps the inst_expr.expr expression in parentheses when
                    // stripping the type arguments, e.g. `obj.fn<number>` -> `(obj.fn)`
                    // However, when used as the object of a member/call expression,
                    // the parens are not needed (continuation provides grouping).
                    // When type args are empty (e.g. `fx<>`), TypeScript strips
                    // the type args entirely and emits the inst_expr.expr without parens.
                    self.emit_expr_with_computed_object_downlevel_suppressed(&inst_expr.expr);
                } else {
                    self.write("(");
                    // Suppress optional chain parens — we already provide outer parens.
                    self.suppress_oc_parens = true;
                    self.emit_expr_with_computed_object_downlevel_suppressed(&inst_expr.expr);
                    self.write(")");
                }
            }
            ExprKind::NonNull(inner) => {
                if self.preserve_type_annotations {
                    self.emit_expr_with_computed_object_downlevel_suppressed(inner);
                    self.write("!");
                } else {
                    self.emit_expr_with_computed_object_downlevel_suppressed(inner);
                }
            }
            ExprKind::Satisfies(sat_expr) => {
                if self.preserve_type_annotations {
                    self.emit_expr_with_computed_object_downlevel_suppressed(&sat_expr.expr);
                    self.write(" satisfies ");
                    let start = sat_expr.type_node.span.start as usize;
                    let end = sat_expr.type_node.span.end as usize;
                    if end <= self.source.len() {
                        self.write(&self.source[start..end]);
                    }
                } else {
                    self.emit_expr_with_computed_object_downlevel_suppressed(&sat_expr.expr);
                }
            }
            ExprKind::This => {
                if let Some(alias) = self.lexical_arrow_this_alias.clone() {
                    self.write(&alias);
                } else if let Some(alias) = self.static_this_alias.clone() {
                    self.write(&alias);
                } else {
                    self.write("this");
                }
            }
            ExprKind::Super => {
                // In static field initializers, bare `super` is an error.
                // TypeScript emits `Reflect.get(base, "", receiver)`.
                if let (Some(base_alias), Some(receiver_alias)) = (
                    self.static_super_base_alias.clone(),
                    self.static_super_receiver_alias.clone(),
                ) {
                    self.write("Reflect.get(");
                    self.write(&base_alias);
                    self.write(", \"\", ");
                    self.write(&receiver_alias);
                    self.write(")");
                } else {
                    self.write("super");
                }
            }
            ExprKind::MetaProp(mp) => {
                if mp.meta.as_str() == "new" && mp.property.as_str() == "target" {
                    if let Some(loop_id) =
                        self.active_lexical_loop_helpers
                            .iter()
                            .copied()
                            .find(|loop_id| {
                                self.lexical_downlevel_plan.loops[*loop_id]
                                    .new_target_references
                                    .contains(&expr.span.into())
                            })
                    {
                        let plan = &self.lexical_downlevel_plan.loops[loop_id];
                        if let Some(name) = plan.new_target_name.clone() {
                            self.write(&name);
                            return;
                        }
                    }
                }
                self.write(&mp.meta);
                self.write(".");
                self.write(&mp.property);
            }
            ExprKind::Comma(exprs) => {
                if self.emit_resumable_object_sequence(expr, exprs) {
                    return;
                }
                for (i, e) in exprs.iter().enumerate() {
                    if i > 0 {
                        // Check source: if the next expression starts on a new line
                        // relative to the previous expression's end, preserve the
                        // line break. Otherwise keep on the same line.
                        let prev_end = exprs[i - 1].span.end as usize;
                        let cur_start = e.span.start as usize;
                        let has_newline_between = prev_end < cur_start
                            && cur_start <= self.source.len()
                            && self.source[prev_end..cur_start].contains('\n');
                        if has_newline_between {
                            let source_between = &self.source[prev_end..cur_start];
                            if source_between.contains(',') {
                                self.writeln(",");
                                self.indent += 1;
                                self.emit_expr(e);
                                self.indent -= 1;
                            } else {
                                self.newline();
                                self.indent += 1;
                                self.writeln(",");
                                self.indent += 1;
                                self.emit_expr(e);
                                self.indent -= 2;
                            }
                        } else {
                            let between = if prev_end < cur_start && cur_start <= self.source.len()
                            {
                                &self.source[prev_end..cur_start]
                            } else {
                                ""
                            };
                            let block_comment = between.find("/*").and_then(|start| {
                                between[start + 2..]
                                    .find("*/")
                                    .map(|end| &between[start..start + 2 + end + 2])
                            });
                            if let Some(comment) = block_comment {
                                self.write(" ");
                                self.write(comment);
                                self.write(", ");
                                self.write(comment);
                                self.write(" ");
                            } else {
                                self.write(", ");
                            }
                            self.emit_expr(e);
                        }
                    } else {
                        self.emit_expr(e);
                    }
                }
            }
            ExprKind::Omitted => {}
            ExprKind::JsxElement(el) => {
                if self.jsx_is_preserve() {
                    let mut reps = Vec::new();
                    // Strip block comments between `<` and the tag name:
                    // `</**/div>` → `<div>`
                    {
                        let es = expr.span.start as usize;
                        let ns = el.name.span.start as usize;
                        if es + 1 < ns && ns <= self.source.len() {
                            let between = &self.source[es + 1..ns];
                            // Check for block comment `/* ... */` possibly with spaces
                            let trimmed = between.trim();
                            if trimmed.starts_with("/*") && trimmed.ends_with("*/") {
                                reps.push((es + 1, ns, String::new()));
                            }
                        }
                    }
                    // JSX namespace name normalization:
                    // 1. Strip spaces around `:` and `.` in the tag name span
                    // 2. If there is a second `:` right after the parsed name span,
                    //    replace it with a space so the leftover token becomes a boolean attribute.
                    if let ExprKind::Ident(n) = &el.name.kind {
                        // Normalize spaces around `:` or `.` in the opening tag name
                        let ns = el.name.span.start as usize;
                        let ne = el.name.span.end as usize;
                        if ns < ne && ne <= self.source.len() {
                            let name_src = &self.source[ns..ne];
                            if (name_src.contains(':') || name_src.contains('.'))
                                && (name_src.contains(' '))
                            {
                                let normalized = name_src
                                    .replace(" : ", ":")
                                    .replace(": ", ":")
                                    .replace(" :", ":")
                                    .replace(" . ", ".")
                                    .replace(". ", ".")
                                    .replace(" .", ".");
                                if normalized != name_src {
                                    reps.push((ns, ne, normalized));
                                }
                            }
                        }
                        if n.contains(':') {
                            if ne < self.source.len() && self.source.as_bytes()[ne] == b':' {
                                reps.push((ne, ne + 1, " ".to_string()));
                            }
                        }
                        // Namespace export qualification: `<X>` → `<M.X>` inside namespace IIFE
                        if self.export_target.as_ref().is_some_and(|t| t != "exports")
                            && !self.cjs_param_shadows.contains(n.as_str())
                        {
                            let qual_target = if self.namespace_exports.contains(n.as_str()) {
                                self.export_target.clone()
                            } else if !self.ns_local_bindings.contains(n.as_str()) {
                                self.ns_export_stack
                                    .iter()
                                    .rev()
                                    .find(|(_, exports)| exports.contains(n.as_str()))
                                    .map(|(t, _)| t.to_string())
                            } else {
                                None
                            };
                            if let Some(target) = qual_target {
                                // Locate close-tag span first so we know whether
                                // the second push is needed before allocating a
                                // second copy of the qualified name.
                                let close_target: Option<(usize, usize)> = {
                                    let s = expr.span.start as usize;
                                    let e = (expr.span.end as usize).min(self.source.len());
                                    let elem_text = &self.source[s..e];
                                    elem_text.rfind("</").and_then(|close_pos| {
                                        let close_name_start = s + close_pos + 2;
                                        let tail = &self.source[close_name_start..e];
                                        let close_gt = tail.find('>')?;
                                        let close_name_end = close_name_start + close_gt;
                                        let close_name =
                                            self.source[close_name_start..close_name_end].trim();
                                        if close_name == n.as_str() {
                                            Some((close_name_start, close_name_end))
                                        } else {
                                            None
                                        }
                                    })
                                };
                                let qualified = format!("{}.{}", target, n);
                                if let Some((cs, ce)) = close_target {
                                    reps.push((cs, ce, qualified.clone()));
                                }
                                reps.push((ns, ne, qualified));
                            }
                        }
                    }
                    // Also handle member-expression names like `A.B.C.D`
                    if let ExprKind::Member(mem) = &el.name.kind {
                        let ns = el.name.span.start as usize;
                        let ne = el.name.span.end as usize;
                        if ns < ne && ne <= self.source.len() {
                            let name_src = &self.source[ns..ne];
                            if name_src.contains('.') && name_src.contains(' ') {
                                let normalized = name_src
                                    .replace(" . ", ".")
                                    .replace(". ", ".")
                                    .replace(" .", ".");
                                if normalized != name_src {
                                    reps.push((ns, ne, normalized));
                                }
                            }
                        }
                        let mut _unused = false;
                        self.collect_jsx_private_field_replacement(
                            mem,
                            expr.span,
                            &mut reps,
                            &mut _unused,
                        );
                        // Namespace qualification for member JSX names:
                        // `<S.Bar />` → `<M.S.Bar />` when `S` is a namespace export
                        self.collect_jsx_member_ns_qualification(mem, &mut reps);
                    }
                    if self.is_js_file {
                        collect_jsx_js_file_type_arg_replacements(
                            el.type_args.as_ref(),
                            self.source,
                            &mut reps,
                        );
                    } else {
                        collect_jsx_type_strip_replacements(
                            el.type_args.as_ref(),
                            &el.attributes,
                            &el.children,
                            self.source,
                            &mut reps,
                        );
                    }
                    self.collect_expr_cjs_import_replacements(&el.name, &mut reps);
                    self.collect_jsx_cjs_import_replacements(&el.attributes, &mut reps);
                    self.collect_invalid_jsx_attribute_replacements(
                        el.name.span.end as usize,
                        &el.attributes,
                        &mut reps,
                    );
                    self.collect_jsx_preserve_multiline_expr_replacements(&el.children, &mut reps);
                    self.collect_jsx_child_line_comment_replacements(&el.children, &mut reps);
                    self.collect_recovered_jsx_self_close_replacements(
                        &el.children,
                        match &el.name.kind {
                            ExprKind::Ident(name) => Some(name.as_str()),
                            _ => None,
                        },
                        &mut reps,
                    );
                    // Normalize empty JSX expression children. TypeScript
                    // removes source `{}`, but closes a recovered lone `{`
                    // before a closing tag: `<a>{ </a>` -> `<a>{} </a>`.
                    for child in &el.children {
                        if let JsxChild::Expression(None, span) = child {
                            let cs = span.start as usize;
                            let ce = (span.end as usize).min(self.source.len());
                            if cs < ce {
                                let inner = &self.source[cs..ce];
                                let trimmed = inner.trim();
                                if trimmed == "{}" {
                                    reps.push((cs, ce, String::new()));
                                } else if trimmed == "{" {
                                    reps.push((ce, ce, "}".to_string()));
                                }
                            }
                        } else if let JsxChild::Expression(Some(_), span) = child {
                            let cs = span.start as usize;
                            let search_end = (expr.span.end as usize).min(self.source.len());
                            if cs < search_end {
                                if let Some(offset) = self.source[cs..search_end].find(";}") {
                                    let semi = cs + offset;
                                    reps.push((semi, semi + 2, "};}".to_string()));
                                }
                            }
                        }
                    }
                    // Strip trailing space before `>` in the opening tag:
                    // `<Comp foo >` → `<Comp foo>`, `<div >` → `<div>`.
                    {
                        // Find the `>` of the opening tag by scanning after the
                        // last attribute's span end (or tag name end if no attrs).
                        let search_start =
                            el.attributes
                                .last()
                                .map_or(el.name.span.end as usize, |a| match a {
                                    JsxAttribute::Normal { span, .. } => span.end as usize,
                                    JsxAttribute::Spread(_, span) => span.end as usize,
                                });
                        let search_end = (expr.span.end as usize).min(self.source.len());
                        if let Some(gt_offset) = self.source[search_start..search_end].find('>') {
                            let gt_pos = search_start + gt_offset;
                            // Only strip if this is the opening tag `>`, not a `>`
                            // inside children or closing tag. Check that it's before
                            // the first child or the closing `</`.
                            let is_opening_gt = el.children.first().map_or(true, |c| {
                                let child_start = match c {
                                    JsxChild::Expression(_, span) => span.start as usize,
                                    JsxChild::Element(e) => e.span.start as usize,
                                    JsxChild::Text(_, span) => span.start as usize,
                                    JsxChild::Fragment(f) => {
                                        // Fragment doesn't have its own span, use its children
                                        f.children.first().map_or(expr.span.end as usize, |fc| {
                                            match fc {
                                                JsxChild::Text(_, s) => s.start as usize,
                                                JsxChild::Element(e) => e.span.start as usize,
                                                _ => expr.span.end as usize,
                                            }
                                        })
                                    }
                                };
                                gt_pos < child_start
                            });
                            if is_opening_gt && gt_pos > 0 {
                                let mut sp = gt_pos;
                                while sp > search_start && self.source.as_bytes()[sp - 1] == b' ' {
                                    sp -= 1;
                                }
                                if sp < gt_pos {
                                    reps.push((sp, gt_pos, String::new()));
                                }
                            }
                        }
                    }
                    // Normalize closing tag: strip spaces around `.` and `:` in
                    // the closing tag name. `</A . B . C.D>` → `</A.B.C.D>`
                    {
                        let s = expr.span.start as usize;
                        let e = (expr.span.end as usize).min(self.source.len());
                        let elem_text = &self.source[s..e];
                        if let Some(close_pos) = elem_text.rfind("</") {
                            let close_start = s + close_pos + 2;
                            if let Some(close_gt) = self.source[close_start..e].find('>') {
                                let tag_end = close_start + close_gt;
                                let tag_name_src = &self.source[close_start..tag_end];
                                if tag_name_src.contains(" . ")
                                    || tag_name_src.contains(" : ")
                                    || tag_name_src.contains(". ")
                                    || tag_name_src.contains(" .")
                                    || tag_name_src.contains(": ")
                                    || tag_name_src.contains(" :")
                                {
                                    let normalized = tag_name_src
                                        .replace(" . ", ".")
                                        .replace(". ", ".")
                                        .replace(" .", ".")
                                        .replace(" : ", ":")
                                        .replace(": ", ":")
                                        .replace(" :", ":");
                                    reps.push((close_start, tag_end, normalized));
                                }
                            }
                        }
                    }
                    // Add missing semicolons inside arrow/function block bodies
                    self.collect_missing_semi_replacements_for_jsx_attrs(&el.attributes, &mut reps);
                    // Always use the replacement path for JSX elements to ensure
                    // consistent normalization (space collapsing, etc.)
                    let output_start = self.output.len();
                    self.emit_jsx_preserve_with_replacements(expr.span, &reps);
                    // An unclosed nested tag can make the JSX recovery node
                    // span across later top-level declarations. Those bytes
                    // are retained deliberately, but the structured printer
                    // does not preserve blank separator lines between them.
                    {
                        let start = expr.span.start as usize;
                        let end = (expr.span.end as usize).min(self.source.len());
                        if start < end && self.source[start..end].contains("\nfunction ") {
                            let segment = self.output[output_start..].replace("\n\n", "\n");
                            self.output.truncate(output_start);
                            self.output.push_str(&segment);
                        }
                    }
                    // Compact multiline opening tags into a single line.
                    // `<AbC_def\n  test="...">` → `<AbC_def test="...">`
                    if !el.attributes.is_empty() {
                        let s = expr.span.start as usize;
                        let e = (expr.span.end as usize).min(self.source.len());
                        if s < e {
                            // Find the opening tag `>` position in the source
                            let open_end = find_jsx_opening_gt(&self.source[s..e]);
                            if let Some(gt_off) = open_end {
                                let open_src = &self.source[s..s + gt_off + 1];
                                if open_src.contains('\n') {
                                    // Compact the opening tag portion in the output
                                    let output_segment = self.output[output_start..].to_string();
                                    if let Some(out_gt) = find_jsx_opening_gt(&output_segment) {
                                        let tag_part = &output_segment[..=out_gt];
                                        let compacted = if open_src.contains("/*") {
                                            normalize_jsx_opening_block_comment_layout(tag_part)
                                        } else {
                                            compact_multiline_jsx_self_closing_layout(tag_part)
                                        };
                                        let compacted = if open_src.contains("//")
                                            && !open_src.contains("/*")
                                        {
                                            normalize_multiline_jsx_attribute_expr_layout(
                                                &compacted,
                                            )
                                        } else {
                                            compacted
                                        };
                                        if compacted != tag_part {
                                            let rest = output_segment[out_gt + 1..].to_string();
                                            self.output.truncate(output_start);
                                            self.output.push_str(&compacted);
                                            self.output.push_str(&rest);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    // Recovery for malformed closing namespace tags:
                    // parser may stop at `</a:ele` with remaining source `:ment>`.
                    // In preserve mode TypeScript keeps the recovered `>` before
                    // the outer `, ment` declarator recovery.
                    {
                        let e = expr.span.end as usize;
                        if e < self.source.len()
                            && self.source.as_bytes()[e] == b':'
                            && self
                                .source
                                .get(expr.span.start as usize..e)
                                .is_some_and(|src| src.contains("</"))
                        {
                            self.write(">");
                        }
                    }
                    // If JSX elements are unterminated in the source, append
                    // one recovery close per open element. At EOF TypeScript
                    // puts the synthetic closes after a blank line.
                    {
                        let s = expr.span.start as usize;
                        let e = expr.span.end as usize;
                        if e <= self.source.len() {
                            let missing_closes = self.count_unclosed_jsx_elements(expr);
                            if missing_closes == 0 {
                                // All opening elements have a corresponding
                                // closing tag in the recovery tree.
                            } else if self.source[e..].trim().is_empty() {
                                let has_any_source_close = self.source[s..e].contains("</");
                                if has_any_source_close {
                                    if !self.output.ends_with('\n') {
                                        self.newline();
                                    }
                                } else if self.output.ends_with('\n') {
                                    self.newline();
                                } else {
                                    self.newline();
                                    self.newline();
                                }
                                for _ in 0..missing_closes {
                                    self.write("</>");
                                }
                            } else {
                                for _ in 0..missing_closes {
                                    self.write("</>");
                                }
                            }
                        }
                    }
                    // Collapse multiline JSX expression containers to single
                    // lines (post-processing). TypeScript compacts containers
                    // like `{ user => (\n  <h1>...</h1>\n) }` to
                    // `{user => (<h1>...</h1>)}` when no comments are present.
                    {
                        let segment = self.output[output_start..].to_string();
                        if let Some(compacted) = collapse_jsx_expression_containers(&segment) {
                            self.output.truncate(output_start);
                            self.output.push_str(&compacted);
                        }
                    }
                    {
                        let segment = self.output[output_start..].to_string();
                        let normalized =
                            normalize_nested_jsx_self_closing_line_comment_layout(&segment);
                        if normalized != segment {
                            self.output.truncate(output_start);
                            self.output.push_str(&normalized);
                        }
                    }
                } else if self.jsx_is_react_jsx() {
                    self.emit_jsx_element_react_jsx(el, expr.span);
                } else {
                    self.emit_jsx_element(el);
                    // Preserve trailing line comment after JSX element (e.g. `</li> // comment`)
                    // Only when the JSX element is inside parens (nested expression), so the
                    // comment belongs to the expression, not the enclosing statement.
                    let span_start = expr.span.start as usize;
                    let span_end = expr.span.end as usize;
                    let in_parens = span_start > 0
                        && self.source[..span_start]
                            .bytes()
                            .rev()
                            .find(|&b| b != b' ' && b != b'\t' && b != b'\n' && b != b'\r')
                            .is_some_and(|b| b == b'(');
                    if in_parens && span_end < self.source.len() {
                        let line_end = self.source[span_end..]
                            .find('\n')
                            .map(|p| span_end + p)
                            .unwrap_or(self.source.len());
                        if let Some(comment) =
                            find_line_comment_in_range(self.source, span_end, line_end)
                        {
                            self.write(" ");
                            self.write(comment);
                            if let Some(comment_start_rel) =
                                self.source[span_end..line_end].find("//")
                            {
                                self.advance_comment_pos(
                                    (span_end + comment_start_rel + comment.len()) as u32,
                                );
                            }
                            self.newline();
                        }
                    }
                }
            }
            ExprKind::JsxSelfClosing(el) => {
                if self.jsx_is_preserve() {
                    let mut reps = Vec::new();
                    let mut stripped_private_jsx_name = false;
                    // JSX namespace/dot normalization for self-closing elements.
                    if let ExprKind::Ident(n) = &el.name.kind {
                        // Normalize spaces around `:` or `.` in the tag name
                        let ns = el.name.span.start as usize;
                        let ne = el.name.span.end as usize;
                        if ns < ne && ne <= self.source.len() {
                            let name_src = &self.source[ns..ne];
                            if (name_src.contains(':') || name_src.contains('.'))
                                && name_src.contains(' ')
                            {
                                let normalized = name_src
                                    .replace(" : ", ":")
                                    .replace(": ", ":")
                                    .replace(" :", ":")
                                    .replace(" . ", ".")
                                    .replace(". ", ".")
                                    .replace(" .", ".");
                                if normalized != name_src {
                                    reps.push((ns, ne, normalized));
                                }
                            }
                        }
                        // Double-colon fix: If there is a second `:` right after
                        // the parsed name span, replace it with a space.
                        if n.contains(':') {
                            if ne < self.source.len() && self.source.as_bytes()[ne] == b':' {
                                reps.push((ne, ne + 1, " ".to_string()));
                                // Strip trailing space before `/>`:
                                let ee = expr.span.end as usize;
                                if ee >= 2 && ee <= self.source.len() {
                                    let slash_pos = ee - 2;
                                    if slash_pos > 0
                                        && self.source.as_bytes()[slash_pos] == b'/'
                                        && self.source.as_bytes()[ee - 1] == b'>'
                                    {
                                        let mut sp = slash_pos;
                                        while sp > 0 && self.source.as_bytes()[sp - 1] == b' ' {
                                            sp -= 1;
                                        }
                                        if sp < slash_pos {
                                            reps.push((sp, slash_pos, String::new()));
                                        }
                                    }
                                }
                            }
                        }
                        // Namespace export qualification: `<X />` → `<M.X />` inside namespace IIFE
                        if self.export_target.as_ref().is_some_and(|t| t != "exports")
                            && !self.cjs_param_shadows.contains(n.as_str())
                        {
                            let qual_target = if self.namespace_exports.contains(n.as_str()) {
                                self.export_target.clone()
                            } else if !self.ns_local_bindings.contains(n.as_str()) {
                                self.ns_export_stack
                                    .iter()
                                    .rev()
                                    .find(|(_, exports)| exports.contains(n.as_str()))
                                    .map(|(t, _)| t.to_string())
                            } else {
                                None
                            };
                            if let Some(target) = qual_target {
                                let qualified = format!("{}.{}", target, n);
                                reps.push((ns, ne, qualified));
                            }
                        }
                    }
                    // Also handle member-expression names like `A.B.C.D`
                    if let ExprKind::Member(mem) = &el.name.kind {
                        let ns = el.name.span.start as usize;
                        let ne = el.name.span.end as usize;
                        if ns < ne && ne <= self.source.len() {
                            let name_src = &self.source[ns..ne];
                            if name_src.contains('.') && name_src.contains(' ') {
                                let normalized = name_src
                                    .replace(" . ", ".")
                                    .replace(". ", ".")
                                    .replace(" .", ".");
                                if normalized != name_src {
                                    reps.push((ns, ne, normalized));
                                }
                            }
                        }
                        self.collect_jsx_private_field_replacement(
                            mem,
                            expr.span,
                            &mut reps,
                            &mut stripped_private_jsx_name,
                        );
                        // Namespace qualification for member JSX names
                        self.collect_jsx_member_ns_qualification(mem, &mut reps);
                    }
                    if self.is_js_file {
                        collect_jsx_js_file_type_arg_replacements(
                            el.type_args.as_ref(),
                            self.source,
                            &mut reps,
                        );
                    } else {
                        collect_jsx_type_strip_replacements(
                            el.type_args.as_ref(),
                            &el.attributes,
                            &[],
                            self.source,
                            &mut reps,
                        );
                    }
                    self.collect_expr_cjs_import_replacements(&el.name, &mut reps);
                    self.collect_jsx_cjs_import_replacements(&el.attributes, &mut reps);
                    self.collect_invalid_jsx_attribute_replacements(
                        el.name.span.end as usize,
                        &el.attributes,
                        &mut reps,
                    );
                    // Recovery: malformed namespace tag names can leave `=`
                    // before a recovered spread attribute (`<a: attr={"x"} />`).
                    // Preserve-mode text copy needs explicit replacements to
                    // produce `<a:attr {..."x"}/>` like TypeScript.
                    if let Some(JsxAttribute::Spread(_, spread_span)) = el.attributes.first() {
                        let name_end = el.name.span.end as usize;
                        let spread_start = spread_span.start as usize;
                        if name_end < spread_start && spread_start <= self.source.len() {
                            let between = &self.source[name_end..spread_start];
                            if let Some(eq_rel) = between.find('=') {
                                let eq_abs = name_end + eq_rel;
                                reps.push((eq_abs, spread_start, " ".to_string()));
                                if spread_start + 1 <= self.source.len()
                                    && self
                                        .source
                                        .as_bytes()
                                        .get(spread_start + 1)
                                        .is_some_and(|b| *b != b'.')
                                {
                                    reps.push((spread_start + 1, spread_start + 1, "...".into()));
                                }
                            }
                        }
                    }
                    // In TS files, type arguments are erased and never reach the
                    // output, so `<Tag<T> />` with no attributes emits as
                    // `<Tag />` (space, no attrs). In JS files, type-args are
                    // recovered as a trailing comma-expression, so keep treating
                    // them as content there.
                    let no_attrs =
                        el.attributes.is_empty() && (!self.is_js_file || el.type_args.is_none());
                    let has_attrs = !no_attrs;
                    // When there ARE attributes, strip space before `/>`.
                    // TypeScript normalizes `<Foo bar />` → `<Foo bar/>`.
                    // Skip when we stripped a private field name — the parser
                    // error-recovery creates a phantom attribute from `#name`.
                    if has_attrs && !stripped_private_jsx_name {
                        let ee = expr.span.end as usize;
                        if ee >= 2
                            && ee <= self.source.len()
                            && self.source.as_bytes()[ee - 2] == b'/'
                            && self.source.as_bytes()[ee - 1] == b'>'
                        {
                            let slash_pos = ee - 2;
                            let mut sp = slash_pos;
                            while sp > 0 && self.source.as_bytes()[sp - 1] == b' ' {
                                sp -= 1;
                            }
                            if sp < slash_pos {
                                reps.push((sp, slash_pos, String::new()));
                            }
                        }
                    }
                    // Add missing semicolons inside arrow/function block bodies
                    // within JSX attributes (JSX preserve copies source text, so
                    // we need to insert them as replacements).
                    self.collect_missing_semi_replacements_for_jsx_attrs(&el.attributes, &mut reps);
                    // Always use the replacement path for consistent normalization
                    let output_start = self.output.len();
                    self.emit_jsx_preserve_with_replacements(expr.span, &reps);
                    // Complete malformed self-closing tags recovered at a
                    // following JSX tag. The parser deliberately leaves that
                    // next `<` unconsumed, so the preserved span may end at
                    // `<Tag`, `<Tag attrs`, or `<Tag/`.
                    {
                        let emitted = self.output[output_start..].trim_end();
                        if emitted.ends_with('/') && !emitted.ends_with("/>") {
                            self.write(">");
                        } else if !emitted.ends_with("/>") {
                            if no_attrs {
                                self.write(" />");
                            } else {
                                self.write("/>");
                            }
                        }
                    }
                    // TypeScript flattens many multi-line self-closing tags into
                    // one line in preserve mode. Keep comments as-is.
                    {
                        let s = expr.span.start as usize;
                        let e = (expr.span.end as usize).min(self.source.len());
                        if s < e {
                            let src = &self.source[s..e];
                            if src.contains('\n') {
                                let segment = self.output[output_start..].to_string();
                                let compacted = if src.contains("//") {
                                    normalize_jsx_self_closing_line_comment_layout(&segment)
                                } else if src.contains("/*") {
                                    normalize_jsx_opening_block_comment_layout(&segment)
                                } else {
                                    compact_multiline_jsx_self_closing_layout(&segment)
                                };
                                if compacted != segment {
                                    self.output.truncate(output_start);
                                    self.output.push_str(&compacted);
                                }
                            }
                        }
                    }
                    // TypeScript normalizes self-closing JSX: when no attributes,
                    // ensure space before `/>` (e.g. `<div/>` → `<div />`).
                    // This runs AFTER copy_expr_span so normalize_close_paren
                    // won't strip the space.
                    if no_attrs && self.output.ends_with("/>") {
                        let len = self.output.len();
                        if len >= 3 && self.output.as_bytes()[len - 3] != b' ' {
                            self.output.insert(len - 2, ' ');
                        }
                    }
                } else if self.jsx_is_react_jsx() {
                    self.emit_jsx_self_closing_react_jsx(el, expr.span);
                } else {
                    self.emit_jsx_self_closing(el);
                    // Preserve trailing line comment after JSX self-closing element
                    // Only when inside parens (nested expression context).
                    let span_start = expr.span.start as usize;
                    let span_end = expr.span.end as usize;
                    let in_parens = span_start > 0
                        && self.source[..span_start]
                            .bytes()
                            .rev()
                            .find(|&b| b != b' ' && b != b'\t' && b != b'\n' && b != b'\r')
                            .is_some_and(|b| b == b'(');
                    if in_parens && span_end < self.source.len() {
                        let line_end = self.source[span_end..]
                            .find('\n')
                            .map(|p| span_end + p)
                            .unwrap_or(self.source.len());
                        if let Some(comment) =
                            find_line_comment_in_range(self.source, span_end, line_end)
                        {
                            self.write(" ");
                            self.write(comment);
                            if let Some(comment_start_rel) =
                                self.source[span_end..line_end].find("//")
                            {
                                self.advance_comment_pos(
                                    (span_end + comment_start_rel + comment.len()) as u32,
                                );
                            }
                            self.newline();
                        }
                    }
                }
            }
            ExprKind::JsxFragment(frag) => {
                if self.jsx_is_preserve() {
                    let mut reps = Vec::new();
                    // Normalize fragment opening tag: `<    >` → `<>`,
                    // `< /*comment*/ >` → `<>`.
                    {
                        let s = expr.span.start as usize;
                        let e = (expr.span.end as usize).min(self.source.len());
                        // Opening fragment: `<` followed by optional whitespace/comments then `>`
                        if s + 1 < e {
                            // Find the first `>` after `<` that ends the opening tag.
                            // The first child or closing `</` tells us where the opening tag ends.
                            let open_end = frag.children.first().map_or(e, |c| match c {
                                JsxChild::Text(_, sp) => sp.start as usize,
                                JsxChild::Element(el) => el.span.start as usize,
                                JsxChild::Expression(_, sp) => sp.start as usize,
                                JsxChild::Fragment(f) => {
                                    f.children.first().map_or(e, |fc| match fc {
                                        JsxChild::Text(_, s) => s.start as usize,
                                        _ => e,
                                    })
                                }
                            });
                            // Scan from `<` to find `>` within the opening tag region
                            if let Some(gt_off) = self.source[s..open_end.min(e)].find('>') {
                                let gt_pos = s + gt_off;
                                // If there's content between `<` and `>`, strip it
                                if gt_pos > s + 1 {
                                    reps.push((s + 1, gt_pos, String::new()));
                                }
                            }
                        }
                        // Closing fragment: `</` followed by optional whitespace/comments then `>`
                        let elem_text = &self.source[s..e];
                        if let Some(close_off) = elem_text.rfind("</") {
                            let close_start = s + close_off + 2; // after `</`
                            if let Some(gt_off) = self.source[close_start..e].find('>') {
                                let gt_pos = close_start + gt_off;
                                if gt_pos > close_start {
                                    reps.push((close_start, gt_pos, String::new()));
                                }
                            }
                        }
                    }
                    // In JS files, JSX type-argument syntax is not type-level; keep it.
                    if !self.is_js_file {
                        collect_jsx_children_deep_type_strips(
                            &frag.children,
                            self.source,
                            &mut reps,
                        );
                    }
                    // Collect CJS import replacements from children
                    for child in &frag.children {
                        if let JsxChild::Expression(Some(child_expr), _) = child {
                            self.collect_expr_cjs_import_replacements(child_expr, &mut reps);
                        }
                    }
                    self.collect_jsx_preserve_multiline_expr_replacements(
                        &frag.children,
                        &mut reps,
                    );
                    self.collect_jsx_child_line_comment_replacements(&frag.children, &mut reps);
                    self.emit_jsx_preserve_with_replacements(expr.span, &reps);
                } else if self.jsx_is_react_jsx() {
                    self.emit_jsx_fragment_react_jsx(frag, expr.span);
                } else {
                    self.emit_jsx_fragment(frag);
                }
            }
        }
    }

    fn emit_compact_multiline_paren_assign_expr(&mut self, expr: &Expr) -> bool {
        let ExprKind::Paren(inner) = &expr.kind else {
            return false;
        };
        if !matches!(inner.kind, ExprKind::Assign(_)) {
            return false;
        }
        let start = expr.span.start as usize;
        let end = expr.span.end as usize;
        if start >= end || end > self.source.len() {
            return false;
        }
        let src = &self.source[start..end];
        if !src.contains('\n') || src.contains("//") || src.contains("/*") {
            return false;
        }
        let mut lines = src.lines();
        let Some(first) = lines.next() else {
            return false;
        };
        self.write(first.trim_end());
        let mut emitted_multiline = false;
        let cont_indent = "    ".repeat((self.indent + 1) as usize);
        for line in lines {
            if line.trim().is_empty() {
                continue;
            }
            emitted_multiline = true;
            self.newline();
            self.write(&cont_indent);
            self.write(line.trim_start());
        }
        emitted_multiline
    }

    fn private_member_needs_new_paren(&self, mem: &MemberExpr) -> bool {
        if !mem.property.starts_with('#') || !self.needs_downlevel("private-fields") {
            return false;
        }
        let name = normalize_unicode_escapes(&mem.property[1..]);
        self.current_class_private_fields.contains(name.as_str())
            || self
                .current_class_private_methods
                .contains_key(name.as_str())
            || self
                .current_class_private_accessors
                .contains_key(name.as_str())
            || self
                .current_class_static_private_methods
                .contains_key(name.as_str())
            || self.enclosing_private_member_info(name.as_str()).is_some()
    }

    fn emit_private_enclosing_static_receiver(
        &mut self,
        object: &Expr,
        alias: &str,
        class_name: &str,
    ) {
        if unwrap_private_receiver_class_name(object) == Some(class_name) {
            self.write(alias);
        } else {
            self.emit_expr(object);
        }
    }

    pub(super) fn emit_assign_expr_core(&mut self, assign: &AssignExpr, value_discarded: bool) {
        // Set binding name for anonymous class expressions so __setFunctionName
        // can infer the name from the assignment target (e.g. `x = @dec class {}` → "x").
        let prev_binding_name = self.class_expr_binding_name.clone();
        if matches!(&assign.right.kind, ExprKind::ClassExpr(cd) if cd.name.is_none()) {
            if let ExprKind::Ident(ref name) = assign.left.kind {
                self.class_expr_binding_name =
                    Some(ClassExprBindingName::Literal(name.to_string()));
            }
        }

        // Check for private field assignment/compound assignment: obj.#field = value, obj.#field += value, etc.
        // Unwrap parens/type assertions from LHS to find inner member expression.
        if self.needs_downlevel("private-fields") {
            if let Some(mem) = unwrap_to_private_member(&assign.left) {
                let field_name = normalize_unicode_escapes(&mem.property[1..]);
                if self
                    .current_class_private_fields
                    .contains(field_name.as_str())
                {
                    if assign.op == AssignOp::Assign {
                        // Simple assignment: obj.#field = value
                        self.needs_private_field_set = true;
                        let field_var = self.private_field_var_name(&field_name);
                        let is_static = self
                            .current_class_static_private_fields
                            .contains(field_name.as_str());
                        let prefix = self.helper_prefix();
                        self.write(prefix);
                        self.write("__classPrivateFieldSet(");
                        if is_static {
                            let alias = self
                                .current_class_static_alias
                                .clone()
                                .unwrap_or_else(|| "_a".to_string());
                            self.emit_private_field_receiver(&mem.object, &alias);
                            self.write(", ");
                            self.write(&alias);
                            self.write(", ");
                            self.emit_expr(&assign.right);
                            self.write(", \"f\", ");
                            self.write(&field_var);
                            self.write(")");
                        } else {
                            self.emit_expr(&mem.object);
                            self.write(", ");
                            self.write(&field_var);
                            self.write(", ");
                            self.emit_expr(&assign.right);
                            self.write(", \"f\")");
                        }
                        return;
                    }
                    if matches!(
                        assign.op,
                        AssignOp::LogAndAssign | AssignOp::LogOrAssign | AssignOp::NullCoalAssign
                    ) {
                        // Logical compound: obj.#field &&= value
                        self.emit_private_field_logical_assign_downlevel(assign, mem, &field_name);
                        return;
                    }
                    // Arithmetic compound: obj.#field += value
                    self.emit_private_field_compound_assign_downlevel(assign, mem, &field_name);
                    return;
                }
                // Private method assignment: obj.#method = value
                // → __classPrivateFieldSet(obj, _C_instances, value, "m")
                if let Some(_method_var) = self
                    .current_class_private_methods
                    .get(field_name.as_str())
                    .cloned()
                {
                    if assign.op == AssignOp::Assign {
                        self.needs_private_field_set = true;
                        let instances_var = if let Some(ref cn) = self.current_class_name {
                            let norm_cn = normalize_unicode_escapes(cn);
                            format!("_{}_instances", norm_cn)
                        } else {
                            "_instances".to_string()
                        };
                        let prefix = self.helper_prefix();
                        self.write(prefix);
                        self.write("__classPrivateFieldSet(");
                        self.emit_expr(&mem.object);
                        self.write(", ");
                        self.write(&instances_var);
                        self.write(", ");
                        self.emit_expr(&assign.right);
                        self.write(", \"m\")");
                        return;
                    }
                    // Compound assignment to private method (obj.#method += value, ++, etc.)
                    // falls through to default handling for now
                }
                // Static private method assignment: obj.#method = value
                if let Some(_method_var) = self
                    .current_class_static_private_methods
                    .get(field_name.as_str())
                    .cloned()
                {
                    if assign.op == AssignOp::Assign {
                        self.needs_private_field_set = true;
                        let alias = self
                            .current_class_static_alias
                            .clone()
                            .unwrap_or_else(|| "_a".to_string());
                        let prefix = self.helper_prefix();
                        self.write(prefix);
                        self.write("__classPrivateFieldSet(");
                        self.emit_private_field_receiver(&mem.object, &alias);
                        self.write(", ");
                        self.write(&alias);
                        self.write(", ");
                        self.emit_expr(&assign.right);
                        self.write(", \"m\")");
                        return;
                    }
                }
                // Private accessor assignment: obj.#prop = value
                // → __classPrivateFieldSet(obj, _A_instances, value, "a", _A_prop_set)
                if let Some((getter_var, setter_var)) = self
                    .current_class_private_accessors
                    .get(field_name.as_str())
                    .cloned()
                {
                    let is_static_acc = self
                        .current_class_static_private_accessors
                        .contains(field_name.as_str());
                    let brand_var = if is_static_acc {
                        self.current_class_static_alias
                            .clone()
                            .unwrap_or_else(|| "_a".to_string())
                    } else if let Some(ref cn) = self.current_class_name {
                        let norm_cn = normalize_unicode_escapes(cn);
                        format!("_{}_instances", norm_cn)
                    } else {
                        "_instances".to_string()
                    };
                    if assign.op == AssignOp::Assign {
                        self.needs_private_field_set = true;
                        let prefix = self.helper_prefix();
                        self.write(prefix);
                        self.write("__classPrivateFieldSet(");
                        if is_static_acc {
                            self.emit_private_field_receiver(&mem.object, &brand_var);
                        } else {
                            self.emit_expr(&mem.object);
                        }
                        self.write(", ");
                        self.write(&brand_var);
                        self.write(", ");
                        self.emit_expr(&assign.right);
                        self.write(", \"a\"");
                        if let Some(ref sv) = setter_var {
                            self.write(", ");
                            self.write(sv);
                        }
                        self.write(")");
                        return;
                    }
                    // Compound accessor assignment: obj.#prop += value
                    // → __classPrivateFieldSet(obj, _inst, __classPrivateFieldGet(obj, _inst, "a", getter) OP value, "a", setter)
                    if !matches!(
                        assign.op,
                        AssignOp::LogAndAssign | AssignOp::LogOrAssign | AssignOp::NullCoalAssign
                    ) {
                        self.needs_private_field_set = true;
                        self.needs_private_field_get = true;
                        let prefix = self.helper_prefix();
                        self.write(prefix);
                        self.write("__classPrivateFieldSet(");
                        if is_static_acc {
                            self.emit_private_field_receiver(&mem.object, &brand_var);
                        } else {
                            self.emit_expr(&mem.object);
                        }
                        self.write(", ");
                        self.write(&brand_var);
                        self.write(", ");
                        self.write(self.helper_prefix());
                        self.write("__classPrivateFieldGet(");
                        if is_static_acc {
                            self.emit_private_field_receiver(&mem.object, &brand_var);
                        } else {
                            self.emit_expr(&mem.object);
                        }
                        self.write(", ");
                        self.write(&brand_var);
                        self.write(", \"a\"");
                        if let Some(ref gv) = getter_var {
                            self.write(", ");
                            self.write(gv);
                        }
                        self.write(") ");
                        self.write(compound_assign_to_bin_op_str(assign.op));
                        self.write(" ");
                        self.emit_expr(&assign.right);
                        self.write(", \"a\"");
                        if let Some(ref sv) = setter_var {
                            self.write(", ");
                            self.write(sv);
                        }
                        self.write(")");
                        return;
                    }
                }
            }
            if assign.op == AssignOp::Assign {
                if let Some(props) = self.as_object_destructure_pattern(&assign.left) {
                    if self.obj_destructuring_has_private_field_target(props) {
                        if self.emit_private_field_obj_destructure_assign_downlevel(
                            props,
                            &assign.right,
                        ) {
                            return;
                        }
                    }
                }
                if let Some(elements) = self.as_array_destructure_pattern(&assign.left) {
                    if elements
                        .iter()
                        .flatten()
                        .any(|expr| self.expr_has_private_destructure_target(expr))
                    {
                        if self.emit_private_array_destructure_assign_downlevel(
                            elements,
                            &assign.right,
                        ) {
                            return;
                        }
                    }
                }
            }
        }
        // Super member/element access assignment in decorated class static context:
        // super.x = val → Reflect.set(_classSuper, "x", val, _classThis)
        // super.x += val → Reflect.set(_classSuper, "x", Reflect.get(...) + val, ...)
        if self.emit_super_reflect_assign(assign) {
            self.class_expr_binding_name = prev_binding_name;
            return;
        }
        // Destructuring assignment with object rest: ({ ...bar } = rhs) → (bar = __rest(rhs, []))
        // Also handles: ({ a, ...rest } = rhs) → ({ a } = rhs, rest = __rest(rhs, ["a"]))
        if assign.op == AssignOp::Assign && self.needs_downlevel("object-spread") {
            if value_discarded
                && self.emit_single_nested_obj_rest_assign_downlevel(&assign.left, &assign.right)
            {
                self.class_expr_binding_name = prev_binding_name;
                return;
            }
            if let Some(props) = self.as_object_destructure_pattern(&assign.left) {
                if assign_target_has_object_rest(&assign.left) {
                    self.emit_obj_rest_assign_downlevel(props, &assign.right, !value_discarded);
                    return;
                }
            }
            if let Some((rewritten_left, transforms)) =
                self.rewrite_nested_obj_rest_expr_with_hoisted_temps(&assign.left)
            {
                self.emit_expr(&rewritten_left);
                self.write(" = ");
                self.emit_expr(&assign.right);
                for transform in transforms {
                    self.write(", ");
                    self.emit_nested_obj_rest_expr_transform(&transform);
                }
                return;
            }
        }
        // CJS destructuring desugaring: when a destructuring assignment targets
        // exported names that have live export chains, flatten to a comma expression
        // with temp vars.  E.g.:
        //   ({ exportedFoo, nonexportedFoo } = null)
        // becomes:
        //   (_a = null, exports.foo = exports.exportedFoo = _a.exportedFoo,
        //    exports.nfoo = exports.nonexportedFoo = nonexportedFoo = _a.nonexportedFoo)
        if assign.op == AssignOp::Assign
            && !self.cjs_live_export_chain.is_empty()
            && self.export_target.as_ref().is_some_and(|t| t == "exports")
        {
            let needs_desugar = match &assign.left.kind {
                ExprKind::ObjectLit(props) => self.obj_destructuring_has_live_export(props),
                ExprKind::Paren(inner) => self
                    .as_object_destructure_pattern(inner)
                    .is_some_and(|props| self.obj_destructuring_has_live_export(props)),
                ExprKind::ArrayLit(elements) => self.array_destructuring_has_live_export(elements),
                _ => false,
            };
            if needs_desugar {
                self.emit_cjs_destructuring_desugar(assign);
                return;
            }
        }
        if assign.op == AssignOp::ExpAssign && self.needs_downlevel("exponentiation") {
            self.emit_exp_assign_downlevel(assign);
        } else if self.needs_downlevel("logical-assignment")
            && matches!(
                assign.op,
                AssignOp::LogAndAssign | AssignOp::LogOrAssign | AssignOp::NullCoalAssign
            )
        {
            self.emit_logical_assign_downlevel(assign);
        } else if self.needs_downlevel("optional-chaining") && is_oc_chain(&assign.left) {
            // Optional chain assignment: obj?.a = 1
            //   → obj === null || obj === void 0 ? void 0 : obj.a = 1
            // The assignment must be inside the ternary's false branch.
            // Suppressing OC parens means the ternary is emitted without outer
            // wrapping, so ` = rhs` binds as the continuation of the false branch
            // (assignment has lower precedence than ternary colon).
            self.suppress_oc_parens = true;
            self.emit_expr(&assign.left);
            self.write(" ");
            self.write(assign_op_str(assign.op));
            self.write(" ");
            self.emit_expr(&assign.right);
        } else {
            // For destructuring assignments in static super context, set flag
            // so super member/element access in LHS emits setter proxy.
            let needs_destructure_target = assign.op == AssignOp::Assign
                && self.static_super_base_alias.is_some()
                && matches!(
                    &assign.left.kind,
                    ExprKind::ObjectLit(_) | ExprKind::ArrayLit(_) | ExprKind::Paren(_)
                );
            if needs_destructure_target {
                self.super_reflect_destructure_target = true;
            }
            let nested_pattern_default = self.emitting_destructuring_assignment_pattern;
            if assign.op == AssignOp::Assign
                && (self.as_object_destructure_pattern(&assign.left).is_some()
                    || self.as_array_destructure_pattern(&assign.left).is_some()
                    || nested_pattern_default)
            {
                self.emit_destructuring_assignment_target(&assign.left);
            } else {
                self.emit_expr(&assign.left);
            }
            if needs_destructure_target {
                self.super_reflect_destructure_target = false;
            }
            self.write(" ");
            self.write(assign_op_str(assign.op));
            // When the RHS is a type assertion that will be stripped and the
            // source has a line break before the `<`, preserve the line break
            // so that multi-line expressions keep their structure.
            let rhs_newline = self.rhs_has_newline_before_type_assertion(&assign.right);
            if rhs_newline {
                self.newline();
                self.write("    "); // continuation indent
            } else {
                self.write(" ");
            }
            self.suppress_oc_parens = true;
            if nested_pattern_default {
                let prev_pattern = self.emitting_destructuring_assignment_pattern;
                self.emitting_destructuring_assignment_pattern = false;
                self.emit_expr(&assign.right);
                self.emitting_destructuring_assignment_pattern = prev_pattern;
            } else {
                self.emit_expr(&assign.right);
            }
        }
        self.class_expr_binding_name = prev_binding_name;
    }

    /// Returns true if `expr` is a type-assertion wrapper (TypeAssertion, As,
    /// Satisfies, NonNull) that starts on a new line in the source (only
    /// whitespace between the last newline and the expression start).
    fn rhs_has_newline_before_type_assertion(&self, expr: &Expr) -> bool {
        use crate::source_transform::strip_type_layers;
        let inner = strip_type_layers(expr);
        if std::ptr::eq(inner, expr) {
            return false; // no type layer to strip
        }
        let ta_start = expr.span.start as usize;
        if ta_start == 0 || ta_start > self.source.len() {
            return false;
        }
        let before = &self.source[..ta_start];
        before
            .rfind('\n')
            .is_some_and(|nl| before[nl + 1..].trim().is_empty())
    }

    pub(super) fn as_object_destructure_pattern<'b>(
        &self,
        expr: &'b Expr,
    ) -> Option<&'b [ObjLitProp]> {
        match &expr.kind {
            ExprKind::ObjectLit(props) => Some(props),
            ExprKind::Paren(inner) | ExprKind::NonNull(inner) => {
                self.as_object_destructure_pattern(inner)
            }
            ExprKind::As(a) => self.as_object_destructure_pattern(&a.expr),
            ExprKind::Satisfies(s) => self.as_object_destructure_pattern(&s.expr),
            ExprKind::TypeAssertion(ta) => self.as_object_destructure_pattern(&ta.expr),
            _ => None,
        }
    }

    fn emit_destructuring_assignment_target(&mut self, expr: &Expr) {
        let prev_pattern = self.emitting_destructuring_assignment_pattern;
        self.emitting_destructuring_assignment_pattern =
            Self::is_destructuring_assignment_pattern_node(expr);
        self.emit_expr(expr);
        self.emitting_destructuring_assignment_pattern = prev_pattern;
    }

    fn is_destructuring_assignment_pattern_node(expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::ObjectLit(_) | ExprKind::ArrayLit(_) | ExprKind::Assign(_) => true,
            ExprKind::Paren(inner) | ExprKind::NonNull(inner) => {
                Self::is_destructuring_assignment_pattern_node(inner)
            }
            ExprKind::As(as_expr) => Self::is_destructuring_assignment_pattern_node(&as_expr.expr),
            ExprKind::Satisfies(satisfies) => {
                Self::is_destructuring_assignment_pattern_node(&satisfies.expr)
            }
            ExprKind::TypeAssertion(assertion) => {
                Self::is_destructuring_assignment_pattern_node(&assertion.expr)
            }
            _ => false,
        }
    }

    fn obj_destructuring_has_private_field_target(&self, props: &[ObjLitProp]) -> bool {
        props.iter().any(|prop| match prop {
            ObjLitProp::Property(p) => self.expr_has_private_destructure_target(&p.value),
            ObjLitProp::Spread(e, _) => self.expr_has_private_destructure_target(e),
            _ => false,
        })
    }

    fn expr_has_private_destructure_target(&self, expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Member(m) => m.property.starts_with('#'),
            ExprKind::ObjectLit(props) => self.obj_destructuring_has_private_field_target(props),
            ExprKind::ArrayLit(elements) => elements
                .iter()
                .flatten()
                .any(|e| self.expr_has_private_destructure_target(e)),
            ExprKind::Assign(a) => self.expr_has_private_destructure_target(&a.left),
            ExprKind::Paren(e) | ExprKind::NonNull(e) => {
                self.expr_has_private_destructure_target(e)
            }
            ExprKind::As(a) => self.expr_has_private_destructure_target(&a.expr),
            ExprKind::Satisfies(s) => self.expr_has_private_destructure_target(&s.expr),
            ExprKind::TypeAssertion(ta) => self.expr_has_private_destructure_target(&ta.expr),
            _ => false,
        }
    }

    fn as_array_destructure_pattern<'b>(&self, expr: &'b Expr) -> Option<&'b [Option<Box<Expr>>]> {
        match &expr.kind {
            ExprKind::ArrayLit(elements) => Some(elements),
            ExprKind::Paren(inner) | ExprKind::NonNull(inner) => {
                self.as_array_destructure_pattern(inner)
            }
            ExprKind::As(a) => self.as_array_destructure_pattern(&a.expr),
            ExprKind::Satisfies(s) => self.as_array_destructure_pattern(&s.expr),
            ExprKind::TypeAssertion(ta) => self.as_array_destructure_pattern(&ta.expr),
            _ => None,
        }
    }

    pub(super) fn direct_private_destructure_member<'b>(
        &self,
        expr: &'b Expr,
    ) -> Option<&'b MemberExpr> {
        unwrap_to_private_member(expr)
    }

    pub(super) fn private_member_proxy_param_name(&mut self, kind_str: &str) -> String {
        if kind_str == "\"m\"" {
            let name = self.next_inline_temp_var();
            self.temp_var_counter = self.temp_var_counter.saturating_sub(1);
            return name;
        }

        if let Some(name) = self.private_destructure_proxy_param.clone() {
            return name;
        }

        let name = self.next_inline_deferred_temp_placeholder();
        self.private_destructure_proxy_param = Some(name.clone());
        name
    }

    pub(super) fn private_member_destructure_capture_temp(
        &mut self,
        mem: &MemberExpr,
        is_static: bool,
    ) -> Option<String> {
        if is_static && matches!(&mem.object.kind, ExprKind::Ident(_)) {
            None
        } else {
            Some(self.next_temp_var())
        }
    }

    pub(super) fn emit_private_member_setter_proxy_value(
        &mut self,
        mem: &MemberExpr,
        recv_temp: Option<&str>,
        state_var: &str,
        kind_str: &str,
        set_extra: Option<&str>,
        setter_param: &str,
        is_static: bool,
    ) {
        self.write("({ set value(");
        self.write(setter_param);
        self.write(") { ");
        self.write(self.helper_prefix());
        self.write("__classPrivateFieldSet(");
        if let Some(recv_temp) = recv_temp {
            self.write(recv_temp);
        } else if is_static {
            self.emit_private_field_receiver(&mem.object, state_var);
        } else {
            self.emit_expr(&mem.object);
        }
        self.write(", ");
        self.write(state_var);
        self.write(", ");
        self.write(setter_param);
        self.write(", ");
        self.write(kind_str);
        if let Some(set_extra) = set_extra {
            self.write(", ");
            self.write(set_extra);
        }
        self.write("); } }).value");
    }

    pub(super) fn private_member_write_info_for_member(
        &self,
        mem: &MemberExpr,
    ) -> Option<(String, &'static str, Option<String>, Option<String>, bool)> {
        if !mem.property.starts_with('#') {
            return None;
        }

        let field_name = normalize_unicode_escapes(&mem.property[1..]);
        if self
            .current_class_private_fields
            .contains(field_name.as_str())
        {
            let field_var = self.private_field_var_name(&field_name);
            let is_static = self
                .current_class_static_private_fields
                .contains(field_name.as_str());
            if is_static {
                let alias = self.current_class_static_alias_or_default();
                return Some((
                    alias,
                    "\"f\"",
                    Some(field_var.clone()),
                    Some(field_var),
                    true,
                ));
            }
            return Some((field_var, "\"f\"", None, None, false));
        }

        if let Some(method_var) = self
            .current_class_private_methods
            .get(field_name.as_str())
            .cloned()
        {
            let instances_var = self.current_class_instances_var();
            return Some((
                instances_var,
                "\"m\"",
                Some(method_var.to_string()),
                None,
                false,
            ));
        }

        if let Some(method_var) = self
            .current_class_static_private_methods
            .get(field_name.as_str())
            .cloned()
        {
            let alias = self.current_class_static_alias_or_default();
            return Some((alias, "\"m\"", Some(method_var.to_string()), None, true));
        }

        if let Some((getter_var, setter_var)) = self
            .current_class_private_accessors
            .get(field_name.as_str())
            .cloned()
        {
            let is_static = self
                .current_class_static_private_accessors
                .contains(field_name.as_str());
            if is_static {
                let alias = self.current_class_static_alias_or_default();
                return Some((
                    alias,
                    "\"a\"",
                    getter_var.map(|v| v.to_string()),
                    setter_var.map(|v| v.to_string()),
                    true,
                ));
            }
            let instances_var = self.current_class_instances_var();
            return Some((
                instances_var,
                "\"a\"",
                getter_var.map(|v| v.to_string()),
                setter_var.map(|v| v.to_string()),
                false,
            ));
        }

        None
    }

    /// Returns the current class's static brand alias (cloned) or the default
    /// `"_a"` placeholder. Used by all private static member dispatch paths.
    #[inline]
    fn current_class_static_alias_or_default(&self) -> String {
        self.current_class_static_alias
            .clone()
            .unwrap_or_else(|| "_a".to_string())
    }

    /// Returns the current class's instance brand variable
    /// (`_<ClassName>_instances`) or the `"_instances"` fallback.
    #[inline]
    fn current_class_instances_var(&self) -> String {
        if let Some(ref cn) = self.current_class_name {
            let norm_cn = normalize_unicode_escapes(cn);
            format!("_{}_instances", norm_cn)
        } else {
            "_instances".to_string()
        }
    }

    fn emit_private_array_destructure_assign_downlevel(
        &mut self,
        elements: &[Option<Box<Expr>>],
        rhs: &Expr,
    ) -> bool {
        // `mem` borrows from the AST via `elements`; `captures.1` borrows the
        // receiver expr from `mem.object`. Storing references avoids a deep
        // clone of the MemberExpr and its receiver per private destructure
        // target — both are recursive AST nodes.
        struct Transform<'a> {
            idx: usize,
            mem: &'a MemberExpr,
            recv_temp: Option<String>,
            state_var: String,
            kind_str: &'static str,
            set_extra: Option<String>,
            setter_param: String,
            is_static: bool,
            is_spread: bool,
        }

        let mut captures: Vec<(String, &Expr)> = Vec::new();
        let mut transforms: Vec<Transform> = Vec::new();

        for (idx, elem) in elements.iter().enumerate() {
            let Some(expr) = elem.as_deref() else {
                continue;
            };
            let (target, is_spread) = match &expr.kind {
                ExprKind::Spread(inner) => (&**inner, true),
                _ => (expr, false),
            };
            let Some(mem) = self.direct_private_destructure_member(target) else {
                continue;
            };
            let Some((state_var, kind_str, _get_extra, set_extra, is_static)) =
                self.private_member_write_info_for_member(mem)
            else {
                continue;
            };
            let recv_temp = self.private_member_destructure_capture_temp(mem, is_static);
            if let Some(recv_temp_name) = recv_temp.as_ref() {
                captures.push((recv_temp_name.clone(), &*mem.object));
            }
            let setter_param = self.private_member_proxy_param_name(&kind_str);
            transforms.push(Transform {
                idx,
                mem,
                recv_temp,
                state_var,
                kind_str,
                set_extra,
                setter_param,
                is_static,
                is_spread,
            });
        }

        if transforms.is_empty() {
            return false;
        }

        self.needs_private_field_set = true;
        for (i, (temp, obj)) in captures.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            self.write(temp);
            self.write(" = ");
            self.emit_expr(obj);
        }
        if !captures.is_empty() {
            self.write(", ");
        }

        self.write("[");
        for (idx, elem) in elements.iter().enumerate() {
            if idx > 0 {
                self.write(", ");
            }
            let Some(expr) = elem.as_deref() else {
                continue;
            };

            if let Some(transform) = transforms.iter().find(|transform| transform.idx == idx) {
                if transform.is_spread {
                    self.write("...");
                }
                self.emit_private_member_setter_proxy_value(
                    transform.mem,
                    transform.recv_temp.as_deref(),
                    &transform.state_var,
                    &transform.kind_str,
                    transform.set_extra.as_deref(),
                    &transform.setter_param,
                    transform.is_static,
                );
                continue;
            }

            self.emit_expr(expr);
        }
        self.write("] = ");
        self.emit_expr(rhs);
        true
    }

    fn enclosing_private_member_info(
        &self,
        field_name: &str,
    ) -> Option<(String, String, EnclosingPrivateMemberKind)> {
        for (members, class_name) in self
            .enclosing_private_members
            .iter()
            .rev()
            .zip(self.enclosing_private_class_names.iter().rev())
        {
            if let (Some(kind), Some(class_name)) = (members.get(field_name), class_name) {
                return Some((
                    class_name.clone(),
                    normalize_unicode_escapes(class_name),
                    kind.clone(),
                ));
            }
        }

        None
    }

    fn private_field_var_name(&self, field_name: &str) -> String {
        // Check the current class's var map for a dedup-suffixed name.
        if let Some(var_name) = self.current_class_private_var_map.get(field_name) {
            return var_name.to_string();
        }
        // Fallback to computed name (for classes that didn't go through dedup allocation).
        if let Some(ref cn) = self.current_class_name {
            let norm_cn = normalize_unicode_escapes(cn);
            format!("_{}_{}", norm_cn, field_name)
        } else {
            format!("_{}", field_name)
        }
    }

    /// Emit the receiver expression for a static private field access.
    /// The class name → alias substitution is handled via cjs_import_map,
    /// so this just delegates to emit_expr.
    fn emit_private_field_receiver(&mut self, object: &Expr, _alias: &str) {
        self.emit_expr(object);
    }

    /// Emit compound assignment on a private field:
    /// `obj.#field += value` → `__classPrivateFieldSet(obj, _w, __classPrivateFieldGet(obj, _w, "f") + value, "f")`
    /// For non-simple receivers, caches in a temp:
    /// `expr.#field += value` → `__classPrivateFieldSet(_a = expr, _w, __classPrivateFieldGet(_a, _w, "f") + value, "f")`
    fn emit_private_field_compound_assign_downlevel(
        &mut self,
        assign: &AssignExpr,
        mem: &MemberExpr,
        field_name: &str,
    ) {
        self.needs_private_field_get = true;
        self.needs_private_field_set = true;
        let field_var = self.private_field_var_name(field_name);
        let prefix = self.helper_prefix();
        let is_static = self
            .current_class_static_private_fields
            .contains(field_name);

        if is_static {
            // Static compound: __classPrivateFieldSet(TEMP = recv, alias, __classPrivateFieldGet(TEMP, alias, "f", field) OP rhs, "f", field)
            // For simple class name receiver, recv is the alias. For complex receivers
            // like A.getClass(), recv is the substituted expression (_a.getClass()).
            let alias = self
                .current_class_static_alias
                .clone()
                .unwrap_or_else(|| "_a".to_string());
            let temp = self.next_temp_var();
            let bin_op = compound_assign_to_bin_op_str(assign.op);
            self.write(prefix);
            self.write("__classPrivateFieldSet(");
            self.write(&temp);
            self.write(" = ");
            self.emit_expr(&mem.object); // uses cjs_import_map to substitute class name
            self.write(", ");
            self.write(&alias);
            self.write(", ");
            if assign.op == AssignOp::ExpAssign {
                self.write("Math.pow(");
                self.write(prefix);
                self.write("__classPrivateFieldGet(");
                self.write(&temp);
                self.write(", ");
                self.write(&alias);
                self.write(", \"f\", ");
                self.write(&field_var);
                self.write("), ");
                self.emit_expr(&assign.right);
                self.write(")");
            } else {
                self.write(prefix);
                self.write("__classPrivateFieldGet(");
                self.write(&temp);
                self.write(", ");
                self.write(&alias);
                self.write(", \"f\", ");
                self.write(&field_var);
                self.write(") ");
                self.write(bin_op);
                self.write(" ");
                self.emit_expr(&assign.right);
            }
            self.write(", \"f\", ");
            self.write(&field_var);
            self.write(")");
        } else {
            // Instance compound assignment (existing logic)
            let is_simple = matches!(&mem.object.kind, ExprKind::Ident(_) | ExprKind::This);
            let object_buf = if is_simple {
                None
            } else {
                // Emit the receiver first so nested class expressions and other
                // inner transforms allocate their temps before the outer cache temp.
                let saved_len = self.output.len();
                let saved_at_line_start = self.at_line_start;
                let saved_out_line = self.out_line;
                let saved_out_col = self.out_col;
                self.at_line_start = false;
                self.emit_expr(&mem.object);
                let inner_text = self.output[saved_len..].to_string();
                self.output.truncate(saved_len);
                self.at_line_start = saved_at_line_start;
                self.out_line = saved_out_line;
                self.out_col = saved_out_col;
                Some(inner_text)
            };
            let obj_ref = if is_simple {
                None
            } else {
                Some(self.next_temp_var())
            };
            let bin_op = compound_assign_to_bin_op_str(assign.op);

            if assign.op == AssignOp::ExpAssign {
                self.write(prefix);
                self.write("__classPrivateFieldSet(");
                if let Some(ref temp) = obj_ref {
                    self.write(temp);
                    self.write(" = ");
                    self.write(object_buf.as_deref().unwrap_or_default());
                } else {
                    self.emit_expr(&mem.object);
                }
                self.write(", ");
                self.write(&field_var);
                self.write(", Math.pow(");
                self.write(prefix);
                self.write("__classPrivateFieldGet(");
                if let Some(ref temp) = obj_ref {
                    self.write(temp);
                } else {
                    self.emit_expr(&mem.object);
                }
                self.write(", ");
                self.write(&field_var);
                self.write(", \"f\"), ");
                self.emit_expr(&assign.right);
                self.write("), \"f\")");
            } else {
                self.write(prefix);
                self.write("__classPrivateFieldSet(");
                if let Some(ref temp) = obj_ref {
                    self.write(temp);
                    self.write(" = ");
                    self.write(object_buf.as_deref().unwrap_or_default());
                } else {
                    self.emit_expr(&mem.object);
                }
                self.write(", ");
                self.write(&field_var);
                self.write(", ");
                self.write(prefix);
                self.write("__classPrivateFieldGet(");
                if let Some(ref temp) = obj_ref {
                    self.write(temp);
                } else {
                    self.emit_expr(&mem.object);
                }
                self.write(", ");
                self.write(&field_var);
                self.write(", \"f\") ");
                self.write(bin_op);
                self.write(" ");
                self.emit_expr(&assign.right);
                self.write(", \"f\")");
            }
        }
    }

    /// Emit update (++/--) on a private field.
    ///
    /// Prefix:  `++obj.#f` → `__classPrivateFieldSet(obj, _w, (_a = __classPrivateFieldGet(obj, _w, "f"), ++_a), "f")`
    /// Postfix (value discarded):
    ///   `obj.#f++` → `__classPrivateFieldSet(obj, _w, (_a = __classPrivateFieldGet(obj, _w, "f"), _a++, _a), "f")`
    /// Postfix (value used):
    ///   `x = obj.#f++` → `(__classPrivateFieldSet(obj, _w, (_b = __classPrivateFieldGet(obj, _w, "f"), _a = _b++, _b), "f"), _a)`
    fn emit_private_field_update_downlevel(
        &mut self,
        up: &UpdateExpr,
        mem: &MemberExpr,
        field_name: &str,
        value_discarded: bool,
    ) {
        self.needs_private_field_get = true;
        self.needs_private_field_set = true;
        let field_var = self.private_field_var_name(field_name);
        let prefix = self.helper_prefix();
        let is_prefix = matches!(up.op, UpdateOp::PreInc | UpdateOp::PreDec);
        let is_postfix = !is_prefix;
        let op_str = match up.op {
            UpdateOp::PreInc | UpdateOp::PostInc => "++",
            UpdateOp::PreDec | UpdateOp::PostDec => "--",
        };
        let is_static = self
            .current_class_static_private_fields
            .contains(field_name);

        if is_static {
            // Static update: always needs receiver temp.
            // __classPrivateFieldSet(TEMP = recv, alias, (_v = __classPrivateFieldGet(TEMP, alias, "f", fv), OP _v), "f", fv)
            let alias = self
                .current_class_static_alias
                .clone()
                .unwrap_or_else(|| "_a".to_string());
            let recv_temp = self.next_temp_var();
            let needs_old_value_temp = is_postfix && !value_discarded;
            let old_value_temp = if needs_old_value_temp {
                Some(self.next_temp_var())
            } else {
                None
            };
            let val_temp = self.next_temp_var();

            if needs_old_value_temp {
                self.write("(");
            }
            self.write(prefix);
            self.write("__classPrivateFieldSet(");
            self.write(&recv_temp);
            self.write(" = ");
            self.emit_expr(&mem.object);
            self.write(", ");
            self.write(&alias);
            self.write(", (");
            self.write(&val_temp);
            self.write(" = ");
            self.write(prefix);
            self.write("__classPrivateFieldGet(");
            self.write(&recv_temp);
            self.write(", ");
            self.write(&alias);
            self.write(", \"f\", ");
            self.write(&field_var);
            self.write("), ");
            if is_prefix {
                self.write(op_str);
                self.write(&val_temp);
            } else if let Some(ref old_temp) = old_value_temp {
                self.write(old_temp);
                self.write(" = ");
                self.write(&val_temp);
                self.write(op_str);
                self.write(", ");
                self.write(&val_temp);
            } else {
                self.write(&val_temp);
                self.write(op_str);
                self.write(", ");
                self.write(&val_temp);
            }
            self.write("), \"f\", ");
            self.write(&field_var);
            self.write(")");
            if let Some(ref old_temp) = old_value_temp {
                self.write(", ");
                self.write(old_temp);
                self.write(")");
            }
        } else {
            // Instance update
            let is_simple = matches!(&mem.object.kind, ExprKind::Ident(_) | ExprKind::This);
            let obj_ref = if is_simple {
                None
            } else {
                Some(self.next_temp_var())
            };
            let needs_old_value_temp = is_postfix && !value_discarded;
            let old_value_temp = if needs_old_value_temp {
                Some(self.next_temp_var())
            } else {
                None
            };
            let val_temp = self.next_temp_var();

            if needs_old_value_temp {
                self.write("(");
            }
            self.write(prefix);
            self.write("__classPrivateFieldSet(");
            if let Some(ref temp) = obj_ref {
                self.write(temp);
                self.write(" = ");
                self.emit_expr(&mem.object);
            } else {
                self.emit_expr(&mem.object);
            }
            self.write(", ");
            self.write(&field_var);
            self.write(", (");
            self.write(&val_temp);
            self.write(" = ");
            self.write(prefix);
            self.write("__classPrivateFieldGet(");
            if let Some(ref temp) = obj_ref {
                self.write(temp);
            } else {
                self.emit_expr(&mem.object);
            }
            self.write(", ");
            self.write(&field_var);
            self.write(", \"f\"), ");
            if is_prefix {
                self.write(op_str);
                self.write(&val_temp);
            } else if let Some(ref old_temp) = old_value_temp {
                self.write(old_temp);
                self.write(" = ");
                self.write(&val_temp);
                self.write(op_str);
                self.write(", ");
                self.write(&val_temp);
            } else {
                self.write(&val_temp);
                self.write(op_str);
                self.write(", ");
                self.write(&val_temp);
            }
            self.write("), \"f\")");
            if let Some(ref old_temp) = old_value_temp {
                self.write(", ");
                self.write(old_temp);
                self.write(")");
            }
        }
    }

    /// Emit logical compound assignment on a private field:
    /// `obj.#field &&= value` → `__classPrivateFieldGet(obj, _w, "f") && __classPrivateFieldSet(obj, _w, value, "f")`
    /// `obj.#field ||= value` → `__classPrivateFieldGet(obj, _w, "f") || __classPrivateFieldSet(obj, _w, value, "f")`
    /// `obj.#field ??= value` → similar with nullish check
    fn emit_private_field_logical_assign_downlevel(
        &mut self,
        assign: &AssignExpr,
        mem: &MemberExpr,
        field_name: &str,
    ) {
        self.needs_private_field_get = true;
        self.needs_private_field_set = true;
        let field_var = self.private_field_var_name(field_name);
        let prefix = self.helper_prefix();
        let is_static = self
            .current_class_static_private_fields
            .contains(field_name);

        let logical_op = match assign.op {
            AssignOp::LogAndAssign => "&&",
            AssignOp::LogOrAssign => "||",
            _ => "??", // NullCoalAssign
        };

        if is_static {
            // Static: __classPrivateFieldSet(obj, alias, __classPrivateFieldGet(obj, alias, "f", fv) OP (val), "f", fv)
            let alias = self
                .current_class_static_alias
                .clone()
                .unwrap_or_else(|| "_a".to_string());
            self.write(prefix);
            self.write("__classPrivateFieldSet(");
            self.emit_expr(&mem.object);
            self.write(", ");
            self.write(&alias);
            self.write(", ");
            self.write(prefix);
            self.write("__classPrivateFieldGet(");
            self.emit_expr(&mem.object);
            self.write(", ");
            self.write(&alias);
            self.write(", \"f\", ");
            self.write(&field_var);
            self.write(") ");
            self.write(logical_op);
            self.write(" (");
            self.emit_expr(&assign.right);
            self.write("), \"f\", ");
            self.write(&field_var);
            self.write(")");
        } else {
            // Instance pattern
            let is_simple = matches!(&mem.object.kind, ExprKind::Ident(_) | ExprKind::This);
            let obj_ref = if is_simple {
                None
            } else {
                Some(self.next_temp_var())
            };
            self.write(prefix);
            self.write("__classPrivateFieldSet(");
            if let Some(ref temp) = obj_ref {
                self.write(temp);
                self.write(" = ");
                self.emit_expr(&mem.object);
            } else {
                self.emit_expr(&mem.object);
            }
            self.write(", ");
            self.write(&field_var);
            self.write(", ");
            self.write(prefix);
            self.write("__classPrivateFieldGet(");
            if let Some(ref temp) = obj_ref {
                self.write(temp);
            } else {
                self.emit_expr(&mem.object);
            }
            self.write(", ");
            self.write(&field_var);
            self.write(", \"f\") ");
            self.write(logical_op);
            self.write(" (");
            self.emit_expr(&assign.right);
            self.write("), \"f\")");
        }
    }

    fn emit_private_field_obj_destructure_assign_downlevel(
        &mut self,
        props: &[ObjLitProp],
        rhs: &Expr,
    ) -> bool {
        self.needs_private_field_set = true;

        // Borrow `mem` from `props` instead of cloning the entire MemberExpr
        // (and a second time for `recv_expr`, and a third time when rebuilding
        // a synthetic MemberExpr). Three deep AST clones per private property
        // collapsed into reference passes.
        struct Transform<'a> {
            idx: usize,
            recv_temp: Option<String>,
            mem: &'a MemberExpr,
            state_var: String,
            kind_str: &'static str,
            set_extra: Option<String>,
            setter_param: String,
            is_static: bool,
        }

        let mut captures: Vec<(String, &Expr)> = Vec::new();
        let mut transforms: Vec<Transform> = Vec::new();

        for (idx, prop) in props.iter().enumerate() {
            if let ObjLitProp::Property(p) = prop {
                if let ExprKind::Member(mem) = &p.value.kind {
                    if let Some((state_var, kind_str, _get_extra, set_extra, is_static)) =
                        self.private_member_write_info_for_member(mem)
                    {
                        let recv_temp =
                            self.private_member_destructure_capture_temp(mem, is_static);
                        let setter_param = self.private_member_proxy_param_name(&kind_str);
                        if let Some(recv_temp) = recv_temp.as_ref() {
                            captures.push((recv_temp.clone(), &*mem.object));
                        }
                        transforms.push(Transform {
                            idx,
                            recv_temp,
                            mem,
                            state_var,
                            kind_str,
                            set_extra,
                            setter_param,
                            is_static,
                        });
                    }
                }
            }
        }

        if transforms.is_empty() {
            return false;
        }

        for (i, (temp, obj)) in captures.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            self.write(temp);
            self.write(" = ");
            self.emit_expr(obj);
        }
        if !captures.is_empty() {
            self.write(", ");
        }
        self.write("{ ");

        for (idx, prop) in props.iter().enumerate() {
            if idx > 0 {
                self.write(", ");
            }

            if let Some(t) = transforms.iter().find(|t| t.idx == idx) {
                if let ObjLitProp::Property(p) = prop {
                    self.emit_prop_name(&p.key);
                    self.write(": ");
                    self.emit_private_member_setter_proxy_value(
                        t.mem,
                        t.recv_temp.as_deref(),
                        &t.state_var,
                        &t.kind_str,
                        t.set_extra.as_deref(),
                        &t.setter_param,
                        t.is_static,
                    );
                    continue;
                }
            }

            match prop {
                ObjLitProp::Property(p) => {
                    self.emit_prop_name(&p.key);
                    self.write(": ");
                    self.emit_expr(&p.value);
                }
                ObjLitProp::Shorthand(name, _) => {
                    self.write(name);
                    // Reserved keywords can't be shorthand property values
                    // in object literals — they need `: ` (empty value).
                    if crate::emit_stmt::is_js_reserved_keyword(name) {
                        self.write(": ");
                    }
                }
                ObjLitProp::ShorthandDefault(name, default_expr, _) => {
                    self.write(name);
                    self.write(" = ");
                    self.emit_expr(default_expr);
                }
                ObjLitProp::Spread(expr, _) => {
                    self.write("...");
                    self.emit_expr(expr);
                }
                ObjLitProp::Method(_) | ObjLitProp::Get(_) | ObjLitProp::Set(_) => {
                    self.emit_obj_lit_prop(prop);
                }
            }
        }

        self.write(" } = ");
        self.emit_expr(rhs);
        true
    }

    fn emit_private_method_update_downlevel(
        &mut self,
        up: &UpdateExpr,
        mem: &MemberExpr,
        state_var: &str,
        get_extra_arg: &str,
        is_static: bool,
        value_discarded: bool,
    ) {
        self.needs_private_field_get = true;
        self.needs_private_field_set = true;
        let prefix = self.helper_prefix();
        let is_prefix = matches!(up.op, UpdateOp::PreInc | UpdateOp::PreDec);
        let op_str = match up.op {
            UpdateOp::PreInc | UpdateOp::PostInc => "++",
            UpdateOp::PreDec | UpdateOp::PostDec => "--",
        };

        let recv_temp = if is_static {
            Some(self.next_temp_var())
        } else {
            Some(self.next_temp_var())
        };
        let needs_old_value_temp =
            matches!(up.op, UpdateOp::PostInc | UpdateOp::PostDec) && !value_discarded;
        let old_value_temp = if needs_old_value_temp {
            Some(self.next_temp_var())
        } else {
            None
        };
        let val_temp = self.next_temp_var();

        if needs_old_value_temp {
            self.write("(");
        }
        self.write(prefix);
        self.write("__classPrivateFieldSet(");
        if let Some(ref recv_temp) = recv_temp {
            self.write(recv_temp);
            self.write(" = ");
            if is_static {
                self.emit_private_field_receiver(&mem.object, state_var);
            } else {
                self.emit_expr(&mem.object);
            }
        } else if is_static {
            self.emit_private_field_receiver(&mem.object, state_var);
        } else {
            self.emit_expr(&mem.object);
        }
        self.write(", ");
        self.write(state_var);
        self.write(", (");
        self.write(&val_temp);
        self.write(" = ");
        self.write(prefix);
        self.write("__classPrivateFieldGet(");
        if let Some(ref recv_temp) = recv_temp {
            self.write(recv_temp);
        } else if is_static {
            self.emit_private_field_receiver(&mem.object, state_var);
        } else {
            self.emit_expr(&mem.object);
        }
        self.write(", ");
        self.write(state_var);
        self.write(", \"m\", ");
        self.write(get_extra_arg);
        self.write("), ");
        if is_prefix {
            self.write(op_str);
            self.write(&val_temp);
        } else if let Some(ref old_temp) = old_value_temp {
            self.write(old_temp);
            self.write(" = ");
            self.write(&val_temp);
            self.write(op_str);
            self.write(", ");
            self.write(&val_temp);
        } else {
            self.write(&val_temp);
            self.write(op_str);
            self.write(", ");
            self.write(&val_temp);
        }
        self.write("), \"m\")");
        if let Some(ref old_temp) = old_value_temp {
            self.write(", ");
            self.write(old_temp);
            self.write(")");
        }
    }

    /// Check if an object destructuring pattern (on the LHS of an assignment)
    /// contains any identifier targets that are in the CJS live export chain.
    fn obj_destructuring_has_live_export(&self, props: &[ObjLitProp]) -> bool {
        for prop in props {
            match prop {
                ObjLitProp::Property(p) => {
                    if self.expr_is_live_export_target(&p.value) {
                        return true;
                    }
                }
                ObjLitProp::Shorthand(name, _) | ObjLitProp::ShorthandDefault(name, _, _) => {
                    if self.cjs_live_export_chain.contains_key(name.as_str()) {
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }

    /// Check if an array destructuring pattern (on the LHS of an assignment)
    /// contains any identifier targets that are in the CJS live export chain.
    fn array_destructuring_has_live_export(&self, elements: &[Option<Box<Expr>>]) -> bool {
        for elem in elements.iter().flatten() {
            if self.expr_is_live_export_target(elem) {
                return true;
            }
        }
        false
    }

    /// Check if an expression (on the LHS of a destructuring) is or contains
    /// an identifier that's in the CJS live export chain.
    fn expr_is_live_export_target(&self, expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Ident(name) => self.cjs_live_export_chain.contains_key(name.as_str()),
            ExprKind::ObjectLit(props) => self.obj_destructuring_has_live_export(props),
            ExprKind::ArrayLit(elements) => self.array_destructuring_has_live_export(elements),
            ExprKind::Assign(assign) => self.expr_is_live_export_target(&assign.left),
            _ => false,
        }
    }

    /// Emit a CJS destructuring desugaring as a flat comma expression.
    /// Transforms `({ a, b } = rhs)` into `(_tmp = rhs, exports.a = _tmp.a, ...)`.
    fn emit_cjs_destructuring_desugar(&mut self, assign: &AssignExpr) {
        match &assign.left.kind {
            ExprKind::ObjectLit(props) => {
                self.emit_cjs_obj_destructure_desugar(props, &assign.right);
            }
            ExprKind::ArrayLit(elements) => {
                self.emit_cjs_array_destructure_desugar(elements, &assign.right);
            }
            _ => {
                // Fallback: emit as normal
                self.emit_expr(&assign.left);
                self.write(" = ");
                self.emit_expr(&assign.right);
            }
        }
    }

    /// Emit desugared object destructuring for CJS exports.
    /// Handles nested patterns by folding access paths to minimize temp vars.
    /// `({ a, b } = rhs)` → `(_tmp = rhs, <chain for a> = _tmp.a, <chain for b> = _tmp.b)`
    /// `({ foo: { bar, baz } } = rhs)` → `(_tmp = rhs.foo, ... = _tmp.bar, ...)`
    fn emit_cjs_obj_destructure_desugar(&mut self, props: &[ObjLitProp], rhs: &Expr) {
        // Fold single-property nested patterns: walk down one level at a time
        // building an access path suffix, until we reach a level with >1 target
        // or a non-nested target.
        let mut access_suffix = String::new();
        let mut current_props = props;
        loop {
            if current_props.len() == 1 {
                if let ObjLitProp::Property(p) = &current_props[0] {
                    let key_str = self.prop_name_to_access_str(&p.key);
                    match &p.value.kind {
                        ExprKind::ObjectLit(nested_props) => {
                            access_suffix.push('.');
                            access_suffix.push_str(&key_str);
                            current_props = nested_props;
                            continue;
                        }
                        ExprKind::ArrayLit(nested_elements) => {
                            access_suffix.push('.');
                            access_suffix.push_str(&key_str);
                            let tmp = self.next_temp_var();
                            self.write(&tmp);
                            self.write(" = ");
                            self.suppress_oc_parens = true;
                            self.emit_expr(rhs);
                            self.write(&access_suffix);
                            self.emit_cjs_array_leaf_elements(nested_elements, &tmp);
                            return;
                        }
                        _ => {}
                    }
                }
            }
            break;
        }
        let tmp = self.next_temp_var();
        self.write(&tmp);
        self.write(" = ");
        self.suppress_oc_parens = true;
        self.emit_expr(rhs);
        self.write(&access_suffix);
        self.emit_cjs_obj_leaf_props(current_props, &tmp);
    }

    /// Emit leaf-level property extractions for a desugared object destructuring.
    fn emit_cjs_obj_leaf_props(&mut self, props: &[ObjLitProp], tmp: &str) {
        for prop in props {
            self.write(", ");
            match prop {
                ObjLitProp::Shorthand(name, _) => {
                    self.emit_cjs_destructure_target(name, tmp, name);
                }
                ObjLitProp::ShorthandDefault(name, _, _) => {
                    self.emit_cjs_destructure_target(name, tmp, name);
                }
                ObjLitProp::Property(p) => {
                    let key_str = self.prop_name_to_access_str(&p.key);
                    match &p.value.kind {
                        ExprKind::Ident(target_name) => {
                            self.emit_cjs_destructure_target(target_name, tmp, &key_str);
                        }
                        ExprKind::ObjectLit(nested_props) => {
                            let rhs_str = format!("{}.{}", tmp, key_str);
                            self.emit_cjs_nested_destructure_obj(nested_props, &rhs_str);
                        }
                        ExprKind::ArrayLit(nested_elements) => {
                            let rhs_str = format!("{}.{}", tmp, key_str);
                            self.emit_cjs_nested_destructure_array(nested_elements, &rhs_str);
                        }
                        _ => {
                            self.emit_expr(&p.value);
                            self.write(" = ");
                            self.write(tmp);
                            self.write(".");
                            self.write(&key_str);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Emit desugared array destructuring for CJS exports.
    fn emit_cjs_array_destructure_desugar(&mut self, elements: &[Option<Box<Expr>>], rhs: &Expr) {
        // Fold single-element nested patterns
        let non_empty: Vec<_> = elements
            .iter()
            .enumerate()
            .filter_map(|(i, e)| e.as_ref().map(|e| (i, e)))
            .collect();
        if non_empty.len() == 1 {
            let (idx, single) = non_empty[0];
            match &single.kind {
                ExprKind::ObjectLit(nested_props) => {
                    let tmp = self.next_temp_var();
                    self.write(&tmp);
                    self.write(" = ");
                    self.suppress_oc_parens = true;
                    self.emit_expr(rhs);
                    self.write(&format!("[{}]", idx));
                    self.emit_cjs_obj_leaf_props(nested_props, &tmp);
                    return;
                }
                ExprKind::ArrayLit(nested_elements) => {
                    let tmp = self.next_temp_var();
                    self.write(&tmp);
                    self.write(" = ");
                    self.suppress_oc_parens = true;
                    self.emit_expr(rhs);
                    self.write(&format!("[{}]", idx));
                    self.emit_cjs_array_leaf_elements(nested_elements, &tmp);
                    return;
                }
                _ => {}
            }
        }
        let tmp = self.next_temp_var();
        self.write(&tmp);
        self.write(" = ");
        self.suppress_oc_parens = true;
        self.emit_expr(rhs);
        self.emit_cjs_array_leaf_elements(elements, &tmp);
    }

    /// Emit leaf-level element extractions for a desugared array destructuring.
    fn emit_cjs_array_leaf_elements(&mut self, elements: &[Option<Box<Expr>>], tmp: &str) {
        for (i, elem) in elements.iter().enumerate() {
            let Some(elem_expr) = elem else { continue };
            self.write(", ");
            let idx_str = i.to_string();
            match &elem_expr.kind {
                ExprKind::Ident(target_name) => {
                    self.emit_cjs_destructure_target_indexed(target_name, tmp, &idx_str);
                }
                ExprKind::ObjectLit(nested_props) => {
                    let rhs_str = format!("{}[{}]", tmp, i);
                    self.emit_cjs_nested_destructure_obj(nested_props, &rhs_str);
                }
                ExprKind::ArrayLit(nested_elements) => {
                    let rhs_str = format!("{}[{}]", tmp, i);
                    self.emit_cjs_nested_destructure_array(nested_elements, &rhs_str);
                }
                _ => {
                    self.emit_expr(elem_expr);
                    self.write(" = ");
                    self.write(tmp);
                    self.write("[");
                    self.write(&idx_str);
                    self.write("]");
                }
            }
        }
    }

    /// Emit a nested object destructure with a new temp variable.
    fn emit_cjs_nested_destructure_obj(&mut self, props: &[ObjLitProp], rhs_str: &str) {
        let tmp = self.next_temp_var();
        self.write(&tmp);
        self.write(" = ");
        self.write(rhs_str);
        self.emit_cjs_obj_leaf_props(props, &tmp);
    }

    /// Emit a nested array destructure with a new temp variable.
    fn emit_cjs_nested_destructure_array(&mut self, elements: &[Option<Box<Expr>>], rhs_str: &str) {
        let tmp = self.next_temp_var();
        self.write(&tmp);
        self.write(" = ");
        self.write(rhs_str);
        self.emit_cjs_array_leaf_elements(elements, &tmp);
    }

    /// Emit a single destructure target with CJS export chain.
    /// For `target_name` with chain `["foo", "exportedFoo"]` and property `key`:
    /// → `exports.foo = exports.exportedFoo = tmp.key`
    /// For `target_name` with chain `["nfoo", "nonexportedFoo"]` and NOT in cjs_var_export_names:
    /// → `exports.nfoo = exports.nonexportedFoo = nonexportedFoo = tmp.key`
    fn emit_cjs_destructure_target(&mut self, target_name: &str, tmp: &str, key: &str) {
        let chain = self.cjs_live_export_chain.get(target_name).cloned();
        if let Some(chain) = chain {
            for exported in &chain {
                self.write_cjs_export_access("exports", exported);
                self.write(" = ");
            }
        }
        // If the name is in cjs_var_export_names, emit `exports.name = ` (it's
        // a var-level export like `export let name`).
        // If NOT in cjs_var_export_names, it has a local variable that needs assignment.
        if self.cjs_var_export_names.contains(target_name) {
            self.write_cjs_export_access("exports", target_name);
            self.write(" = ");
        } else {
            self.write(target_name);
            self.write(" = ");
        }
        self.write(tmp);
        self.write(".");
        self.write(key);
    }

    /// Same as emit_cjs_destructure_target but uses bracket notation for arrays.
    fn emit_cjs_destructure_target_indexed(&mut self, target_name: &str, tmp: &str, idx: &str) {
        let chain = self.cjs_live_export_chain.get(target_name).cloned();
        if let Some(chain) = chain {
            for exported in &chain {
                self.write_cjs_export_access("exports", exported);
                self.write(" = ");
            }
        }
        if self.cjs_var_export_names.contains(target_name) {
            self.write_cjs_export_access("exports", target_name);
            self.write(" = ");
        } else {
            self.write(target_name);
            self.write(" = ");
        }
        self.write(tmp);
        self.write("[");
        self.write(idx);
        self.write("]");
    }

    /// Convert a PropName to a string suitable for property access.
    /// Returns `Cow::Borrowed` for the common Ident/String/Number cases (no
    /// allocation), `Cow::Owned` only for Private (`#name`) and Computed.
    fn prop_name_to_access_str<'k>(&self, key: &'k PropName) -> std::borrow::Cow<'k, str> {
        use std::borrow::Cow;
        match key {
            PropName::Ident(s, _) | PropName::String(s, _) => Cow::Borrowed(s.as_str()),
            PropName::Number(s, _) => Cow::Borrowed(s.as_str()),
            PropName::Private(s, _) => Cow::Owned(format!("#{}", s)),
            PropName::Computed(_, _) => Cow::Borrowed("/* computed */"),
        }
    }

    fn emit_compact_multiline_arrow_binary_expr(&mut self, span: Span) {
        let start = span.start as usize;
        let end = span.end as usize;
        if start >= end || end > self.source.len() {
            return;
        }
        let src = &self.source[start..end];
        let mut first = true;
        for line in src.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            // Five chain stages collapsed into one unified pass.
            let normalized = if self.skip_brace_normalize {
                std::borrow::Cow::Borrowed(trimmed)
            } else {
                normalize_brace_spacing(trimmed)
            };
            let normalized = normalize_unified_pass(&normalized);
            if first {
                self.write(&normalized);
                first = false;
            } else {
                self.newline();
                self.write("    ");
                self.write(&normalized);
            }
        }
        if first {
            self.copy_expr_span(span);
        }
    }

    fn emit_compact_multiline_operator_expr(&mut self, span: Span) {
        let start = span.start as usize;
        let end = span.end as usize;
        if start >= end || end > self.source.len() {
            return;
        }
        let src = &self.source[start..end];
        let mut first = true;
        for line in src.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            // Five chain stages collapsed into one unified pass.
            let normalized = if self.skip_brace_normalize {
                std::borrow::Cow::Borrowed(trimmed)
            } else {
                normalize_brace_spacing(trimmed)
            };
            let normalized = normalize_unified_pass(&normalized);
            if first {
                self.write(&normalized);
                first = false;
            } else {
                self.newline();
                self.write("    ");
                self.write(&normalized);
            }
        }
        if first {
            self.copy_expr_span(span);
        }
    }

    /// Returns true when an object literal method was produced by parser error
    /// recovery (the source has no `{` for the method body).  TypeScript strips
    /// these members from the emit output.  Only applies to `Method` variants;
    /// accessors without bodies are kept (with an empty body `{ }`).
    fn is_error_recovery_obj_method(&self, prop: &ObjLitProp) -> bool {
        let span = match prop {
            ObjLitProp::Method(m) => m.span,
            _ => return false,
        };
        let start = span.start as usize;
        let end = span.end as usize;
        if start >= end || end > self.source.len() {
            return false;
        }
        // A valid method body requires `{` in the source text. A signature
        // followed by a comma is the one recovery form tsc emits as an empty
        // method; other body-less methods are stripped.
        if self.source[start..end].contains('{') {
            return false;
        }
        self.source[end..].chars().find(|ch| !ch.is_whitespace()) != Some(',')
    }

    fn emit_recovery_invalid_object_member_expressions(&mut self, start: u32) -> bool {
        if !self.file_has_recovery_errors {
            return false;
        }
        let start = start as usize;
        if self.source.as_bytes().get(start) != Some(&b'{') {
            return false;
        }
        let Some(close_rel) = self.source[start + 1..].find('}') else {
            return false;
        };
        let close = start + 1 + close_rel;
        let body = &self.source[start + 1..close];
        if body.contains('{') {
            return false;
        }

        let mut recovered = Vec::new();
        for line in body.lines().map(str::trim).filter(|line| !line.is_empty()) {
            let ident_end = line
                .char_indices()
                .take_while(|(_, ch)| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '$'))
                .map(|(index, ch)| index + ch.len_utf8())
                .last()
                .unwrap_or(0);
            if ident_end == 0 {
                return false;
            }
            let ident = &line[..ident_end];
            let tail = line[ident_end..].trim_start();
            let normalized = if tail.starts_with('.') {
                let (code, comment) = tail
                    .split_once("//")
                    .map_or((tail.trim_end(), None), |(code, comment)| {
                        (code.trim_end(), Some(comment))
                    });
                let mut output = format!("{ident}, : {code}");
                if let Some(comment) = comment {
                    output.push_str(" //");
                    output.push_str(comment);
                }
                output
            } else if tail.starts_with('[') {
                let Some(bracket_end) = tail.find(']') else {
                    return false;
                };
                let key = &tail[..=bracket_end];
                let after_key = tail[bracket_end + 1..].trim();
                if after_key != "," {
                    return false;
                }
                format!("{ident}, {key}: ,")
            } else {
                return false;
            };
            recovered.push(normalized);
        }
        if recovered.is_empty() {
            return false;
        }

        self.writeln("{");
        self.indent += 1;
        for line in recovered {
            self.writeln(&line);
        }
        self.indent -= 1;
        self.write("}");
        let mut skip_end = close + 1;
        while self
            .source
            .as_bytes()
            .get(skip_end)
            .is_some_and(|byte| byte.is_ascii_whitespace())
        {
            skip_end += 1;
        }
        if self.source.as_bytes().get(skip_end) == Some(&b';') {
            skip_end += 1;
        }
        let skip_end = skip_end as u32;
        self.skip_recovery_until = self.skip_recovery_until.max(skip_end);
        self.advance_comment_pos(skip_end);
        true
    }

    fn obj_prop_binding_name_for_class_expr(
        &mut self,
        key: &PropName,
    ) -> Option<ClassExprBindingName> {
        match key {
            PropName::Ident(name, _) | PropName::String(name, _) => (name.as_str() != "__proto__")
                .then(|| ClassExprBindingName::Literal(name.to_string())),
            PropName::Number(name, _) => Some(ClassExprBindingName::Literal(name.to_string())),
            PropName::Computed(expr, _) => Self::computed_name_literal_text(expr)
                .map(ClassExprBindingName::Literal)
                .or_else(|| {
                    self.needs_prop_key_helper = true;
                    Some(ClassExprBindingName::Expr(self.next_temp_var()))
                }),
            PropName::Private(_, _) => None,
        }
    }

    fn emit_obj_prop_name_for_class_expr(
        &mut self,
        key: &PropName,
        binding_name: Option<&ClassExprBindingName>,
    ) {
        if let (PropName::Computed(expr, _), Some(ClassExprBindingName::Expr(temp))) =
            (key, binding_name)
        {
            self.write("[");
            self.write(temp);
            self.write(" = ");
            self.write(self.helper_prefix());
            self.write("__propKey(");
            self.emit_expr(expr);
            self.write(")]");
            return;
        }
        self.emit_prop_name(key);
    }

    pub(super) fn emit_obj_lit_prop(&mut self, prop: &ObjLitProp) {
        let destructuring_pattern = self.emitting_destructuring_assignment_pattern;
        match prop {
            ObjLitProp::Property(p) => {
                if destructuring_pattern {
                    self.emitting_destructuring_assignment_pattern = false;
                }
                let binding_name = Self::expr_needs_class_expr_binding_name(&p.value)
                    .then(|| self.obj_prop_binding_name_for_class_expr(&p.key))
                    .flatten();
                // Private names in object literals are an error — TypeScript
                // strips the name entirely, emitting just `: value`.
                if !matches!(&p.key, PropName::Private(_, _)) {
                    self.emit_obj_prop_name_for_class_expr(&p.key, binding_name.as_ref());
                }
                self.emitting_destructuring_assignment_pattern = destructuring_pattern;
                self.write(": ");
                // Don't add newline before value if the value is an error
                // placeholder (from error recovery) — it would be skipped.
                let is_error_value = matches!(&p.value.kind,
                    ExprKind::Ident(name) if name == "<error>" || name.is_empty());
                if !is_error_value && self.obj_prop_value_starts_on_new_line(p) {
                    self.newline();
                }
                self.emit_leading_comments(p.value.span.start);
                let prev_binding_name = self.class_expr_binding_name.clone();
                if let Some(binding_name) = binding_name.as_ref() {
                    self.class_expr_binding_name = Some(binding_name.clone());
                }
                self.suppress_oc_parens = true;
                if destructuring_pattern {
                    self.emit_destructuring_assignment_target(&p.value);
                } else {
                    self.emit_expr(&p.value);
                }
                self.class_expr_binding_name = prev_binding_name;
            }
            ObjLitProp::Shorthand(name, span) => {
                if let Some(binding) = self.lexical_downlevel_plan.binding_for_reference(*span) {
                    let renamed = binding.emitted_name != binding.source_name;
                    if renamed
                        || (self.generator_catch_scope
                            && binding.kind == lexical_downlevel::BindingKind::Catch)
                    {
                        let value_name = binding.emitted_name.clone();
                        self.write(name);
                        if renamed {
                            self.write(": ");
                            self.write(&value_name);
                        }
                        return;
                    }
                }
                let ns_qualify = self
                    .export_target
                    .as_ref()
                    .filter(|t| *t != "exports")
                    .is_some_and(|_| {
                        (self.namespace_exports.contains(name.as_str())
                            || self
                                .ns_export_stack
                                .iter()
                                .any(|(_, exports)| exports.contains(name.as_str())))
                            && !self.ns_local_bindings.contains(name.as_str())
                    });
                let needs_expand = ns_qualify
                    || self.cjs_import_map.get(name.as_str()).is_some_and(
                        |(var_name, imported)| {
                            !imported.is_empty() || var_name.as_str() != name.as_str()
                        },
                    );
                // CJS: shorthand prop referencing an exported var must expand
                // `{ test }` → `{ test: exports.test }`
                let cjs_expand = self.export_target.as_ref().is_some_and(|t| t == "exports")
                    && self.cjs_var_export_names.contains(name.as_str());
                if cjs_expand {
                    self.write(name);
                    self.write(": exports.");
                    self.write(name);
                } else if needs_expand {
                    self.write(name);
                    self.write(": ");
                    self.emit_value_name_ref(name);
                } else {
                    self.write(name);
                    // Reserved keywords can't be shorthand property values
                    // in object literals — they need `: ` (empty value).
                    if crate::emit_stmt::is_js_reserved_keyword(name) {
                        self.write(": ");
                    }
                }
            }
            ObjLitProp::Spread(expr, spread_span) => {
                self.write("...");
                let dot_end = spread_span.start as usize + 3;
                if !self.emit_compact_spread_comment(dot_end) {
                    self.emit_compact_comment_between(dot_end, expr.span.start as usize, true);
                }
                if destructuring_pattern {
                    self.emit_destructuring_assignment_target(expr);
                } else {
                    self.emit_expr(expr);
                }
            }
            ObjLitProp::Method(method) => {
                let saved_arguments_alias = self.current_arguments_alias.take();
                let dl_async_obj =
                    method.is_async && !method.is_generator && self.needs_downlevel("async");
                let dl_async_gen_obj = method.is_async
                    && method.is_generator
                    && self.needs_downlevel("async-generator");
                let async_gen_move_params =
                    dl_async_gen_obj && method.params.iter().any(Self::param_needs_async_lift);
                let downlevel_simple_params = !method.is_async
                    && !method.is_generator
                    && matches!(method.name, PropName::Ident(_, _))
                    && !method.body.iter().any(stmt_has_super)
                    && self.can_downlevel_simple_param_initializers(&method.params);
                if !downlevel_simple_params {
                    if method.is_async && !(dl_async_obj || dl_async_gen_obj) {
                        self.write("async ");
                    }
                    if method.is_generator && !dl_async_gen_obj {
                        self.write("*");
                    }
                }
                // Private names in object literals are an error — TypeScript
                // strips the name, emitting just `() { ... }`.
                if !matches!(&method.name, PropName::Private(_, _)) {
                    self.emit_prop_name(&method.name);
                }
                if downlevel_simple_params {
                    self.write(": function ");
                }
                self.write("(");
                if async_gen_move_params {
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
                } else if downlevel_simple_params {
                    self.emit_params_without_initializers(&method.params);
                } else {
                    self.emit_params(&method.params);
                }
                self.write(") ");
                // Skip comments inside erased return type annotation
                if let Some(ref rt) = method.return_type {
                    self.advance_comment_pos(rt.span.end);
                }
                let prev_in_parameter_initializer = self.in_parameter_initializer;
                self.in_parameter_initializer = false;
                if dl_async_obj || dl_async_gen_obj {
                    self.awaiter_enclosing_span = Some(method.span);
                    if dl_async_gen_obj {
                        let inner_name = method.name.ident_name().map(|n| format!("{n}_1"));
                        self.emit_async_generator_body(
                            &method.body,
                            inner_name.as_deref(),
                            async_gen_move_params.then_some(method.params.as_slice()),
                        );
                    } else {
                        let async_arguments_alias = if stmts_have_lexical_arguments(&method.body) {
                            Some(self.next_arguments_capture_name())
                        } else {
                            None
                        };
                        self.emit_awaiter_body_with_params(
                            &method.body,
                            None,
                            &method.params,
                            async_arguments_alias.as_deref(),
                        );
                    }
                } else if downlevel_simple_params {
                    self.emit_block_for_decl_body_with_param_initializers(
                        &method.params,
                        &method.body,
                        method.span,
                    );
                } else {
                    self.emit_block_for_decl_body(&method.body, method.span);
                }
                self.current_arguments_alias = saved_arguments_alias;
                self.in_parameter_initializer = prev_in_parameter_initializer;
            }
            ObjLitProp::Get(acc) => {
                let saved_arguments_alias = self.current_arguments_alias.take();
                self.write("get ");
                if !matches!(&acc.name, PropName::Private(_, _)) {
                    self.emit_prop_name(&acc.name);
                }
                self.write("(");
                self.emit_params(&acc.params);
                self.write(") ");
                // Skip comments inside erased return type annotation
                if let Some(ref rt) = acc.return_type {
                    self.advance_comment_pos(rt.span.end);
                }
                self.emit_block_for_decl_body(&acc.body, acc.span);
                self.current_arguments_alias = saved_arguments_alias;
            }
            ObjLitProp::Set(acc) => {
                let saved_arguments_alias = self.current_arguments_alias.take();
                self.write("set ");
                if !matches!(&acc.name, PropName::Private(_, _)) {
                    self.emit_prop_name(&acc.name);
                }
                self.write("(");
                self.emit_params(&acc.params);
                self.write(") ");
                self.emit_block_for_decl_body(&acc.body, acc.span);
                self.current_arguments_alias = saved_arguments_alias;
            }
            ObjLitProp::ShorthandDefault(name, default_expr, _) => {
                self.write(name);
                self.write(" = ");
                let prev_binding_name = self.class_expr_binding_name.clone();
                if matches!(&default_expr.kind, ExprKind::ClassExpr(cd) if cd.name.is_none() && (class_has_static_initializers(cd) || !cd.decorators.is_empty() || class_has_member_decorators(cd)))
                {
                    self.class_expr_binding_name =
                        Some(ClassExprBindingName::Literal(name.to_string()));
                }
                if destructuring_pattern {
                    self.emitting_destructuring_assignment_pattern = false;
                }
                self.emit_expr(default_expr);
                self.emitting_destructuring_assignment_pattern = destructuring_pattern;
                self.class_expr_binding_name = prev_binding_name;
            }
        }
    }

    /// Lower the ES2015 computed-name transform for the deliberately bounded
    /// object-literal subset that can be represented without semantic services:
    ///
    /// `{ a: before(), [key()]: value(), m() {} }`
    ///   -> `(_a = { a: before() }, _a[key()] = value(),
    ///       _a.m = function () {}, _a)`
    ///
    /// The temporary preserves construction identity and source evaluation
    /// order. Methods become ordinary function values so their dynamic `this`
    /// and `arguments` bindings match a downleveled object method.
    fn try_emit_computed_object_literal_downlevel(
        &mut self,
        props: &[ObjLitProp],
        expr: &Expr,
    ) -> bool {
        self.try_emit_computed_object_literal_downlevel_with_wrap(props, expr, true)
    }

    pub(super) fn try_emit_direct_return_computed_object(&mut self, expr: &Expr) -> bool {
        if let ExprKind::ObjectLit(props) = &expr.kind {
            return self.try_emit_computed_object_literal_downlevel_with_wrap(props, expr, false);
        }
        false
    }

    fn emit_expr_with_computed_object_downlevel_suppressed(&mut self, expr: &Expr) {
        let previous_suppression = self.suppress_computed_object_downlevel;
        self.suppress_computed_object_downlevel |=
            self.emit_expr_depth == 1 && Self::type_layer_subject_is_object(expr);
        self.emit_expr(expr);
        self.suppress_computed_object_downlevel = previous_suppression;
    }

    /// Whether a type layer directly owns an object expression, allowing only
    /// parentheses and more type layers between them. Runtime expressions such
    /// as conditionals are boundaries: their nested objects remain independent
    /// transform candidates.
    fn type_layer_subject_is_object(expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::ObjectLit(_) => true,
            ExprKind::Paren(inner) | ExprKind::NonNull(inner) => {
                Self::type_layer_subject_is_object(inner)
            }
            ExprKind::As(wrapper) => Self::type_layer_subject_is_object(&wrapper.expr),
            ExprKind::Satisfies(wrapper) => Self::type_layer_subject_is_object(&wrapper.expr),
            ExprKind::TypeAssertion(wrapper) => Self::type_layer_subject_is_object(&wrapper.expr),
            ExprKind::Instantiation(wrapper) => Self::type_layer_subject_is_object(&wrapper.expr),
            _ => false,
        }
    }

    pub(super) fn try_emit_computed_object_literal_downlevel_with_wrap(
        &mut self,
        props: &[ObjLitProp],
        expr: &Expr,
        wrap: bool,
    ) -> bool {
        if self.suppress_computed_object_downlevel
            || self.in_parameter_initializer
            || self.emitting_destructuring_assignment_pattern
            || !self.computed_object_literal_can_downlevel(props, expr.span)
        {
            return false;
        }

        let first_computed = props
            .iter()
            .position(Self::obj_lit_prop_is_computed)
            .expect("supported computed object has a computed member");
        let multiline = self.computed_object_literal_is_multiline(expr.span);

        let temp = self.next_computed_obj_temp_var();
        if wrap {
            self.write("(");
        }
        self.write(&temp);
        self.write(" = ");
        self.write("{");
        if first_computed > 0 {
            if multiline {
                // TypeScript gives the retained object-literal prefix one
                // additional indentation level inside the assignment
                // sequence. This remains relative to the structural emitter
                // indentation even when the sequence starts inside a call.
                self.indent += 2;
                self.newline();
                for (index, prop) in props[..first_computed].iter().enumerate() {
                    if index > 0 {
                        self.write(",");
                        self.newline();
                    }
                    self.emit_computed_obj_initial_prop(prop);
                }
                self.indent -= 1;
                self.newline();
                self.write("}");
            } else {
                self.write(" ");
                for (index, prop) in props[..first_computed].iter().enumerate() {
                    if index > 0 {
                        self.write(", ");
                    }
                    self.emit_computed_obj_initial_prop(prop);
                }
                self.write(" }");
            }
        } else if multiline {
            self.indent += 1;
        }
        if first_computed == 0 {
            self.write("}");
        }

        for prop in &props[first_computed..] {
            self.write(",");
            if multiline {
                self.newline();
            } else {
                self.write(" ");
            }
            match prop {
                ObjLitProp::Property(property) => {
                    self.write(&temp);
                    self.emit_member_access(&property.key);
                    self.write(" = ");
                    self.suppress_oc_parens = true;
                    self.emit_computed_obj_assigned_value(&property.value);
                }
                ObjLitProp::Shorthand(name, _) => {
                    self.write(&temp);
                    self.write(".");
                    self.write(name);
                    self.write(" = ");
                    self.emit_value_name_ref(name);
                }
                ObjLitProp::Method(method) => {
                    self.write(&temp);
                    self.emit_member_access(&method.name);
                    self.write(" = ");
                    self.emit_computed_obj_method_function(method);
                }
                ObjLitProp::Get(accessor) => {
                    self.emit_computed_obj_accessor_definition(&temp, accessor, true);
                }
                ObjLitProp::Set(accessor) => {
                    self.emit_computed_obj_accessor_definition(&temp, accessor, false);
                }
                ObjLitProp::ShorthandDefault(_, _, _) | ObjLitProp::Spread(_, _) => {
                    unreachable!("unsupported computed object member")
                }
            }
        }
        self.write(",");
        if multiline {
            self.newline();
        } else {
            self.write(" ");
        }
        self.write(&temp);
        if wrap {
            self.write(")");
        }
        if multiline {
            self.indent -= 1;
        }
        true
    }

    pub(crate) fn computed_object_literal_can_downlevel(
        &self,
        props: &[ObjLitProp],
        span: Span,
    ) -> bool {
        if self.effective_target() >= ScriptTarget::ES2015 || props.is_empty() {
            return false;
        }

        // Comments have materially different attachment rules. Keep them on
        // the native fallback until the transform can preserve their exact
        // ownership; multiline data/method members are handled structurally.
        let start = span.start as usize;
        let end = span.end as usize;
        if start >= end || end > self.source.len() {
            return false;
        }
        let source = &self.source[start..end];
        let has_unsupported_keyword = source
            .split(|ch: char| !(ch == '_' || ch == '$' || ch.is_alphanumeric()))
            .any(|word| matches!(word, "super" | "class"));
        if source.contains("//") || source.contains("/*") || has_unsupported_keyword {
            return false;
        }

        if !props.iter().any(Self::obj_lit_prop_is_computed) {
            return false;
        }

        // Spread, recovery-only shorthand defaults/private names, non-computed
        // accessors, and members needing an additional parameter/async/generator
        // transform are intentionally outside this independently verifiable cluster.
        if !props
            .iter()
            .all(|prop| self.computed_obj_lit_prop_is_supported(prop))
        {
            return false;
        }
        true
    }

    fn computed_object_literal_is_multiline(&self, span: Span) -> bool {
        let start = span.start as usize;
        let end = span.end as usize;
        start < end
            && end <= self.source.len()
            && self.source[start..end]
                .chars()
                .any(|ch| ch == '\n' || ch == '\r')
    }

    fn next_computed_obj_temp_var(&mut self) -> String {
        let name = self.make_temp_name();
        if self
            .namespace_iife_fn_scope_depths
            .last()
            .is_some_and(|depth| *depth == self.fn_scope_depth)
        {
            self.ns_temp_var_names.push(name.clone());
        } else {
            self.temp_var_names.push(name.clone().into());
        }
        name
    }

    fn obj_lit_prop_is_computed(prop: &ObjLitProp) -> bool {
        match prop {
            ObjLitProp::Property(property) => {
                matches!(property.key, PropName::Computed(_, _))
            }
            ObjLitProp::Method(method) => matches!(method.name, PropName::Computed(_, _)),
            ObjLitProp::Get(accessor) | ObjLitProp::Set(accessor) => {
                matches!(accessor.name, PropName::Computed(_, _))
            }
            _ => false,
        }
    }

    fn computed_obj_lit_prop_is_supported(&self, prop: &ObjLitProp) -> bool {
        match prop {
            ObjLitProp::Property(property) => {
                Self::computed_obj_prop_name_is_supported(&property.key)
                    && !matches!(&property.value.kind, ExprKind::Ident(name) if name.is_empty() || name == "<error>")
                    && !Self::expr_needs_class_expr_binding_name(&property.value)
                    && self.computed_obj_value_is_supported(&property.value)
            }
            ObjLitProp::Shorthand(name, _) => {
                !name.is_empty()
                    && name != "<error>"
                    && !crate::emit_stmt::is_js_reserved_keyword(name)
            }
            ObjLitProp::Method(method) => {
                !method.is_async
                    && !method.is_generator
                    && Self::computed_obj_prop_name_is_supported(&method.name)
                    && method.params.iter().all(|param| {
                        matches!(&param.name.kind, PatKind::Ident(name)
                            if !name.is_empty()
                                && name != "<error>"
                                && !crate::emit_stmt::is_js_reserved_keyword(name))
                            && param.initializer.is_none()
                            && !param.dotdotdot
                            && param.decorators.is_empty()
                    })
            }
            ObjLitProp::Get(accessor) => {
                matches!(accessor.name, PropName::Computed(_, _)) && accessor.params.is_empty()
            }
            ObjLitProp::Set(accessor) => {
                matches!(accessor.name, PropName::Computed(_, _))
                    && matches!(accessor.params.as_slice(), [param]
                        if matches!(&param.name.kind, PatKind::Ident(name)
                            if !name.is_empty()
                                && name != "<error>"
                                && !crate::emit_stmt::is_js_reserved_keyword(name))
                            && param.initializer.is_none()
                            && !param.dotdotdot
                            && param.decorators.is_empty()
                            && param.modifiers == MOD_NONE)
            }
            ObjLitProp::ShorthandDefault(_, _, _) | ObjLitProp::Spread(_, _) => false,
        }
    }

    fn computed_obj_prop_name_is_supported(name: &PropName) -> bool {
        match name {
            PropName::Ident(name, _) => !name.is_empty() && name != "<error>",
            PropName::String(_, _) | PropName::Number(_, _) => true,
            PropName::Computed(expr, _) => {
                !matches!(&expr.kind, ExprKind::Ident(name) if name.is_empty() || name == "<error>")
            }
            PropName::Private(_, _) => false,
        }
    }

    fn computed_obj_value_is_supported(&self, value: &Expr) -> bool {
        let ExprKind::Arrow(arrow) = &value.kind else {
            return true;
        };
        !arrow.is_async
            && !arrow_has_lexical_environment_hazard(arrow)
            && arrow.params.iter().all(|param| {
                matches!(&param.name.kind, PatKind::Ident(name)
                    if !name.is_empty()
                        && name != "<error>"
                        && !crate::emit_stmt::is_js_reserved_keyword(name)
                        && !self.cjs_var_export_names.contains(name.as_str())
                        && !self.namespace_exports.contains(name.as_str()))
                    && param.initializer.is_none()
                    && !param.dotdotdot
                    && param.decorators.is_empty()
                    && param.modifiers == MOD_NONE
            })
    }

    fn emit_computed_obj_initial_prop(&mut self, prop: &ObjLitProp) {
        match prop {
            ObjLitProp::Property(property) => {
                self.emit_prop_name(&property.key);
                self.write(": ");
                self.emit_computed_obj_assigned_value(&property.value);
            }
            ObjLitProp::Shorthand(name, _) => {
                self.write(name);
                self.write(": ");
                self.emit_value_name_ref(name);
            }
            ObjLitProp::Method(method) => {
                self.emit_prop_name(&method.name);
                self.write(": ");
                self.emit_computed_obj_method_function(method);
            }
            _ => unreachable!("unsupported computed object prefix member"),
        }
    }

    fn emit_computed_obj_assigned_value(&mut self, value: &Expr) {
        let ExprKind::Arrow(arrow) = &value.kind else {
            self.emit_expr(value);
            return;
        };

        self.write("function (");
        self.emit_params(&arrow.params);
        self.write(") ");
        if let Some(return_type) = &arrow.return_type {
            self.advance_comment_pos(return_type.span.end);
        }
        match &arrow.body {
            ArrowBody::Expr(body) => {
                self.write("{ return ");
                self.emit_expr(body);
                self.write("; }");
            }
            ArrowBody::Block(stmts) => self.emit_block_for_decl_body(stmts, arrow.span),
        }
    }

    fn emit_computed_obj_method_function(&mut self, method: &ObjMethod) {
        // An object method creates both of these bindings. Clear any aliases
        // inherited from a surrounding transformed class/async context while
        // emitting its ordinary-function replacement.
        let saved_static_this_alias = self.static_this_alias.take();
        let saved_arguments_alias = self.current_arguments_alias.take();
        self.write("function (");
        self.emit_params(&method.params);
        self.write(") ");
        if let Some(return_type) = &method.return_type {
            self.advance_comment_pos(return_type.span.end);
        }
        self.emit_block_for_decl_body(&method.body, method.span);
        self.current_arguments_alias = saved_arguments_alias;
        self.static_this_alias = saved_static_this_alias;
    }

    fn emit_computed_obj_accessor_definition(
        &mut self,
        temp: &str,
        accessor: &ObjAccessor,
        is_getter: bool,
    ) {
        let PropName::Computed(key, _) = &accessor.name else {
            unreachable!("supported computed object accessor must have a computed name");
        };

        self.write("Object.defineProperty(");
        self.write(temp);
        self.write(", ");
        self.emit_expr(key);
        self.writeln(", {");
        self.indent += 1;
        self.write(if is_getter { "get: " } else { "set: " });
        self.emit_computed_obj_accessor_function(accessor);
        self.writeln(",");
        self.writeln("enumerable: false,");
        self.writeln("configurable: true");
        self.indent -= 1;
        self.write("})");
    }

    fn emit_computed_obj_accessor_function(&mut self, accessor: &ObjAccessor) {
        // Like an object method, an accessor owns `this` and `arguments`; aliases
        // inherited from a surrounding downlevel transform must not leak in.
        let saved_static_this_alias = self.static_this_alias.take();
        let saved_arguments_alias = self.current_arguments_alias.take();
        self.write("function (");
        self.emit_params(&accessor.params);
        self.write(") ");
        if let Some(return_type) = &accessor.return_type {
            self.advance_comment_pos(return_type.span.end);
        }
        let shadowed = self.hide_cjs_imports_shadowed_by_params(&accessor.params);
        self.emit_block_for_decl_body(&accessor.body, accessor.span);
        self.restore_shadowed_cjs_imports(shadowed);
        self.current_arguments_alias = saved_arguments_alias;
        self.static_this_alias = saved_static_this_alias;
    }

    fn obj_prop_value_starts_on_new_line(&self, prop: &ObjProp) -> bool {
        let start = prop.span.start as usize;
        let value_start = prop.value.span.start as usize;
        if start >= value_start || value_start > self.source.len() {
            return false;
        }
        let prefix = &self.source[start..value_start];
        let Some(colon_idx) = prefix.rfind(':') else {
            return false;
        };
        prefix[colon_idx + 1..]
            .chars()
            .any(|ch| ch == '\n' || ch == '\r')
    }

    pub(super) fn emit_template(&mut self, tpl: &TemplateLit) {
        self.emit_template_with_mode(tpl, false);
    }

    pub(super) fn emit_template_await_to_yield(&mut self, tpl: &TemplateLit) {
        self.emit_template_with_mode(tpl, true);
    }

    fn emit_template_with_mode(&mut self, tpl: &TemplateLit, await_to_yield: bool) {
        if self.effective_target() < ScriptTarget::ES2015 {
            self.write("\"");
            self.write(&downlevel_template_string(
                tpl.quasis.first().map_or("", |quasi| &quasi.raw),
            ));
            self.write("\"");
            for (i, expr) in tpl.exprs.iter().enumerate() {
                self.write(".concat(");
                let leading_start = tpl
                    .quasis
                    .get(i)
                    .map_or(expr.span.start, |quasi| quasi.span.end);
                self.emit_template_comments_in_range(leading_start, expr.span.start);
                let is_comma = matches!(expr.kind, ExprKind::Comma(_));
                if is_comma {
                    self.write("(");
                }
                if await_to_yield {
                    self.emit_expr_await_to_yield(expr);
                } else {
                    self.emit_expr(expr);
                }
                if is_comma {
                    self.write(")");
                }
                if let Some(quasi) = tpl.quasis.get(i + 1) {
                    self.emit_template_comments_in_range(expr.span.end, quasi.span.start);
                }
                if let Some(quasi) = tpl.quasis.get(i + 1) {
                    let cooked = downlevel_template_string(&quasi.raw);
                    if !cooked.is_empty() {
                        self.write(", \"");
                        self.write(&cooked);
                        self.write("\"");
                    }
                }
                self.write(")");
            }
            return;
        }

        self.emit_template_syntax_with_mode(tpl, await_to_yield);
    }

    /// Emit backtick template syntax without applying the untagged ES5 string
    /// transform. Tagged templates call this after their tag/helper decision so
    /// an outer tagged quasi can never be mistaken for an untagged expression.
    fn emit_template_syntax(&mut self, tpl: &TemplateLit) {
        self.emit_template_syntax_with_mode(tpl, false);
    }

    fn emit_template_syntax_with_mode(&mut self, tpl: &TemplateLit, await_to_yield: bool) {
        // Unterminated templates carry a zero-span recovery tail quasi. tsc
        // emits them up to the last opened `${` and stops — no `}`, no
        // closing backtick (`f \`123${1}${;`). A `}` is only written when a
        // REAL continuation quasi follows the interpolation.
        let is_recovery_quasi = |q: &tsc_rs_ast::TemplateElement| q.span.start == q.span.end;
        self.write("`");
        for (i, quasi) in tpl.quasis.iter().enumerate() {
            if i > 0 {
                if is_recovery_quasi(quasi) {
                    return; // unterminated — stop before `}`/backtick
                }
                self.write("}");
            }
            self.write(&quasi.raw);
            if i < tpl.exprs.len() {
                self.write("${");
                let e = &tpl.exprs[i];
                let missing = matches!(e.kind, tsc_rs_ast::ExprKind::Omitted)
                    || (e.span.start == e.span.end
                        && matches!(&e.kind, tsc_rs_ast::ExprKind::Ident(n) if n == "<error>"));
                if !missing {
                    if await_to_yield {
                        self.emit_expr_await_to_yield(e);
                    } else {
                        self.emit_expr(e);
                    }
                }
            }
        }
        self.write("`");
    }

    /// Preserve comments that live in template interpolation trivia rather
    /// than inside the parsed expression span. Copying the exact trivia range
    /// retains line-comment newlines and annotations such as `/*#__PURE__*/`.
    fn emit_template_comments_in_range(&mut self, start: u32, end: u32) {
        if start >= end
            || (self.options.remove_comments == Some(true) && !self.preserve_comments)
            || !self.has_comments_in_range(start, end)
        {
            return;
        }
        self.copy_expr_span_raw(Span::new(start, end));
        self.advance_comment_pos(end);
    }

    /// Tagged-template lowering duplicates interpolation comments: once at
    /// the corresponding raw-array boundary and once around the substitution
    /// argument. Replay the raw-array copy without advancing global comment
    /// state so the argument copy can still be emitted later.
    fn emit_tagged_template_raw_boundary_comments(
        &mut self,
        start: u32,
        end: u32,
        after_raw_value: bool,
    ) {
        if start >= end || (self.options.remove_comments == Some(true) && !self.preserve_comments) {
            if !after_raw_value {
                self.write(" ");
            }
            return;
        }

        let comments: Vec<(String, bool)> = self
            .comments
            .iter()
            .filter(|comment| comment.pos >= start && comment.pos < end)
            .filter_map(|comment| {
                let comment_start = comment.pos as usize;
                let comment_end = (comment.end as usize).min(self.source.len());
                (comment_start < comment_end).then(|| {
                    (
                        self.source[comment_start..comment_end].to_string(),
                        comment.is_multiline,
                    )
                })
            })
            .collect();

        if comments.is_empty() {
            if !after_raw_value {
                self.write(" ");
            }
            return;
        }

        for (index, (comment, is_block)) in comments.iter().enumerate() {
            if *is_block {
                self.write(" ");
                self.write(comment.trim());
            } else {
                self.newline();
                self.write(comment.trim_end());
                self.newline();
            }
            if !after_raw_value && index + 1 == comments.len() {
                self.write(" ");
            }
        }
    }

    fn comment_bounds_in_range(
        &self,
        range_start: u32,
        range_end: u32,
    ) -> Option<(u32, u32, bool)> {
        if range_start >= range_end {
            return None;
        }
        let mut first_pos = None;
        let mut last_end = 0;
        let mut last_is_line = false;
        for c in self.comments.iter().skip(self.next_comment_idx) {
            if c.pos >= range_end {
                break;
            }
            if c.pos < range_start || c.pos < self.comment_emit_pos {
                continue;
            }
            if first_pos.is_none() {
                first_pos = Some(c.pos);
            }
            last_end = c.end;
            last_is_line = !c.is_multiline;
        }
        first_pos.map(|first_pos| (first_pos, last_end, last_is_line))
    }

    pub(super) fn emit_paren_comments_in_range(&mut self, range_start: u32, range_end: u32) {
        if self.options.remove_comments == Some(true) && !self.preserve_comments {
            return;
        }
        let Some((first_pos, last_end, last_is_line)) =
            self.comment_bounds_in_range(range_start, range_end)
        else {
            return;
        };
        let source_len = self.source.len();
        let range_start = (range_start as usize).min(source_len);
        let range_end = (range_end as usize).min(source_len);
        let first_pos = (first_pos as usize).min(source_len);
        let last_end = (last_end as usize).min(source_len);
        if first_pos >= last_end || range_start > first_pos || last_end > range_end {
            return;
        }

        let before = &self.source[range_start..first_pos];
        let after = &self.source[last_end..range_end];
        let comment_text = &self.source[first_pos..last_end];
        let has_leading_newline = before.contains('\n') || before.contains('\r');
        let has_trailing_newline = after.contains('\n') || after.contains('\r');
        let uses_multiline_layout =
            has_leading_newline || comment_text.contains('\n') || comment_text.contains('\r');

        if has_leading_newline {
            if !self.at_line_start {
                self.newline();
            }
        } else if !self.at_line_start {
            self.write(" ");
        }

        self.copy_expr_span(Span::new(first_pos as u32, last_end as u32));
        self.advance_comment_pos(last_end as u32);

        if last_is_line || has_trailing_newline {
            if !self.at_line_start {
                self.newline();
            }
        } else if uses_multiline_layout {
            self.write(" ");
        }
    }

    pub(super) fn member_operator_positions(
        &self,
        expr_span: Span,
        mem: &MemberExpr,
    ) -> Option<(usize, usize, usize)> {
        if mem.property == "<error>" {
            return None;
        }
        let obj_end = mem.object.span.end as usize;
        let expr_end = expr_span.end as usize;
        if obj_end >= expr_end || expr_end > self.source.len() {
            return None;
        }
        let op_text = if mem.optional { "?." } else { "." };
        let after_obj = &self.source[obj_end..expr_end];
        let op_rel = after_obj.find(op_text)?;
        let op_start = obj_end + op_rel;
        let op_end = op_start + op_text.len();
        let after_op = &self.source[op_end..expr_end];
        let prop_rel = after_op.rfind(mem.property.as_str())?;
        Some((op_start, op_end, op_end + prop_rel))
    }

    fn emit_member_gap_before_operator(&mut self, gap_text: &str, indent_levels: u32) -> bool {
        if gap_text.trim().is_empty() {
            return false;
        }
        let indent_str = "    ".repeat(indent_levels as usize);
        let parts: Vec<&str> = gap_text.split('\n').collect();
        let last_is_ws_only = parts.last().is_some_and(|line| line.trim().is_empty());
        let mut operator_shares_line_with_comment = false;

        for (i, line) in parts.iter().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if i == 0 && !gap_text.starts_with('\n') {
                self.write(" ");
                self.write(trimmed);
            } else {
                self.newline();
                if !indent_str.is_empty() {
                    self.write(&indent_str);
                }
                self.write(trimmed);
                operator_shares_line_with_comment = true;
            }
        }

        if last_is_ws_only {
            self.newline();
            if !indent_str.is_empty() {
                self.write(&indent_str);
            }
            return false;
        }

        gap_text.contains('\n') && operator_shares_line_with_comment
    }

    fn emit_member_gap_after_operator(
        &mut self,
        gap_text: &str,
        indent_levels: u32,
        preserve_block_comments: bool,
        preserve_line_comments: bool,
    ) -> bool {
        if gap_text.trim().is_empty()
            || (self.options.remove_comments == Some(true) && !self.preserve_comments)
        {
            return false;
        }
        let indent_str = "    ".repeat(indent_levels as usize);
        let parts: Vec<&str> = gap_text.split('\n').collect();
        let last_is_ws_only = parts.last().is_some_and(|line| line.trim().is_empty());
        let mut emitted_newline_comment = false;
        let mut last_emitted_was_line_comment = false;

        for (i, line) in parts.iter().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let is_block = trimmed.starts_with("/*");
            let is_line = trimmed.starts_with("//");
            if (is_block && !preserve_block_comments) || (is_line && !preserve_line_comments) {
                continue;
            }

            if i == 0 && !gap_text.starts_with('\n') {
                self.write(" ");
                self.write(trimmed);
            } else {
                self.newline();
                if !indent_str.is_empty() {
                    self.write(&indent_str);
                }
                self.write(trimmed);
                emitted_newline_comment = true;
            }
            last_emitted_was_line_comment = is_line;
        }

        if parts.len() > 1
            && (last_is_ws_only || last_emitted_was_line_comment || emitted_newline_comment)
        {
            self.newline();
            if !indent_str.is_empty() {
                self.write(&indent_str);
            }
            return true;
        }

        false
    }

    fn line_fragment_has_code(text: &str) -> bool {
        let bytes = text.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i].is_ascii_whitespace() {
                i += 1;
            } else if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
                i += 2;
                while i + 1 < bytes.len() {
                    if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                        i += 2;
                        break;
                    }
                    i += 1;
                }
            } else if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                break;
            } else {
                return true;
            }
        }
        false
    }

    pub(super) fn expr_has_member_inner_comments(&self, expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Member(mem) => self.member_operator_positions(expr.span, mem).is_some_and(
                |(op_start, op_end, prop_start)| {
                    let obj_end = mem.object.span.end as usize;
                    let before = &self.source[obj_end..op_start];
                    let after = &self.source[op_end..prop_start];
                    before.contains("/*")
                        || before.contains("//")
                        || after.contains("/*")
                        || after.contains("//")
                },
            ),
            ExprKind::Paren(inner) | ExprKind::NonNull(inner) => {
                self.expr_has_member_inner_comments(inner)
            }
            ExprKind::TypeAssertion(ta) => self.expr_has_member_inner_comments(&ta.expr),
            ExprKind::As(a) => self.expr_has_member_inner_comments(&a.expr),
            ExprKind::Satisfies(s) => self.expr_has_member_inner_comments(&s.expr),
            _ => false,
        }
    }

    /// Check if an expression or any of its sub-expressions contain an
    /// element access with whitespace before `[`.  TypeScript always writes
    /// `obj[index]` without space, so source-copy would produce wrong output.
    pub(super) fn expr_has_space_before_elem_bracket(&self, expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::ElemAccess(ea) => {
                let obj_end = ea.object.span.end as usize;
                let expr_end = expr.span.end as usize;
                if obj_end < expr_end && expr_end <= self.source.len() {
                    let tail = &self.source[obj_end..expr_end];
                    if let Some(bracket_pos) = tail.find('[') {
                        tail.as_bytes()[..bracket_pos]
                            .iter()
                            .any(|&b| b == b' ' || b == b'\t')
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    // JSX emit methods are in jsx.rs module

    // ------------------------------------------------------------------
    // Downlevel emit helpers
    // ------------------------------------------------------------------

    /// Emit `a?.b` as `a === null || a === void 0 ? void 0 : a.b` for target < ES2020.
    /// For complex objects: `(_a = expr) === null || _a === void 0 ? void 0 : _a.b`
    /// In delete mode: `a === null || a === void 0 ? true : delete a.b`
    pub(super) fn emit_optional_member_downlevel(&mut self, mem: &MemberExpr, expr_span: Span) {
        let suppress = self.suppress_oc_parens;
        self.suppress_oc_parens = false;
        let delete_mode = self.oc_delete_mode;
        self.oc_delete_mode = false;
        let null_branch = if delete_mode { "true" } else { "void 0" };
        let delete_prefix = if delete_mode { "delete " } else { "" };
        let preserve_comments =
            self.options.remove_comments != Some(true) || self.preserve_comments;
        let positions = self.member_operator_positions(expr_span, mem);
        let (before_optional_text, after_optional_text, source_has_newline_before_optional) =
            if let Some((op_start, op_end, prop_start)) = positions {
                let line_start = self.source[..op_start]
                    .rfind('\n')
                    .map(|pos| pos + 1)
                    .unwrap_or(0);
                (
                    &self.source[mem.object.span.end as usize..op_start],
                    &self.source[op_end..prop_start],
                    line_start > mem.object.span.start as usize
                        && !Self::line_fragment_has_code(&self.source[line_start..op_start]),
                )
            } else {
                ("", "", false)
            };
        let has_comment_before_optional =
            before_optional_text.contains("/*") || before_optional_text.contains("//");
        let inline_before_optional_comment = has_comment_before_optional
            && !source_has_newline_before_optional
            && preserve_comments
            && !before_optional_text.trim().is_empty();
        let is_simple = matches!(&mem.object.kind, ExprKind::Ident(_) | ExprKind::This);
        if is_simple {
            if !suppress {
                self.write("(");
            }
            self.emit_expr(&mem.object);
            if inline_before_optional_comment {
                self.write(" ");
                self.write(before_optional_text.trim());
            }
            self.write(" === null || ");
            self.emit_expr(&mem.object);
            if inline_before_optional_comment {
                self.write(" ");
                self.write(before_optional_text.trim());
            }
            self.write(" === void 0 ? ");
            self.write(null_branch);
            self.write(" : ");
            self.write(delete_prefix);
            self.emit_expr(&mem.object);
            let needs_space_before_operator = if has_comment_before_optional && preserve_comments {
                if source_has_newline_before_optional {
                    self.emit_member_gap_before_operator(before_optional_text, 0)
                } else {
                    self.write(" ");
                    self.write(before_optional_text.trim());
                    false
                }
            } else {
                if source_has_newline_before_optional {
                    self.newline();
                }
                false
            };
            if mem.property != "<error>" {
                if needs_space_before_operator {
                    self.write(" ");
                }
                self.write(".");
                if preserve_comments {
                    self.emit_member_gap_after_operator(after_optional_text, 0, false, true);
                }
                self.write(&mem.property);
            }
            if !suppress {
                self.write(")");
            }
        } else {
            // Emit the inner expression first to a buffer so that inner optional
            // chains allocate their temp vars before the outer temp var, matching
            // TypeScript's bottom-up allocation order. Buffered source mappings
            // are removed from their speculative coordinates and relocated to
            // the one final insertion point below.
            let saved_len = self.output.len();
            let saved_at_line_start = self.at_line_start;
            let saved_out_line = self.out_line;
            let saved_out_col = self.out_col;
            let mapping_checkpoint = self
                .source_map_gen
                .as_ref()
                .map(SourceMapGenerator::mapping_checkpoint);
            self.at_line_start = false; // suppress auto-indent in buffer
            self.suppress_oc_parens = true;
            self.emit_expr(&mem.object);
            let inner_text = self.output[saved_len..].to_string();
            let mut inner_mappings = mapping_checkpoint
                .and_then(|checkpoint| {
                    self.source_map_gen
                        .as_mut()
                        .map(|gen| gen.take_mappings_since(checkpoint))
                })
                .unwrap_or_default();
            self.output.truncate(saved_len);
            self.at_line_start = saved_at_line_start;
            self.out_line = saved_out_line;
            self.out_col = saved_out_col;

            let tmp = self.next_temp_var();
            if !suppress {
                self.write("(");
            }
            self.write("(");
            self.write(&tmp);
            self.write(" = ");
            let inserted_line = self.out_line;
            let inserted_col = self.out_col;
            self.write(&inner_text);
            for mapping in &mut inner_mappings {
                if mapping.generated_line == saved_out_line {
                    mapping.generated_line = inserted_line;
                    mapping.generated_column = inserted_col
                        .saturating_add(mapping.generated_column.saturating_sub(saved_out_col));
                } else {
                    mapping.generated_line = inserted_line
                        .saturating_add(mapping.generated_line.saturating_sub(saved_out_line));
                }
            }
            if let Some(gen) = self.source_map_gen.as_mut() {
                for mapping in inner_mappings {
                    gen.add_mapping(mapping);
                }
            }
            self.write(") === null || ");
            self.write(&tmp);
            if inline_before_optional_comment {
                self.write(" ");
                self.write(before_optional_text.trim());
            }
            self.write(" === void 0 ? ");
            self.write(null_branch);
            self.write(" : ");
            self.write(delete_prefix);
            self.write(&tmp);
            let needs_space_before_operator = if has_comment_before_optional && preserve_comments {
                if source_has_newline_before_optional {
                    self.emit_member_gap_before_operator(before_optional_text, 0)
                } else {
                    self.write(" ");
                    self.write(before_optional_text.trim());
                    false
                }
            } else {
                if source_has_newline_before_optional {
                    self.newline();
                }
                false
            };
            if mem.property != "<error>" {
                if needs_space_before_operator {
                    self.write(" ");
                }
                self.write(".");
                if preserve_comments {
                    self.emit_member_gap_after_operator(after_optional_text, 0, false, true);
                }
                self.write(&mem.property);
            }
            if !suppress {
                self.write(")");
            }
        }
    }

    #[allow(dead_code)]
    fn emit_oc_member_chain_with_tail(&mut self, expr: &Expr) {
        // Walk down the member chain to find the optional root and collect tail properties.
        let mut tail_props: Vec<&str> = Vec::new();
        let mut current = expr;

        // The root can be an optional Member, Call, or ElemAccess.
        enum ChainRoot<'a> {
            Member(&'a MemberExpr),
            Call(&'a CallExpr),
            ElemAccess(&'a ElemAccessExpr),
        }

        let root = loop {
            match &current.kind {
                ExprKind::Member(mem) if mem.optional => break ChainRoot::Member(mem),
                ExprKind::Member(mem) => {
                    if mem.property != "<error>" {
                        tail_props.push(&mem.property);
                    }
                    current = &mem.object;
                }
                ExprKind::Call(call) if call.optional => break ChainRoot::Call(call),
                ExprKind::ElemAccess(ea) if ea.optional => break ChainRoot::ElemAccess(ea),
                // Walk through type-stripping wrappers (e.g. o2?.b!.c, o2?.b as T)
                ExprKind::NonNull(inner) => {
                    current = inner;
                }
                ExprKind::TypeAssertion(ta) => {
                    current = &ta.expr;
                }
                ExprKind::As(a) => {
                    current = &a.expr;
                }
                ExprKind::Satisfies(s) => {
                    current = &s.expr;
                }
                _ => {
                    // Fallback: should not happen if is_oc_member_chain returned true
                    self.emit_expr(expr);
                    return;
                }
            }
        };

        let suppress = self.suppress_oc_parens;
        self.suppress_oc_parens = false;
        let delete_mode = self.oc_delete_mode;
        self.oc_delete_mode = false;
        let null_branch = if delete_mode { "true" } else { "void 0" };
        let delete_prefix = if delete_mode { "delete " } else { "" };

        match root {
            ChainRoot::Member(root_mem) => {
                let root_obj = &root_mem.object;
                let is_simple = matches!(&root_obj.kind, ExprKind::Ident(_) | ExprKind::This);

                if is_simple {
                    if !suppress {
                        self.write("(");
                    }
                    self.emit_expr(root_obj);
                    self.write(" === null || ");
                    self.emit_expr(root_obj);
                    self.write(" === void 0 ? ");
                    self.write(null_branch);
                    self.write(" : ");
                    self.write(delete_prefix);
                    self.emit_expr(root_obj);
                    if root_mem.property != "<error>" {
                        self.write(".");
                        self.write(&root_mem.property);
                    }
                    for prop in tail_props.iter().rev() {
                        self.write(".");
                        self.write(prop);
                    }
                    if !suppress {
                        self.write(")");
                    }
                } else {
                    // Emit inner expression first for bottom-up temp var allocation.
                    let saved_len = self.output.len();
                    let saved_at_line_start = self.at_line_start;
                    let saved_out_line = self.out_line;
                    let saved_out_col = self.out_col;
                    self.at_line_start = false; // suppress auto-indent in buffer
                    self.suppress_oc_parens = true;
                    self.emit_expr(root_obj);
                    let inner_text = self.output[saved_len..].to_string();
                    self.output.truncate(saved_len);
                    self.at_line_start = saved_at_line_start;
                    self.out_line = saved_out_line;
                    self.out_col = saved_out_col;

                    let tmp = self.next_temp_var();
                    if !suppress {
                        self.write("(");
                    }
                    self.write("(");
                    self.write(&tmp);
                    self.write(" = ");
                    self.output.push_str(&inner_text);
                    self.write(") === null || ");
                    self.write(&tmp);
                    self.write(" === void 0 ? ");
                    self.write(null_branch);
                    self.write(" : ");
                    self.write(delete_prefix);
                    self.write(&tmp);
                    if root_mem.property != "<error>" {
                        self.write(".");
                        self.write(&root_mem.property);
                    }
                    for prop in tail_props.iter().rev() {
                        self.write(".");
                        self.write(prop);
                    }
                    if !suppress {
                        self.write(")");
                    }
                }
            }
            ChainRoot::Call(call) => {
                // `a?.b?.().c.d` → the call is the optional root, `.c.d` are the tail.
                // Emit: `(_t = callee) === null || _t === void 0 ? void 0 : _t.call(obj, args).tail`
                // Delegate to emit_optional_call_downlevel but with tail appended.
                // We need to build the call's ternary inline with tail props.
                let callee_object: Option<&Expr> = match &call.callee.kind {
                    ExprKind::Member(mem) => Some(&mem.object),
                    ExprKind::ElemAccess(ea) => Some(&ea.object),
                    _ => None,
                };
                let callee_optional = match &call.callee.kind {
                    ExprKind::Member(mem) => mem.optional,
                    ExprKind::ElemAccess(ea) => ea.optional,
                    _ => false,
                };

                if let Some(callee_obj) = callee_object {
                    let obj_is_simple =
                        matches!(&callee_obj.kind, ExprKind::Ident(_) | ExprKind::This);

                    // Emit inner callee first for bottom-up temp var allocation.
                    let saved_len = self.output.len();
                    let saved_at_line_start = self.at_line_start;
                    let saved_out_line = self.out_line;
                    let saved_out_col = self.out_col;
                    self.at_line_start = false; // suppress auto-indent in buffer
                    if callee_optional {
                        self.suppress_oc_parens = true;
                    }
                    self.emit_expr(&call.callee);
                    let inner_text = self.output[saved_len..].to_string();
                    self.output.truncate(saved_len);
                    self.at_line_start = saved_at_line_start;
                    self.out_line = saved_out_line;
                    self.out_col = saved_out_col;

                    let tmp = self.next_temp_var();
                    if !suppress {
                        self.write("(");
                    }
                    self.write("(");
                    self.write(&tmp);
                    self.write(" = ");
                    self.output.push_str(&inner_text);
                    self.write(") === null || ");
                    self.write(&tmp);
                    self.write(" === void 0 ? ");
                    self.write(null_branch);
                    self.write(" : ");
                    self.write(delete_prefix);
                    self.write(&tmp);
                    self.write(".call(");
                    // When the callee object is `super`, the `.call()` receiver
                    // should be `this` (super is used for property access, not
                    // as the call receiver).
                    if matches!(&callee_obj.kind, ExprKind::Super) {
                        self.write("this");
                    } else if obj_is_simple {
                        self.emit_expr(callee_obj);
                    } else {
                        self.emit_expr(callee_obj);
                    }
                    if !call.args.is_empty() {
                        self.write(", ");
                    }
                    self.emit_call_args_inline(&call.args);
                    self.write(")");
                    // Append tail properties
                    for prop in tail_props.iter().rev() {
                        self.write(".");
                        self.write(prop);
                    }
                    if !suppress {
                        self.write(")");
                    }
                } else {
                    // Simple callee (identifier)
                    let is_simple =
                        matches!(&call.callee.kind, ExprKind::Ident(_) | ExprKind::This);
                    if is_simple {
                        if !suppress {
                            self.write("(");
                        }
                        self.emit_expr(&call.callee);
                        self.write(" === null || ");
                        self.emit_expr(&call.callee);
                        self.write(" === void 0 ? ");
                        self.write(null_branch);
                        self.write(" : ");
                        self.write(delete_prefix);
                        self.emit_expr(&call.callee);
                        self.write("(");
                        self.emit_call_args_inline(&call.args);
                        self.write(")");
                        for prop in tail_props.iter().rev() {
                            self.write(".");
                            self.write(prop);
                        }
                        if !suppress {
                            self.write(")");
                        }
                    } else {
                        let saved_len = self.output.len();
                        let saved_at_line_start = self.at_line_start;
                        let saved_out_line = self.out_line;
                        let saved_out_col = self.out_col;
                        self.at_line_start = false; // suppress auto-indent in buffer
                        self.suppress_oc_parens = true;
                        self.emit_expr(&call.callee);
                        let inner_text = self.output[saved_len..].to_string();
                        self.output.truncate(saved_len);
                        self.at_line_start = saved_at_line_start;
                        self.out_line = saved_out_line;
                        self.out_col = saved_out_col;

                        let tmp = self.next_temp_var();
                        if !suppress {
                            self.write("(");
                        }
                        self.write("(");
                        self.write(&tmp);
                        self.write(" = ");
                        self.output.push_str(&inner_text);
                        self.write(") === null || ");
                        self.write(&tmp);
                        self.write(" === void 0 ? ");
                        self.write(null_branch);
                        self.write(" : ");
                        self.write(delete_prefix);
                        self.write(&tmp);
                        self.write("(");
                        self.emit_call_args_inline(&call.args);
                        self.write(")");
                        for prop in tail_props.iter().rev() {
                            self.write(".");
                            self.write(prop);
                        }
                        if !suppress {
                            self.write(")");
                        }
                    }
                }
            }
            ChainRoot::ElemAccess(ea) => {
                // `a?.['b'].c.d` → elem access is the optional root, `.c.d` are the tail.
                let root_obj = &ea.object;
                let is_simple = matches!(&root_obj.kind, ExprKind::Ident(_) | ExprKind::This);

                if is_simple {
                    if !suppress {
                        self.write("(");
                    }
                    self.emit_expr(root_obj);
                    self.write(" === null || ");
                    self.emit_expr(root_obj);
                    self.write(" === void 0 ? ");
                    self.write(null_branch);
                    self.write(" : ");
                    self.write(delete_prefix);
                    self.emit_expr(root_obj);
                    self.write("[");
                    self.emit_expr(&ea.index);
                    self.write("]");
                    for prop in tail_props.iter().rev() {
                        self.write(".");
                        self.write(prop);
                    }
                    if !suppress {
                        self.write(")");
                    }
                } else {
                    let saved_len = self.output.len();
                    let saved_at_line_start = self.at_line_start;
                    let saved_out_line = self.out_line;
                    let saved_out_col = self.out_col;
                    self.at_line_start = false; // suppress auto-indent in buffer
                    self.suppress_oc_parens = true;
                    self.emit_expr(root_obj);
                    let inner_text = self.output[saved_len..].to_string();
                    self.output.truncate(saved_len);
                    self.at_line_start = saved_at_line_start;
                    self.out_line = saved_out_line;
                    self.out_col = saved_out_col;

                    let tmp = self.next_temp_var();
                    if !suppress {
                        self.write("(");
                    }
                    self.write("(");
                    self.write(&tmp);
                    self.write(" = ");
                    self.output.push_str(&inner_text);
                    self.write(") === null || ");
                    self.write(&tmp);
                    self.write(" === void 0 ? ");
                    self.write(null_branch);
                    self.write(" : ");
                    self.write(delete_prefix);
                    self.write(&tmp);
                    self.write("[");
                    self.emit_expr(&ea.index);
                    self.write("]");
                    for prop in tail_props.iter().rev() {
                        self.write(".");
                        self.write(prop);
                    }
                    if !suppress {
                        self.write(")");
                    }
                }
            }
        }
    }

    /// Emit `a?.[b]` as `a === null || a === void 0 ? void 0 : a[b]` for target < ES2020.
    /// For complex objects: `(_a = expr) === null || _a === void 0 ? void 0 : _a[b]`
    /// In delete mode: `a === null || a === void 0 ? true : delete a[b]`
    pub(super) fn emit_optional_elem_access_downlevel(&mut self, ea: &ElemAccessExpr) {
        let suppress = self.suppress_oc_parens;
        self.suppress_oc_parens = false;
        let delete_mode = self.oc_delete_mode;
        self.oc_delete_mode = false;
        let null_branch = if delete_mode { "true" } else { "void 0" };
        let delete_prefix = if delete_mode { "delete " } else { "" };
        let is_simple = matches!(&ea.object.kind, ExprKind::Ident(_) | ExprKind::This);
        if is_simple {
            if !suppress {
                self.write("(");
            }
            self.emit_expr(&ea.object);
            self.write(" === null || ");
            self.emit_expr(&ea.object);
            self.write(" === void 0 ? ");
            self.write(null_branch);
            self.write(" : ");
            self.write(delete_prefix);
            self.emit_expr(&ea.object);
            self.write("[");
            self.emit_expr(&ea.index);
            self.write("]");
            if !suppress {
                self.write(")");
            }
        } else {
            // Emit inner expression first for bottom-up temp var allocation.
            let saved_len = self.output.len();
            let saved_at_line_start = self.at_line_start;
            let saved_out_line = self.out_line;
            let saved_out_col = self.out_col;
            self.at_line_start = false; // suppress auto-indent in buffer
            self.suppress_oc_parens = true;
            self.emit_expr(&ea.object);
            let inner_text = self.output[saved_len..].to_string();
            self.output.truncate(saved_len);
            self.at_line_start = saved_at_line_start;
            self.out_line = saved_out_line;
            self.out_col = saved_out_col;

            let tmp = self.next_temp_var();
            if !suppress {
                self.write("(");
            }
            self.write("(");
            self.write(&tmp);
            self.write(" = ");
            self.output.push_str(&inner_text);
            self.write(") === null || ");
            self.write(&tmp);
            self.write(" === void 0 ? ");
            self.write(null_branch);
            self.write(" : ");
            self.write(delete_prefix);
            self.write(&tmp);
            self.write("[");
            self.emit_expr(&ea.index);
            self.write("]");
            if !suppress {
                self.write(")");
            }
        }
    }

    pub(super) fn new_expr_has_empty_array_call_callee_recovery_shape(
        &self,
        new_expr: &NewExpr,
    ) -> bool {
        let ExprKind::ElemAccess(ea) = &new_expr.callee.kind else {
            return false;
        };
        let callee_start = new_expr.callee.span.start as usize;
        let callee_end = new_expr.callee.span.end as usize;
        let has_brackets = callee_start < callee_end && callee_end <= self.source.len() && {
            let callee_src = &self.source[callee_start..callee_end];
            callee_src.contains('[') && callee_src.contains(']')
        };
        if !has_brackets {
            return false;
        }
        match (&ea.index.kind, &new_expr.args) {
            (ExprKind::Call(call), None) => {
                matches!(&call.callee.kind, ExprKind::Ident(name) if name == "<error>")
                    && call.args.is_empty()
                    && {
                        let callee_src = &self.source[callee_start..callee_end];
                        callee_src.contains('(')
                    }
            }
            (ExprKind::Omitted, Some(args)) => args.is_empty(),
            _ => false,
        }
    }

    fn emit_recovery_new_empty_array_call_callee(&mut self, new_expr: &NewExpr) -> bool {
        if !self.new_expr_has_empty_array_call_callee_recovery_shape(new_expr) {
            return false;
        }
        let ExprKind::ElemAccess(ea) = &new_expr.callee.kind else {
            return false;
        };
        self.write("new ");
        self.emit_expr(&ea.object);
        self.write("[]()");
        true
    }

    #[allow(dead_code)]
    fn emit_optional_member_call_inline(&mut self, call: &CallExpr) {
        let mem = match &call.callee.kind {
            ExprKind::Member(m) => m,
            _ => unreachable!(),
        };
        let is_simple = matches!(&mem.object.kind, ExprKind::Ident(_) | ExprKind::This);
        if is_simple {
            self.emit_expr(&mem.object);
            self.write(" === null || ");
            self.emit_expr(&mem.object);
            self.write(" === void 0 ? void 0 : ");
            self.emit_expr(&mem.object);
            if mem.property != "<error>" {
                self.write(".");
                self.write(&mem.property);
            }
            self.write("(");
            self.emit_call_args_inline(&call.args);
            self.write(")");
        } else {
            // Emit inner expression first for bottom-up temp var allocation.
            let saved_len = self.output.len();
            let saved_at_line_start = self.at_line_start;
            let saved_out_line = self.out_line;
            let saved_out_col = self.out_col;
            self.at_line_start = false; // suppress auto-indent in buffer
            self.suppress_oc_parens = true;
            self.emit_expr(&mem.object);
            let inner_text = self.output[saved_len..].to_string();
            self.output.truncate(saved_len);
            self.at_line_start = saved_at_line_start;
            self.out_line = saved_out_line;
            self.out_col = saved_out_col;

            let tmp = self.next_temp_var();
            self.write("(");
            self.write(&tmp);
            self.write(" = ");
            self.output.push_str(&inner_text);
            self.write(") === null || ");
            self.write(&tmp);
            self.write(" === void 0 ? void 0 : ");
            self.write(&tmp);
            if mem.property != "<error>" {
                self.write(".");
                self.write(&mem.property);
            }
            self.write("(");
            self.emit_call_args_inline(&call.args);
            self.write(")");
        }
    }

    /// Check if a call expression has a block comment (e.g. `/** @this */`)
    /// between the opening `(` and the first argument in the source text.
    /// Returns the comment text if found.
    fn call_has_leading_arg_block_comment(
        &self,
        call: &CallExpr,
        _expr_span: Span,
    ) -> Option<String> {
        if call.args.is_empty() {
            return None;
        }
        if self.options.remove_comments == Some(true) {
            return None;
        }
        let first_arg = &call.args[0];
        let arg_start = first_arg.span.start as usize;
        // Find the `(` before the first arg by scanning backwards from arg_start
        let callee_end = call.callee.span.end as usize;
        if callee_end >= arg_start || arg_start > self.source.len() {
            return None;
        }
        let between = &self.source[callee_end..arg_start];
        // Look for a block comment between `(` and the first arg
        if let Some(bc_start) = between.find("/*") {
            if let Some(bc_end) = between[bc_start..].find("*/") {
                let comment = between[bc_start..bc_start + bc_end + 2].trim();
                if !comment.is_empty() {
                    return Some(comment.to_string());
                }
            }
        }
        None
    }

    fn emit_call_args_inline(&mut self, args: &[Box<Expr>]) {
        let mut first = true;
        for arg in args.iter() {
            // Skip error-placeholder arguments (e.g. bare `\` from parser recovery)
            if expr_is_error_placeholder(arg) {
                continue;
            }
            if !first {
                self.write(", ");
            }
            first = false;
            // Inside call arguments, suppress outer parens on nullish
            // coalescing lowering — the call parens already provide grouping.
            self.suppress_oc_parens = true;
            self.emit_expr(arg);
        }
    }

    pub(super) fn expr_has_missing_arg_commas(&self, expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Call(call) => self.args_have_missing_commas(&call.args),
            ExprKind::New(new_expr) => new_expr
                .args
                .as_ref()
                .is_some_and(|args| self.args_have_missing_commas(args)),
            _ => false,
        }
    }

    fn args_have_missing_commas(&self, args: &[Box<Expr>]) -> bool {
        let mut prev_end: Option<usize> = None;
        for arg in args {
            if matches!(arg.kind, ExprKind::Omitted) {
                continue;
            }
            let start = arg.span.start as usize;
            let end = arg.span.end as usize;
            if let Some(prev_end) = prev_end {
                if prev_end > start || start > self.source.len() {
                    return true;
                }
                if !self.source[prev_end..start].contains(',') {
                    return true;
                }
            }
            prev_end = Some(end);
        }
        false
    }

    /// Check if an expression tree contains an ArrayLit where adjacent
    /// elements have no comma between them in the source (parser error recovery
    /// implied comma). Forces structured emit to insert the comma.
    pub(super) fn expr_has_array_lit_missing_commas(&self, expr: &Expr) -> bool {
        if let ExprKind::ArrayLit(elements) = &expr.kind {
            let mut prev_end: Option<usize> = None;
            for elem in elements.iter().flatten() {
                let start = elem.span.start as usize;
                let end = elem.span.end as usize;
                if let Some(pe) = prev_end {
                    if pe <= start && start <= self.source.len() {
                        if !self.source[pe..start].contains(',') {
                            return true;
                        }
                    }
                }
                prev_end = Some(end);
            }
        }
        false
    }

    /// Check if an object literal has shorthand properties with `?` (optional
    /// marker) or reserved keyword names that need special emit.
    pub(super) fn expr_has_optional_shorthand(&self, expr: &Expr) -> bool {
        if let ExprKind::ObjectLit(props) = &expr.kind {
            for prop in props {
                if let ObjLitProp::Shorthand(name, _) = prop {
                    if crate::emit_stmt::is_js_reserved_keyword(name) {
                        return true;
                    }
                }
                if let ObjLitProp::Shorthand(name, span) = prop {
                    let s = span.start as usize;
                    let e = span.end as usize;
                    if s < e && e <= self.source.len() {
                        let text = &self.source[s..e];
                        if text.contains('?') {
                            return true;
                        }
                        // Also check just after the span end for `?`
                        let after = e;
                        if after < self.source.len() {
                            let rest = &self.source[after..];
                            let trimmed = rest.trim_start();
                            if trimmed.starts_with('?') {
                                return true;
                            }
                        }
                    }
                    // Also check between name end and next significant char
                    let name_end = span.start as usize + name.len();
                    if name_end < self.source.len() {
                        let rest = &self.source[name_end..e.min(self.source.len())];
                        if rest.contains('?') {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    /// Check if an expression tree contains an ObjectLit whose consecutive
    /// properties have a missing comma in the source (parser error recovery).
    pub(super) fn expr_has_obj_lit_missing_commas(&self, expr: &Expr) -> bool {
        let mut stack = vec![expr];
        while let Some(e) = stack.pop() {
            if let ExprKind::ObjectLit(props) = &e.kind {
                if self.obj_lit_props_missing_comma(props) {
                    return true;
                }
            }
            // Recurse into sub-expressions
            match &e.kind {
                ExprKind::Assign(a) => {
                    stack.push(&a.left);
                    stack.push(&a.right);
                }
                ExprKind::Paren(inner) => stack.push(inner),
                ExprKind::Binary(b) => {
                    stack.push(&b.left);
                    stack.push(&b.right);
                }
                ExprKind::Cond(cond) => {
                    stack.push(&cond.test);
                    stack.push(&cond.consequent);
                    stack.push(&cond.alternate);
                }
                ExprKind::Comma(exprs) => {
                    for child in exprs {
                        stack.push(child);
                    }
                }
                ExprKind::ArrayLit(exprs) => {
                    for child in exprs.iter().flatten() {
                        stack.push(child);
                    }
                }
                ExprKind::Call(call) => {
                    stack.push(&call.callee);
                    for arg in &call.args {
                        stack.push(arg);
                    }
                }
                ExprKind::ObjectLit(props) => {
                    for prop in props {
                        match prop {
                            ObjLitProp::Property(p) => stack.push(&p.value),
                            ObjLitProp::Spread(inner, _) => stack.push(inner),
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        false
    }

    fn obj_lit_props_missing_comma(&self, props: &[ObjLitProp]) -> bool {
        if props.len() < 2 {
            return false;
        }
        for i in 0..props.len() - 1 {
            let end = obj_lit_prop_span_end(&props[i]) as usize;
            let next_start = match &props[i + 1] {
                ObjLitProp::Property(p) => p.span.start as usize,
                ObjLitProp::Shorthand(_, span) => span.start as usize,
                ObjLitProp::ShorthandDefault(_, _, span) => span.start as usize,
                ObjLitProp::Spread(_, span) => span.start as usize,
                ObjLitProp::Method(m) => m.span.start as usize,
                ObjLitProp::Get(a) => a.span.start as usize,
                ObjLitProp::Set(a) => a.span.start as usize,
            };
            if end <= next_start && next_start <= self.source.len() {
                let between = &self.source[end..next_start];
                if !between.contains(',') && !between.contains(';') {
                    return true;
                }
            }
        }
        false
    }

    /// Emit a non-optional call whose callee is part of an OC chain.
    /// The call `(args)` is placed inside the ternary's truthy branch by
    /// emitting the callee with suppress_oc_parens=true (producing an unwrapped
    /// ternary) and appending the call args — JS precedence ensures the call
    /// is part of the ternary's alt expression.
    ///
    /// `o?.b(args)` → `o === null || o === void 0 ? void 0 : o.b(args)`
    /// `o?.["b"](args)` → `o === null || o === void 0 ? void 0 : o["b"](args)`
    /// `o?.a.b(args)` → `o === null || o === void 0 ? void 0 : o.a.b(args)`
    fn emit_oc_call_inline(&mut self, call: &CallExpr) {
        let suppress = self.suppress_oc_parens;
        self.suppress_oc_parens = false;

        // When a parenthesized OC chain ending in a member access is called,
        // use `.call(receiver, args)` to preserve `this` binding.
        // E.g.: `(o2?.b)(args)` → `(o2 === null || ... : o2.b).call(o2, args)`
        // E.g.: `(o4?.b().c)(args)` → `(... : (_a = o4.b()).c).call(_a, args)`
        if let ExprKind::Paren(inner) = &call.callee.kind {
            if let Some(receiver_info) = self.oc_chain_last_member_receiver(inner) {
                if !suppress {
                    self.write("(");
                }
                self.suppress_oc_parens = true;
                match receiver_info {
                    OcCallReceiver::Simple(ref receiver_expr) => {
                        self.emit_expr(&call.callee);
                        self.write(".call(");
                        self.emit_expr(receiver_expr);
                    }
                    OcCallReceiver::NeedsTemp => {
                        // For complex receivers like `o4?.b().c`, we need a temp var.
                        // Emit the callee first, then post-process to wrap the call
                        // inside the ternary's truthy branch:
                        // `o4 === null || ... : o4.b().c`
                        // → `o4 === null || ... : (_a = o4.b()).c`
                        let temp = self.next_temp_var();
                        let before_pos = self.output.len();
                        self.emit_expr(&call.callee);
                        // Find the truthy branch's call parens in the emitted output.
                        // Look for the last `()` followed by `).prop` pattern.
                        let emitted = &self.output[before_pos..].to_string();
                        if let Some(colon_pos) = emitted.rfind("void 0 : ") {
                            let branch_start = before_pos + colon_pos + "void 0 : ".len();
                            // Find the call `()` in the truthy branch
                            let branch = &self.output[branch_start..].to_string();
                            if let Some(call_end) = branch.find(").") {
                                let insert_open = branch_start;
                                let insert_close = branch_start + call_end + 1;
                                let assign = format!("({} = ", temp);
                                self.output.insert_str(insert_open, &assign);
                                self.output.insert(insert_close + assign.len(), ')');
                            }
                        }
                        self.write(".call(");
                        self.write(&temp);
                    }
                }
                if !call.args.is_empty() {
                    self.write(", ");
                    self.emit_call_args_inline(&call.args);
                }
                self.write(")");
                if !suppress {
                    self.write(")");
                }
                return;
            }
        }

        if !suppress {
            self.write("(");
        }
        // Emit the callee with suppress so the OC ternary has no outer parens
        self.suppress_oc_parens = true;
        self.emit_expr(&call.callee);
        // Append call args — JS precedence places this in the ternary's alt branch
        self.write("(");
        self.emit_call_args_inline(&call.args);
        self.write(")");
        if !suppress {
            self.write(")");
        }
    }

    /// Check if an OC chain ends with a member access and return info about
    /// the receiver for `.call()` transformation.
    fn oc_chain_last_member_receiver(&self, expr: &Expr) -> Option<OcCallReceiver> {
        match &expr.kind {
            ExprKind::Member(mem) if mem.optional => {
                // `o?.b` — receiver is `o` (simple)
                Some(OcCallReceiver::Simple((*mem.object).clone()))
            }
            ExprKind::Member(mem) => {
                // `chain.b` where `chain` is an OC chain — receiver is `chain`
                // If `chain` is a call (like `o?.b().c`), we need a temp var
                if is_oc_chain(&mem.object) {
                    match &mem.object.kind {
                        ExprKind::Call(_) => Some(OcCallReceiver::NeedsTemp),
                        _ => Some(OcCallReceiver::Simple((*mem.object).clone())),
                    }
                } else {
                    None
                }
            }
            ExprKind::Call(_) => {
                // Chain ends with a call, not a member access — no `.call()` needed
                None
            }
            ExprKind::Paren(inner) => self.oc_chain_last_member_receiver(inner),
            _ => None,
        }
    }

    /// Emit a non-optional element access whose object is part of an OC chain.
    /// The `[index]` is placed inside the ternary's truthy branch.
    ///
    /// `o?.b["c"]` → `o === null || o === void 0 ? void 0 : o.b["c"]`
    /// `o?.b()["c"]` → `(_a = o.b) === null || _a === void 0 ? void 0 : _a.call(o)["c"]`
    fn emit_oc_elem_access_inline(&mut self, ea: &ElemAccessExpr, _expr: &Expr) {
        let suppress = self.suppress_oc_parens;
        self.suppress_oc_parens = false;

        if !suppress {
            self.write("(");
        }
        // Emit the object with suppress so the OC ternary has no outer parens
        self.suppress_oc_parens = true;
        self.emit_expr(&ea.object);
        // Append [index] — JS precedence places this in the ternary's alt branch
        self.write("[");
        self.emit_expr(&ea.index);
        self.write("]");
        if !suppress {
            self.write(")");
        }
    }

    /// Emit `a?.b()` as `a === null || a === void 0 ? void 0 : a.b()` for target < ES2020.
    /// When the callee is a member expression, uses `.call(obj, args)` to preserve `this`.
    /// In delete mode: uses `true` for null branch and `delete` prefix for non-null branch.
    pub(super) fn emit_optional_call_downlevel(&mut self, call: &CallExpr) {
        let suppress = self.suppress_oc_parens;
        self.suppress_oc_parens = false;
        let delete_mode = self.oc_delete_mode;
        self.oc_delete_mode = false;
        let null_branch = if delete_mode { "true" } else { "void 0" };
        let delete_prefix = if delete_mode { "delete " } else { "" };

        // When the callee is a member or element access expression
        // (e.g. `o3.b?.()`, `a?.m?.()`, `obj[key]?.()`, `(foo.m as any)?.()`),
        // we need `.call(obj, args)` to preserve the `this` context.
        // Unwrap type-stripping wrappers to find the underlying member/elem access.
        let mut unwrapped_callee: &Expr = &call.callee;
        loop {
            match &unwrapped_callee.kind {
                ExprKind::NonNull(inner) | ExprKind::Paren(inner) => unwrapped_callee = inner,
                ExprKind::TypeAssertion(ta) => unwrapped_callee = &ta.expr,
                ExprKind::As(a) => unwrapped_callee = &a.expr,
                ExprKind::Satisfies(s) => unwrapped_callee = &s.expr,
                _ => break,
            }
        }
        let callee_object: Option<&Expr> = match &unwrapped_callee.kind {
            ExprKind::Member(mem) => Some(&mem.object),
            ExprKind::ElemAccess(ea) => Some(&ea.object),
            _ => None,
        };
        let callee_optional = match &unwrapped_callee.kind {
            ExprKind::Member(mem) => mem.optional,
            ExprKind::ElemAccess(ea) => ea.optional,
            _ => false,
        };
        if let Some(callee_obj) = callee_object {
            let obj_is_simple = matches!(&callee_obj.kind, ExprKind::Ident(_) | ExprKind::This);

            // Emit inner callee first for bottom-up temp var allocation.
            let saved_len = self.output.len();
            let saved_at_line_start = self.at_line_start;
            let saved_out_line = self.out_line;
            let saved_out_col = self.out_col;
            self.at_line_start = false; // suppress auto-indent in buffer
            if callee_optional {
                self.suppress_oc_parens = true;
            }
            self.emit_expr(&call.callee);
            let mut inner_text = self.output[saved_len..].to_string();
            self.output.truncate(saved_len);
            self.at_line_start = saved_at_line_start;
            self.out_line = saved_out_line;
            self.out_col = saved_out_col;

            // Preserve block comments lost during type-assertion stripping.
            // E.g. `(/*a*/foo.m as any/*b*/)/*c*/?.()` → the callee emission
            // strips `as any` and the Paren, losing /*b*/ and /*c*/.
            let unwrapped_end = unwrapped_callee.span.end as usize;
            let callee_end = call.callee.span.end as usize;
            if unwrapped_end < callee_end && unwrapped_end < self.source.len() {
                let scan_end = std::cmp::min(callee_end + 50, self.source.len());
                let scan = &self.source[unwrapped_end..scan_end];
                let mut pos = 0;
                while pos < scan.len() {
                    let b = scan.as_bytes()[pos];
                    if b == b'/' && scan[pos..].starts_with("/*") {
                        if let Some(end) = scan[pos + 2..].find("*/") {
                            let comment = &scan[pos..pos + 2 + end + 2];
                            inner_text.push(' ');
                            inner_text.push_str(comment);
                            pos = pos + 2 + end + 2;
                        } else {
                            break;
                        }
                    } else if b == b'?' || b == b'(' {
                        break; // reached ?. or call parens
                    } else {
                        pos += 1;
                    }
                }
            }

            let tmp = self.next_temp_var();
            if !suppress {
                self.write("(");
            }
            self.write("(");
            self.write(&tmp);
            self.write(" = ");
            self.output.push_str(&inner_text);
            self.write(") === null || ");
            self.write(&tmp);
            self.write(" === void 0 ? ");
            self.write(null_branch);
            self.write(" : ");
            self.write(delete_prefix);
            self.write(&tmp);
            self.write(".call(");
            // When the callee object is `super`, the `.call()` receiver
            // should be `this` (super is used for property access, not
            // as the call receiver).
            if matches!(&callee_obj.kind, ExprKind::Super) {
                self.write("this");
            } else if obj_is_simple {
                self.emit_expr(callee_obj);
            } else {
                self.emit_expr(callee_obj);
            }
            if !call.args.is_empty() {
                self.write(", ");
            }
            self.emit_call_args_inline(&call.args);
            self.write(")");
            if !suppress {
                self.write(")");
            }
            return;
        }

        let is_simple = matches!(&call.callee.kind, ExprKind::Ident(_) | ExprKind::This);
        if is_simple {
            if !suppress {
                self.write("(");
            }
            self.emit_expr(&call.callee);
            self.write(" === null || ");
            self.emit_expr(&call.callee);
            self.write(" === void 0 ? ");
            self.write(null_branch);
            self.write(" : ");
            self.write(delete_prefix);
            self.emit_expr(&call.callee);
            self.write("(");
            self.emit_call_args_inline(&call.args);
            self.write(")");
            if !suppress {
                self.write(")");
            }
        } else {
            // Emit inner expression first for bottom-up temp var allocation.
            let saved_len = self.output.len();
            let saved_at_line_start = self.at_line_start;
            let saved_out_line = self.out_line;
            let saved_out_col = self.out_col;
            self.at_line_start = false; // suppress auto-indent in buffer
            self.suppress_oc_parens = true;
            self.emit_expr(&call.callee);
            let inner_text = self.output[saved_len..].to_string();
            self.output.truncate(saved_len);
            self.at_line_start = saved_at_line_start;
            self.out_line = saved_out_line;
            self.out_col = saved_out_col;

            let tmp = self.next_temp_var();
            if !suppress {
                self.write("(");
            }
            self.write("(");
            self.write(&tmp);
            self.write(" = ");
            self.output.push_str(&inner_text);
            self.write(") === null || ");
            self.write(&tmp);
            self.write(" === void 0 ? ");
            self.write(null_branch);
            self.write(" : ");
            self.write(delete_prefix);
            self.write(&tmp);
            self.write("(");
            self.emit_call_args_inline(&call.args);
            self.write(")");
            if !suppress {
                self.write(")");
            }
        }
    }

    /// Emit `a ?? b` for target < ES2020.
    /// Simple identifiers: `a !== null && a !== void 0 ? a : b`
    /// Complex expressions: `((_a = expr) !== null && _a !== void 0 ? _a : b)`
    pub(super) fn emit_nullish_coalescing_downlevel(&mut self, bin: &BinaryExpr) {
        let is_simple = matches!(
            &bin.left.kind,
            ExprKind::Ident(_)
                | ExprKind::This
                | ExprKind::NullLit
                | ExprKind::NumLit(_)
                | ExprKind::StrLit(_)
                | ExprKind::BoolLit(_)
                | ExprKind::NoSubstTemplate(_)
        );
        if is_simple {
            // Simple identifier: no temp var needed, no outer parens in safe context
            let suppress = self.suppress_oc_parens;
            self.suppress_oc_parens = false;
            if !suppress {
                self.write("(");
            }
            self.emit_expr(&bin.left);
            self.write(" !== null && ");
            self.emit_expr(&bin.left);
            self.write(" !== void 0 ? ");
            self.emit_expr(&bin.left);
            self.write(" : ");
            self.suppress_oc_parens = true;
            self.emit_expr(&bin.right);
            if !suppress {
                self.write(")");
            }
        } else {
            // Complex expression: use temp var to avoid evaluating twice.
            // Emit the left side first so inner ?? temp vars are allocated before
            // the outer temp var, matching TypeScript's allocation order.
            let suppress = self.suppress_oc_parens;
            self.suppress_oc_parens = false;
            let left_start = self.output.len();
            self.suppress_oc_parens = true;
            self.emit_expr(&bin.left);
            let left_output = self.output[left_start..].to_string();
            self.output.truncate(left_start);
            // If left output is a multi-line parenthesized expression (e.g. from
            // source-copy of `(\n  expr &&\n  expr\n)`), collapse the leading
            // `(\n<indent>` to `(` and trailing `\n<indent>)` to `)` so the
            // content flows inline after `_a = (`.
            let left_output = {
                let trimmed = left_output.trim_start();
                if (trimmed.starts_with("(\n") || trimmed.starts_with("(\r\n"))
                    && trimmed.ends_with(')')
                {
                    let after_open = &trimmed[1..]; // skip `(`
                    let inner = after_open.trim_start();
                    // Remove trailing `)` and any whitespace before it
                    if let Some(last_close) = inner.rfind(')') {
                        let before_close = inner[..last_close].trim_end();
                        format!("({})", before_close)
                    } else {
                        left_output
                    }
                } else {
                    left_output
                }
            };
            let tmp = self.next_temp_var();
            self.suppress_oc_parens = false;
            if !suppress {
                self.write("(");
            }
            self.write("(");
            self.write(&tmp);
            self.write(" = ");
            self.write(&left_output);
            self.write(") !== null && ");
            self.write(&tmp);
            self.write(" !== void 0 ? ");
            self.write(&tmp);
            self.write(" : ");
            self.suppress_oc_parens = true;
            self.emit_expr(&bin.right);
            if !suppress {
                self.write(")");
            }
        }
    }

    /// Emit compound exponentiation assignment (`**=`) downlevel.
    ///
    /// For member expressions, caches the object (and key for element access) in
    /// temp vars to avoid evaluating side-effectful expressions twice:
    ///   `obj.prop **= val`   →  `(_a = obj).prop = Math.pow(_a.prop, val)`
    ///   `obj[key] **= val`   →  `(_a = obj)[_b = key] = Math.pow(_a[_b], val)`
    ///   `x **= val`          →  `x = Math.pow(x, val)`
    ///
    /// For nested `**=`, inner temps are allocated first (matching TypeScript)
    /// by emitting the RHS to a buffer before allocating the outer temp.
    pub(super) fn emit_exp_assign_downlevel(&mut self, assign: &AssignExpr) {
        match &assign.left.kind {
            ExprKind::Member(mem) => {
                // Emit RHS first so nested **= temps get lower-numbered names
                let rhs_start = self.output.len();
                self.emit_expr(&assign.right);
                let rhs_buf = self.output[rhs_start..].to_string();
                self.output.truncate(rhs_start);

                let obj_temp = self.next_temp_var();
                self.write("(");
                self.write(&obj_temp);
                self.write(" = ");
                self.emit_expr(&mem.object);
                self.write(").");
                self.write(&mem.property);
                self.write(" = Math.pow(");
                self.write(&obj_temp);
                self.write(".");
                self.write(&mem.property);
                self.write(", ");
                self.write(&rhs_buf);
                self.write(")");
            }
            ExprKind::ElemAccess(ea) => {
                // Emit RHS first so nested **= temps get lower-numbered names
                let rhs_start = self.output.len();
                self.emit_expr(&assign.right);
                let rhs_buf = self.output[rhs_start..].to_string();
                self.output.truncate(rhs_start);

                let obj_temp = self.next_temp_var();
                let key_temp = self.next_temp_var();
                self.write("(");
                self.write(&obj_temp);
                self.write(" = ");
                self.emit_expr(&ea.object);
                self.write(")[");
                self.write(&key_temp);
                self.write(" = ");
                self.emit_expr(&ea.index);
                self.write("] = Math.pow(");
                self.write(&obj_temp);
                self.write("[");
                self.write(&key_temp);
                self.write("], ");
                self.write(&rhs_buf);
                self.write(")");
            }
            ExprKind::Super => {
                // `super **= val` → `(_a = super). = Math.pow(_a., val)`
                // TypeScript caches super in a temp and uses empty-property member access.
                // Suppress the bare-super post-processor so we can place dots precisely.
                self.suppress_bare_super_fixup = true;
                let rhs_start = self.output.len();
                let saved_at_line_start = self.at_line_start;
                self.at_line_start = false;
                self.emit_expr(&assign.right);
                let rhs_buf = self.output[rhs_start..].to_string();
                self.output.truncate(rhs_start);
                self.at_line_start = saved_at_line_start;

                let obj_temp = self.next_temp_var();
                self.write("(");
                self.write(&obj_temp);
                self.write(" = super). = Math.pow(");
                self.write(&obj_temp);
                self.write("., ");
                self.write(&rhs_buf);
                self.write(")");
            }
            _ => {
                // Simple identifier or parenthesized: x = Math.pow(x, y)
                self.emit_expr(&assign.left);
                self.write(" = Math.pow(");
                self.emit_expr(&assign.left);
                self.write(", ");
                self.emit_expr(&assign.right);
                self.write(")");
            }
        }
    }

    /// Emit logical assignment downlevel.
    /// `x ??= y` -> `x !== null && x !== void 0 ? x : (x = y)`
    /// `x &&= y` -> `x && (x = y)`
    /// `x ||= y` -> `x || (x = y)`
    pub(super) fn emit_logical_assign_downlevel(&mut self, assign: &AssignExpr) {
        let stable_lhs = self.stable_logical_assign_lhs(&assign.left);
        let lhs = stable_lhs.unwrap_or(&assign.left);
        let rhs = self.logical_assign_rhs(&assign.right);

        match assign.op {
            AssignOp::NullCoalAssign => {
                // ES2020 can preserve nullish coalescing, but not logical
                // assignment. For stable targets, TypeScript consequently
                // emits the smaller `lhs ?? (lhs = rhs)` form. Restrict this
                // path to identifiers and direct properties of identifiers:
                // more complex receivers require the full capture transform.
                if self.effective_target() == ScriptTarget::ES2020
                    && assign.right.span.start < assign.right.span.end
                    && !matches!(&assign.right.kind, ExprKind::Omitted)
                    && !matches!(
                        &assign.right.kind,
                        ExprKind::Ident(name) if name.is_empty() || name == "<error>"
                    )
                    && !self.has_comments_in_range(assign.left.span.start, assign.right.span.end)
                {
                    if stable_lhs.is_some() {
                        self.emit_expr(lhs);
                        self.write(" ?? (");
                        self.emit_expr(lhs);
                        self.write(" = ");
                        self.emit_expr(rhs);
                        self.write(")");
                        return;
                    }
                }

                // For property accesses (x.a ??= y), use a temp var to avoid
                // evaluating the LHS multiple times:
                //   var _a; (_a = x.a) !== null && _a !== void 0 ? _a : (x.a = y)
                let needs_temp = !matches!(lhs.kind, ExprKind::Ident(_));
                if needs_temp {
                    let temp = self.next_temp_var();
                    self.write("(");
                    self.write(&temp);
                    self.write(" = ");
                    self.emit_expr(lhs);
                    self.write(") !== null && ");
                    self.write(&temp);
                    self.write(" !== void 0 ? ");
                    self.write(&temp);
                    self.write(" : (");
                    self.emit_expr(lhs);
                    self.write(" = ");
                    self.emit_expr(rhs);
                    self.write(")");
                } else {
                    self.emit_expr(lhs);
                    self.write(" !== null && ");
                    self.emit_expr(lhs);
                    self.write(" !== void 0 ? ");
                    self.emit_expr(lhs);
                    self.write(" : (");
                    self.emit_expr(lhs);
                    self.write(" = ");
                    self.emit_expr(rhs);
                    self.write(")");
                }
            }
            AssignOp::LogAndAssign => {
                self.emit_expr(lhs);
                self.write(" && (");
                self.emit_expr(lhs);
                self.write(" = ");
                self.emit_expr(rhs);
                self.write(")");
            }
            AssignOp::LogOrAssign => {
                self.emit_expr(lhs);
                self.write(" || (");
                self.emit_expr(lhs);
                self.write(" = ");
                self.emit_expr(rhs);
                self.write(")");
            }
            _ => {
                // Fallback: emit normally
                self.emit_expr(&assign.left);
                self.write(" ");
                self.write(assign_op_str(assign.op));
                self.write(" ");
                self.emit_expr(&assign.right);
            }
        }
    }

    /// Return a stable logical-assignment LHS whose source parentheses can be
    /// discarded without receiver/key capture. Parentheses are safe to discard
    /// only when they do not own comments; all type-erasing and recovery-shaped
    /// wrappers fail closed rather than risking evaluation or comment drift.
    fn stable_logical_assign_lhs<'expr>(&self, lhs: &'expr Expr) -> Option<&'expr Expr> {
        if lhs.span.start >= lhs.span.end
            || self.has_comments_in_range(lhs.span.start, lhs.span.end)
        {
            return None;
        }

        let mut stripped = lhs;
        while let ExprKind::Paren(inner) = &stripped.kind {
            if inner.span.start >= inner.span.end
                || inner.span.start < stripped.span.start
                || inner.span.end > stripped.span.end
            {
                return None;
            }
            stripped = inner;
        }

        match &stripped.kind {
            ExprKind::Ident(name)
                if !name.is_empty() && name != "<error>" && !name.starts_with('#') =>
            {
                Some(stripped)
            }
            ExprKind::Member(member)
                if !member.optional
                    && !member.property.is_empty()
                    && member.property != "<error>"
                    && !member.property.starts_with('#')
                    && matches!(
                        &member.object.kind,
                        ExprKind::Ident(name)
                            if !name.is_empty()
                                && name != "<error>"
                                && !name.starts_with('#')
                    ) =>
            {
                Some(stripped)
            }
            _ => None,
        }
    }

    /// Generated logical assignments do not retain source parentheses around
    /// a single arrow RHS or a nested logical assignment with a stable target.
    /// Strip exactly one validated, comment-free wrapper: comma expressions,
    /// type wrappers, recovery shapes, and unstable nested targets keep their
    /// original grouping.
    fn logical_assign_rhs<'expr>(&self, rhs: &'expr Expr) -> &'expr Expr {
        let ExprKind::Paren(inner) = &rhs.kind else {
            return rhs;
        };
        if rhs.span.start >= rhs.span.end
            || inner.span.start >= inner.span.end
            || inner.span.start < rhs.span.start
            || inner.span.end > rhs.span.end
            || self.has_comments_in_range(rhs.span.start, rhs.span.end)
        {
            return rhs;
        }
        if matches!(&inner.kind, ExprKind::Arrow(_)) {
            return inner;
        }
        let ExprKind::Assign(nested) = &inner.kind else {
            return rhs;
        };
        if matches!(
            nested.op,
            AssignOp::LogAndAssign | AssignOp::LogOrAssign | AssignOp::NullCoalAssign
        ) && self.stable_logical_assign_lhs(&nested.left).is_some()
        {
            inner
        } else {
            rhs
        }
    }

    /// Emit `for (const x of arr)` as an index-based for loop for ES5 targets.
    ///
    /// Without `downlevelIteration`:
    /// ```js
    /// for (var _i = 0, arr_1 = arr; _i < arr_1.length; _i++) {
    ///     var x = arr_1[_i];
    ///     // body
    /// }
    /// ```
    ///
    /// With `downlevelIteration`:
    /// ```js
    /// var e_1, _a;
    /// try {
    ///     for (var arr_1 = __values(arr), arr_1_1 = arr_1.next(); !arr_1_1.done; arr_1_1 = arr_1.next()) {
    ///         var x = arr_1_1.value;
    ///         // body
    ///     }
    /// } catch (e_1_1) { e_1 = { error: e_1_1 }; }
    /// finally {
    ///     try { if (arr_1_1 && !arr_1_1.done && (_a = arr_1.return)) _a.call(arr_1); }
    ///     finally { if (e_1) throw e_1.error; }
    /// }
    /// ```
    pub(super) fn emit_for_of_downlevel(&mut self, fo: &ForOfStmt, span: Span) {
        self.emit_for_of_downlevel_core(fo, span, &[]);
    }

    pub(super) fn emit_for_of_using_downlevel(
        &mut self,
        fo: &ForOfStmt,
        span: Span,
        labels: &[&str],
    ) {
        self.emit_for_of_downlevel_core(fo, span, labels);
    }

    pub(super) fn emit_for_of_downlevel_labeled(
        &mut self,
        fo: &ForOfStmt,
        span: Span,
        labels: &[&str],
    ) {
        self.emit_for_of_downlevel_core(fo, span, labels);
    }

    fn emit_for_of_downlevel_core(&mut self, fo: &ForOfStmt, span: Span, labels: &[&str]) {
        let using_binding = self.simple_for_of_using_binding(fo);

        // Extract the binding variable name for the loop variable.
        let binding_name = for_in_of_left_binding_str(&fo.left);

        let is_destructuring = for_in_of_left_is_destructuring(&fo.left);

        if self.options.down_level_iteration == Some(true) {
            // Preserve the established iterator-path suffix allocation. The
            // indexed transform below uses TypeScript's separate inline-name
            // planner and must not perturb these names.
            self.temp_var_counter += 1;
            let suffix = self.temp_var_counter;
            let arr_var = format!("arr_{}", suffix);
            // downlevelIteration path: use __values helper
            let iter_var = arr_var.clone();
            let result_var = format!("{}_1", &arr_var);
            let err_var = format!("e_{}", suffix);
            let catch_var = format!("{}_1", &err_var);
            let finally_tmp = format!(
                "_a{}",
                if suffix == 1 {
                    String::new()
                } else {
                    format!("_{}", suffix)
                }
            );

            // var e_1, _b;
            self.write("var ");
            self.write(&err_var);
            self.write(", ");
            self.write(&finally_tmp);
            self.writeln(";");

            // try {
            self.writeln("try {");
            self.indent += 1;

            for label in labels {
                self.write(label);
                self.writeln(":");
            }

            // for (var arr_1 = __values(arr), arr_1_1 = arr_1.next(); !arr_1_1.done; arr_1_1 = arr_1.next()) {
            self.write("for (var ");
            self.write(&iter_var);
            self.write(" = ");
            self.write(self.helper_prefix());
            self.write("__values(");
            self.emit_expr(&fo.right);
            self.write("), ");
            self.write(&result_var);
            self.write(" = ");
            self.write(&iter_var);
            self.write(".next(); !");
            self.write(&result_var);
            self.write(".done; ");
            self.write(&result_var);
            self.write(" = ");
            self.write(&iter_var);
            self.write(".next()) ");

            // Emit the body wrapped in a block
            self.writeln("{");
            self.indent += 1;

            if let Some((resource_name, is_async)) = &using_binding {
                let value_name = format!("{result_var}.value");
                self.emit_native_for_of_using_iteration(
                    resource_name,
                    &value_name,
                    *is_async,
                    &fo.body,
                );
            } else {
                if let Some(plan) = self.active_lexical_for_of_plan.clone() {
                    self.emit_lexical_for_of_binding(&fo.left, &format!("{result_var}.value"));
                    self.emit_lexical_loop_call(&plan);
                } else {
                    let value = format!("{result_var}.value");
                    if !self.emit_simple_es5_for_of_declaration_binding(fo, span, &value) {
                        // Assignment-form headers retain their original target
                        // instead of introducing a declaration. This also routes
                        // parser recovery placeholders through the span-aware
                        // for-in/of emitter (for example `obj.value`).
                        let is_assignment =
                            matches!(&fo.left, ForInOfLeft::Pat(_) | ForInOfLeft::Expr(_));
                        if is_assignment {
                            self.emit_for_in_of_left(&fo.left);
                        } else {
                            if let ForInOfLeft::Var(var) = &fo.left {
                                if let Some(declaration) = var.declarations.first() {
                                    self.emit_moved_line_comments_in_range(
                                        declaration.full_start,
                                        declaration.name.span.start,
                                    );
                                    self.emit_moved_line_comments_in_range(
                                        declaration.name.span.end,
                                        fo.right.span.start,
                                    );
                                }
                            }
                            // Suspending functions retain a native lexical
                            // per-iteration binding so closures do not collapse
                            // while the surrounding iterator is downleveled.
                            self.emit_downlevel_for_of_binding_keyword(&fo.left);
                            self.write(" ");
                            if is_destructuring {
                                self.emit_for_in_of_left_binding(&fo.left);
                            } else {
                                self.write(&binding_name);
                            }
                            if let ForInOfLeft::Var(var) = &fo.left {
                                if let Some(declaration) = var.declarations.first() {
                                    self.emit_moved_block_comments_in_range(
                                        declaration.full_start,
                                        declaration.name.span.start,
                                    );
                                    self.emit_moved_block_comments_in_range(
                                        declaration.name.span.end,
                                        fo.right.span.start,
                                    );
                                }
                            }
                        }
                        self.write(" = ");
                        self.write(&result_var);
                        self.writeln(".value;");
                    }
                    // Emit the body statements
                    self.emit_for_of_body_stmts(&fo.body);
                }
            }

            self.indent -= 1;
            self.writeln("}");

            self.indent -= 1;
            self.writeln("}");

            // catch (e_1_1) { e_1 = { error: e_1_1 }; }
            self.write("catch (");
            self.write(&catch_var);
            self.write(") { ");
            self.write(&err_var);
            self.write(" = { error: ");
            self.write(&catch_var);
            self.writeln(" }; }");

            // finally {
            self.writeln("finally {");
            self.indent += 1;

            // try {
            //     if (arr_1_1 && !arr_1_1.done && (_a = arr_1.return)) _a.call(arr_1);
            // }
            self.writeln("try {");
            self.indent += 1;
            self.write("if (");
            self.write(&result_var);
            self.write(" && !");
            self.write(&result_var);
            self.write(".done && (");
            self.write(&finally_tmp);
            self.write(" = ");
            self.write(&iter_var);
            self.write(".return)) ");
            self.write(&finally_tmp);
            self.write(".call(");
            self.write(&iter_var);
            self.writeln(");");
            self.indent -= 1;
            self.writeln("}");

            // finally { if (e_1) throw e_1.error; }
            self.write("finally { if (");
            self.write(&err_var);
            self.write(") throw ");
            self.write(&err_var);
            self.writeln(".error; }");

            self.indent -= 1;
            self.writeln("}");
        } else {
            // Simple index-based transform (no downlevelIteration)
            // for (var _i = 0, arr_1 = arr; _i < arr_1.length; _i++) {
            let idx_var = self.next_simple_loop_index_name();
            let arr_var = self.next_indexed_for_of_rhs_name(&fo.right);
            for label in labels {
                self.write(label);
                self.writeln(":");
            }
            self.write("for (var ");
            self.write(&idx_var);
            self.write(" = 0, ");
            self.write(&arr_var);
            self.write(" = ");
            self.emit_expr(&fo.right);
            self.write("; ");
            self.write(&idx_var);
            self.write(" < ");
            self.write(&arr_var);
            self.write(".length; ");
            self.write(&idx_var);
            self.write("++) ");

            // Emit the body wrapped in a block
            self.writeln("{");
            self.indent += 1;

            if let Some((resource_name, is_async)) = &using_binding {
                let value_name = format!("{arr_var}[{idx_var}]");
                self.emit_native_for_of_using_iteration(
                    resource_name,
                    &value_name,
                    *is_async,
                    &fo.body,
                );
            } else {
                if let Some(plan) = self.active_lexical_for_of_plan.clone() {
                    self.emit_lexical_for_of_binding(&fo.left, &format!("{arr_var}[{idx_var}]"));
                    self.emit_lexical_loop_call(&plan);
                } else {
                    let value = format!("{arr_var}[{idx_var}]");
                    if !self.emit_simple_es5_for_of_declaration_binding(fo, span, &value) {
                        // Assignment-form headers retain their original target
                        // instead of introducing a declaration. This also routes
                        // parser recovery placeholders through the span-aware
                        // for-in/of emitter (for example `obj.value`).
                        let is_assignment =
                            matches!(&fo.left, ForInOfLeft::Pat(_) | ForInOfLeft::Expr(_));
                        if is_assignment {
                            self.emit_for_in_of_left(&fo.left);
                        } else {
                            if let ForInOfLeft::Var(var) = &fo.left {
                                if let Some(declaration) = var.declarations.first() {
                                    self.emit_moved_line_comments_in_range(
                                        declaration.full_start,
                                        declaration.name.span.start,
                                    );
                                    self.emit_moved_line_comments_in_range(
                                        declaration.name.span.end,
                                        fo.right.span.start,
                                    );
                                }
                            }
                            self.emit_downlevel_for_of_binding_keyword(&fo.left);
                            self.write(" ");
                            if is_destructuring {
                                self.emit_for_in_of_left_binding(&fo.left);
                            } else {
                                self.write(&binding_name);
                            }
                            if let ForInOfLeft::Var(var) = &fo.left {
                                if let Some(declaration) = var.declarations.first() {
                                    self.emit_moved_block_comments_in_range(
                                        declaration.full_start,
                                        declaration.name.span.start,
                                    );
                                    self.emit_moved_block_comments_in_range(
                                        declaration.name.span.end,
                                        fo.right.span.start,
                                    );
                                }
                            }
                        }
                        self.write(" = ");
                        self.write(&arr_var);
                        self.write("[");
                        self.write(&idx_var);
                        self.writeln("];");
                    }
                    // Emit the body statements
                    self.emit_for_of_body_stmts(&fo.body);
                }
            }

            self.indent -= 1;
            self.writeln("}");
        }
    }

    /// Expand a narrowly supported declaration-form binding into one compact
    /// ES5 declarator list. Assignment-form headers never enter this path.
    fn emit_simple_es5_for_of_declaration_binding(
        &mut self,
        fo: &ForOfStmt,
        span: Span,
        value: &str,
    ) -> bool {
        if self.lexical_downlevel_plan.loop_preserves_native(span) {
            return false;
        }
        let Some(declaration) = crate::analysis::eligible_es5_for_of_declaration(&fo.left) else {
            return false;
        };
        if self.has_comments_in_range(declaration.full_start, fo.right.span.start) {
            return false;
        }

        if matches!(&declaration.name.kind, PatKind::Ident(name) if name == "<error>") {
            let discard = self.next_inline_temp_var();
            self.write("var ");
            self.write(&discard);
            self.write(" = ");
            self.write(value);
            self.writeln(";");
            return true;
        }

        let is_array = crate::analysis::simple_es5_for_of_array_binding(&declaration.name);
        let is_object = crate::analysis::simple_es5_for_of_object_binding(&declaration.name);
        if !(is_array || is_object) {
            return false;
        }
        if is_array
            && self.options.down_level_iteration == Some(true)
            && self
                .lexical_downlevel_plan
                .simple_iterator_array_comment_range(span)
                .is_none()
        {
            return false;
        }

        let snapshot = self.next_inline_temp_var();
        self.write("var ");
        self.write(&snapshot);
        self.write(" = ");
        if is_array && self.options.down_level_iteration == Some(true) {
            let PatKind::Array(elements) = &declaration.name.kind else {
                unreachable!();
            };
            self.write(self.helper_prefix());
            self.write("__read(");
            self.write(value);
            self.write(", ");
            self.write(&elements.len().to_string());
            self.write(")");
        } else {
            self.write(value);
        }

        match &declaration.name.kind {
            PatKind::Array(elements) => {
                for (index, element) in elements.iter().enumerate() {
                    let Some(ArrayPatElem::Pat(binding)) = element else {
                        continue;
                    };
                    self.write(", ");
                    self.emit_binding_name(binding);
                    self.write(" = ");
                    self.write(&snapshot);
                    self.write("[");
                    self.write(&index.to_string());
                    self.write("]");
                }
            }
            PatKind::Object(properties) => {
                for property in properties {
                    let (key, binding) = match property {
                        ObjPatProp::Shorthand(name, span) => {
                            let binding = Pat {
                                kind: PatKind::Ident(name.clone()),
                                span: *span,
                            };
                            (name.as_str(), binding)
                        }
                        ObjPatProp::KeyValue(PropName::Ident(key, _), binding) => {
                            (key.as_str(), binding.clone())
                        }
                        _ => unreachable!(),
                    };
                    self.write(", ");
                    self.emit_binding_name(&binding);
                    self.write(" = ");
                    self.write(&snapshot);
                    self.write(".");
                    self.write(key);
                }
            }
            _ => unreachable!(),
        }
        self.writeln(";");
        true
    }

    /// Emit the binding pattern from a ForInOfLeft (for downlevel transforms).
    pub(super) fn emit_for_in_of_left_binding(&mut self, left: &ForInOfLeft) {
        match left {
            ForInOfLeft::Var(vs) => {
                if let Some(decl) = vs.declarations.first() {
                    self.emit_binding_name(&decl.name);
                }
            }
            ForInOfLeft::Pat(pat) => self.emit_binding_name(pat),
            ForInOfLeft::Expr(expr) => self.emit_expr(expr),
        }
    }

    fn emit_downlevel_for_of_binding_keyword(&mut self, left: &ForInOfLeft) {
        let Some(var) = (match left {
            ForInOfLeft::Var(var) => Some(var),
            ForInOfLeft::Pat(_) | ForInOfLeft::Expr(_) => None,
        }) else {
            self.write("var");
            return;
        };
        self.write(self.emitted_var_keyword(var));
    }

    /// Emit the body statements of a for-of loop (unwrapping the block if needed).
    pub(super) fn emit_for_of_body_stmts(&mut self, body: &Stmt) {
        match &body.kind {
            StmtKind::Block(stmts) => {
                if let Some(first_using) = crate::emit_stmt::first_using_index(stmts) {
                    for s in &stmts[..first_using] {
                        self.emit_leading_comments(s.span.start);
                        self.emit_stmt(s);
                        self.advance_comment_pos(s.span.end);
                    }
                    self.emit_block_using_dispose_scope(&stmts[first_using..]);
                } else {
                    for s in stmts {
                        self.emit_leading_comments(s.span.start);
                        self.emit_stmt(s);
                        self.advance_comment_pos(s.span.end);
                    }
                }
            }
            _ => {
                self.emit_stmt(body);
            }
        }
    }

    /// Emit `{...a, b: 1}` as `Object.assign({}, a, { b: 1 })` for target < ES2018.
    /// TypeScript nests calls: `{...a, ...b}` → `Object.assign(Object.assign({}, a), b)`.
    /// Each group after the first spread wraps in another `Object.assign`.
    pub(super) fn emit_object_spread_downlevel(
        &mut self,
        props: &[ObjLitProp],
        obj_lit_span_end: u32,
    ) {
        // Group properties into chunks: each spread is a separate argument,
        // consecutive non-spread props are grouped into object literals.
        let mut groups: Vec<Vec<&ObjLitProp>> = Vec::new();
        let mut current_group: Vec<&ObjLitProp> = Vec::new();
        let mut first_is_spread = false;

        for (i, prop) in props.iter().enumerate() {
            if matches!(prop, ObjLitProp::Spread(_, _)) {
                if i == 0 {
                    first_is_spread = true;
                }
                if !current_group.is_empty() {
                    groups.push(current_group);
                    current_group = Vec::new();
                }
                groups.push(vec![prop]);
            } else {
                current_group.push(prop);
            }
        }
        if !current_group.is_empty() {
            groups.push(current_group);
        }

        // Find the index of the first spread group.
        let first_spread_idx = groups
            .iter()
            .position(|g| g.len() == 1 && matches!(g[0], ObjLitProp::Spread(_, _)))
            .unwrap_or(0);

        // Count groups after the first spread — each wraps in Object.assign.
        let groups_after_first_spread = if groups.len() > first_spread_idx + 1 {
            groups.len() - first_spread_idx - 1
        } else {
            0
        };

        // If the first element is a spread of a plain object literal (no inner spreads),
        // the literal itself becomes the first argument directly (no extra {} prefix,
        // and one fewer Object.assign nesting level).
        let first_is_plain_obj_spread = first_is_spread
            && groups.first().and_then(|g| g.first()).is_some_and(|p| {
                if let ObjLitProp::Spread(e, _) = p {
                    if let ExprKind::ObjectLit(inner_props) = &e.kind {
                        return !inner_props
                            .iter()
                            .any(|ip| matches!(ip, ObjLitProp::Spread(_, _)));
                    }
                }
                false
            });

        // Emit nested Object.assign prefixes: one for the base call, plus
        // one for each group after the first spread. When first spread is a
        // plain object literal, it replaces the {} prefix so one fewer nesting.
        let nesting_adjust = if first_is_plain_obj_spread { 1 } else { 0 };
        for _ in 0..groups_after_first_spread.saturating_sub(nesting_adjust) {
            self.write("Object.assign(");
        }
        self.write("Object.assign(");

        // If the first element is a spread, start with an empty object target
        // UNLESS it's a plain object literal (already handled as direct first arg).
        if first_is_spread && !first_is_plain_obj_spread {
            self.write("{}, ");
        }

        // Check for a trailing line comment after the last prop in the source
        // (e.g., `...expr // comment`). This comment sits between the last
        // property's span end and the object literal's closing `}`.
        let trailing_comment = if let Some(last_prop) = props.last() {
            let last_end = obj_lit_prop_span_end(last_prop) as usize;
            find_line_comment_in_range(self.source, last_end, obj_lit_span_end as usize)
                .map(|s| s.to_string())
        } else {
            None
        };

        // Emit the innermost call: groups up to and including the first spread.
        // When first spread is a plain object literal, include one more group
        // in the innermost call (the literal serves as direct first arg).
        let inner_end = (first_spread_idx + nesting_adjust).min(groups.len() - 1);
        for gi in 0..=inner_end {
            if gi > 0 {
                self.write(", ");
            }
            self.emit_object_spread_group(&groups[gi]);
        }

        let has_subsequent = inner_end + 1 < groups.len();
        if !has_subsequent {
            // Outermost closing paren — emit trailing comment if present
            if let Some(ref comment) = trailing_comment {
                self.write(" ");
                self.write(comment);
                self.newline();
            }
        }
        self.write(")");

        // Each subsequent group closes the previous call and adds another arg.
        let subsequent_end = groups.len();
        for gi in (inner_end + 1)..subsequent_end {
            self.write(", ");
            self.emit_object_spread_group(&groups[gi]);
            if gi == subsequent_end - 1 {
                // Outermost closing paren — emit trailing comment if present
                if let Some(ref comment) = trailing_comment {
                    self.write(" ");
                    self.write(comment);
                    self.newline();
                }
            }
            self.write(")");
        }
    }

    fn emit_object_spread_group(&mut self, group: &[&ObjLitProp]) {
        if group.len() == 1 {
            if let ObjLitProp::Spread(ref expr, _) = group[0] {
                let can_copy_multiline_binary_spread = if let ExprKind::Paren(inner) = &expr.kind {
                    let s = expr.span.start as usize;
                    let e = expr.span.end as usize;
                    s < e
                        && e <= self.source.len()
                        && self.source[s..e].contains('\n')
                        && !self.source[s..e].contains("//")
                        && !self.source[s..e].contains("/*")
                        && matches!(&inner.kind, ExprKind::Binary(_))
                } else {
                    false
                };
                if can_copy_multiline_binary_spread {
                    let s = expr.span.start as usize;
                    let e = expr.span.end as usize;
                    let src = &self.source[s..e];
                    let base_indent = self
                        .output
                        .rsplit('\n')
                        .next()
                        .map(|line| line.len().saturating_sub(line.trim_start().len()))
                        .unwrap_or(0);
                    let mut lines = src.lines();
                    if let Some(first) = lines.next() {
                        let first_trimmed = first.trim_end();
                        let first_normalized = if self.skip_brace_normalize {
                            std::borrow::Cow::Borrowed(first_trimmed)
                        } else {
                            normalize_brace_spacing(first_trimmed)
                        };
                        // keyword_paren + import_call_spacing +
                        // comment_word_space → one unified pass.
                        let first_normalized = normalize_unified_pass(&first_normalized);
                        self.write(&first_normalized);

                        let remaining: Vec<&str> = lines.collect();
                        let min_indent = remaining
                            .iter()
                            .filter(|line| !line.trim().is_empty())
                            .map(|line| line.len().saturating_sub(line.trim_start().len()))
                            .min()
                            .unwrap_or(0);
                        // Detect indent unit for normalization (e.g. 2-space → 4-space).
                        let indent_unit = remaining
                            .iter()
                            .filter(|line| !line.trim().is_empty())
                            .map(|line| line.len().saturating_sub(line.trim_start().len()))
                            .filter(|&ind| ind > min_indent)
                            .map(|ind| ind - min_indent)
                            .min()
                            .unwrap_or(4)
                            .max(1);
                        for line in remaining {
                            if line.trim().is_empty() {
                                continue;
                            }
                            self.output.push('\n');
                            self.out_line += 1;
                            self.out_col = 0;
                            self.at_line_start = false;

                            let line_indent = line.len().saturating_sub(line.trim_start().len());
                            let relative = line_indent.saturating_sub(min_indent);
                            let normalized_relative = if indent_unit != 4 {
                                let levels = relative / indent_unit;
                                let extra = relative % indent_unit;
                                levels * 4 + extra
                            } else {
                                relative
                            };
                            let pad = base_indent + normalized_relative;
                            if pad > 0 {
                                self.output.push_str(&" ".repeat(pad));
                                self.out_col += pad as u32;
                            }
                            let raw_content = line.trim_start().trim_end();
                            let normalized = if self.skip_brace_normalize {
                                std::borrow::Cow::Borrowed(raw_content)
                            } else {
                                normalize_brace_spacing(raw_content)
                            };
                            // keyword_paren + import_call_spacing +
                            // trailing_semi + comment_word_space → one
                            // unified pass.
                            let normalized = normalize_unified_pass(&normalized);
                            self.output.push_str(&normalized);
                            self.track_position(&normalized);
                        }
                        return;
                    }
                }
                self.suppress_oc_parens = true;
                self.emit_expr(expr);
                return;
            }
        }
        self.write("{ ");
        self.indent += 1;
        for (j, prop) in group.iter().enumerate() {
            if j > 0 {
                self.write(", ");
            }
            self.emit_obj_lit_prop(prop);
        }
        self.indent -= 1;
        self.write(" }");
    }

    /// In JS files, type arguments on Call/TaggedTemplate/Instantiation are not
    /// TypeScript type parameters — they are binary `<` and `>` operators.
    /// Emit them with spaces to match TypeScript's output, e.g. `Foo < number > `.
    pub(super) fn emit_js_type_args_as_binary(&mut self, type_args: &[TypeNode]) {
        self.write(" < ");
        for (i, ta) in type_args.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            let start = ta.span.start as usize;
            let end = ta.span.end as usize;
            if end <= self.source.len() {
                self.write(&self.source[start..end]);
            }
        }
        self.write(" > ");
    }

    fn emit_jsdoc_recovery_type_args(&mut self, type_args: &[TypeNode]) {
        self.write("<");
        for (i, ta) in type_args.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            self.emit_jsdoc_recovery_type_node(ta);
        }
        self.write(">");
    }

    fn emit_jsdoc_recovery_type_node(&mut self, ty: &TypeNode) {
        match &ty.kind {
            TypeNodeKind::JSDocNullable(inner) => {
                self.write("?");
                if let Some(inner) = inner {
                    self.emit_jsdoc_recovery_type_node(inner);
                }
            }
            TypeNodeKind::Reference(type_ref) => {
                self.copy_span(type_ref.name.span);
                if let Some(args) = &type_ref.type_args {
                    self.emit_jsdoc_recovery_type_args(args);
                }
            }
            TypeNodeKind::Keyword(kw) => {
                let text = match kw {
                    KeywordTypeKind::Any => "any",
                    KeywordTypeKind::Unknown => "unknown",
                    KeywordTypeKind::Number => "number",
                    KeywordTypeKind::BigInt => "bigint",
                    KeywordTypeKind::String => "string",
                    KeywordTypeKind::Boolean => "boolean",
                    KeywordTypeKind::Void => "void",
                    KeywordTypeKind::Undefined => "undefined",
                    KeywordTypeKind::Null => "null",
                    KeywordTypeKind::Never => "never",
                    KeywordTypeKind::Object => "object",
                    KeywordTypeKind::Symbol => "symbol",
                    KeywordTypeKind::Intrinsic => "intrinsic",
                };
                self.write(text);
            }
            TypeNodeKind::Array(inner) => {
                self.emit_jsdoc_recovery_type_node(inner);
                self.write("[]");
            }
            TypeNodeKind::Tuple(elements) => {
                self.write("[");
                for (i, elem) in elements.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    if elem.dotdotdot {
                        self.write("...");
                    }
                    if let Some(label) = &elem.label {
                        self.write(label);
                        if elem.optional {
                            self.write("?");
                        }
                        self.write(": ");
                    }
                    self.emit_jsdoc_recovery_type_node(&elem.type_node);
                    if elem.label.is_none() && elem.optional {
                        self.write("?");
                    }
                }
                self.write("]");
            }
            TypeNodeKind::Union(types) => {
                for (i, inner) in types.iter().enumerate() {
                    if i > 0 {
                        self.write(" | ");
                    }
                    self.emit_jsdoc_recovery_type_node(inner);
                }
            }
            TypeNodeKind::Intersection(types) => {
                for (i, inner) in types.iter().enumerate() {
                    if i > 0 {
                        self.write(" & ");
                    }
                    self.emit_jsdoc_recovery_type_node(inner);
                }
            }
            TypeNodeKind::Paren(inner) => {
                self.write("(");
                self.emit_jsdoc_recovery_type_node(inner);
                self.write(")");
            }
            _ => {
                let start = ty.span.start as usize;
                let end = ty.span.end as usize;
                if end <= self.source.len() {
                    self.write(&self.source[start..end]);
                }
            }
        }
    }

    /// Collect replacement entries that insert missing semicolons inside arrow/function
    /// block bodies within JSX attributes. JSX preserve mode copies source text, so
    /// semicolons omitted in the source must be inserted as text replacements.
    fn collect_missing_semi_replacements_for_jsx_attrs(
        &self,
        attrs: &[JsxAttribute],
        reps: &mut Vec<(usize, usize, String)>,
    ) {
        for attr in attrs {
            match attr {
                JsxAttribute::Spread(e, _) => {
                    self.collect_missing_semi_replacements_for_expr(e, reps);
                }
                JsxAttribute::Normal { value: Some(v), .. } => {
                    self.collect_missing_semi_replacements_for_expr(v, reps);
                }
                _ => {}
            }
        }
    }

    fn collect_missing_semi_replacements_for_expr(
        &self,
        expr: &Expr,
        reps: &mut Vec<(usize, usize, String)>,
    ) {
        let mut stack = vec![expr];
        while let Some(expr) = stack.pop() {
            match &expr.kind {
                ExprKind::Arrow(arrow) => {
                    if let ArrowBody::Block(stmts) = &arrow.body {
                        // 1. Add semicolons first (order matters for same-position replacements)
                        for s in stmts {
                            if stmt_needs_trailing_semicolon(s) {
                                let end = s.span.end as usize;
                                if end > 0 && end <= self.source.len() {
                                    let ch = self.source.as_bytes()[end - 1];
                                    if ch != b';' {
                                        let trimmed = self.source[..end].trim_end();
                                        if !trimmed.ends_with(';') {
                                            reps.push((end, end, ";".to_string()));
                                        }
                                    }
                                }
                            }
                            // Recurse into statements for nested expressions
                            if let StmtKind::Expr(e) = &s.kind {
                                stack.push(e);
                            } else if let StmtKind::Var(vs) = &s.kind {
                                for d in &vs.declarations {
                                    if let Some(init) = &d.init {
                                        stack.push(init);
                                    }
                                }
                            } else if let StmtKind::Return(Some(e)) = &s.kind {
                                stack.push(e);
                            }
                        }
                        // 2. Add brace/arrow spacing for single-line block bodies
                        if !stmts.is_empty() {
                            let arrow_start = arrow.span.start as usize;
                            let arrow_end = arrow.span.end as usize;
                            let first_stmt_start = stmts[0].span.start as usize;
                            let last_stmt_end = stmts.last().unwrap().span.end as usize;
                            if arrow_end <= self.source.len() && first_stmt_start > arrow_start {
                                let search_open = &self.source[arrow_start..first_stmt_start];
                                if let Some(brace_rel) = search_open.rfind('{') {
                                    let open_brace = arrow_start + brace_rel;
                                    if last_stmt_end <= arrow_end {
                                        let search_close = &self.source[last_stmt_end..arrow_end];
                                        if let Some(close_rel) = search_close.find('}') {
                                            let close_brace = last_stmt_end + close_rel;
                                            // Only for single-line blocks
                                            if !self.source[open_brace..=close_brace].contains('\n')
                                            {
                                                // Space after =>: find => between arrow start and {
                                                let before_brace =
                                                    &self.source[arrow_start..open_brace];
                                                if let Some(arrow_rel) = before_brace.rfind("=>") {
                                                    let after_arrow = arrow_start + arrow_rel + 2;
                                                    if after_arrow <= open_brace
                                                        && self.source.as_bytes()[after_arrow]
                                                            != b' '
                                                    {
                                                        reps.push((
                                                            after_arrow,
                                                            after_arrow,
                                                            " ".to_string(),
                                                        ));
                                                    }
                                                }
                                                // Space after {
                                                if open_brace + 1 < self.source.len()
                                                    && self.source.as_bytes()[open_brace + 1]
                                                        != b' '
                                                    && self.source.as_bytes()[open_brace + 1]
                                                        != b'\n'
                                                {
                                                    reps.push((
                                                        open_brace + 1,
                                                        open_brace + 1,
                                                        " ".to_string(),
                                                    ));
                                                }
                                                // Space before } (after semicolons in sort order)
                                                if close_brace > 0
                                                    && self.source.as_bytes()[close_brace - 1]
                                                        != b' '
                                                    && self.source.as_bytes()[close_brace - 1]
                                                        != b'\n'
                                                {
                                                    reps.push((
                                                        close_brace,
                                                        close_brace,
                                                        " ".to_string(),
                                                    ));
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                ExprKind::FnExpr(fn_decl) => {
                    if let Some(stmts) = &fn_decl.body {
                        for s in stmts {
                            if stmt_needs_trailing_semicolon(s) {
                                let end = s.span.end as usize;
                                if end > 0 && end <= self.source.len() {
                                    let ch = self.source.as_bytes()[end - 1];
                                    if ch != b';' {
                                        let trimmed = self.source[..end].trim_end();
                                        if !trimmed.ends_with(';') {
                                            reps.push((end, end, ";".to_string()));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                ExprKind::ObjectLit(props) => {
                    for prop in props {
                        match prop {
                            ObjLitProp::Property(p) => stack.push(&p.value),
                            ObjLitProp::Spread(e, _) => stack.push(e),
                            ObjLitProp::Method(m) => {
                                for s in &m.body {
                                    if stmt_needs_trailing_semicolon(s) {
                                        let end = s.span.end as usize;
                                        if end > 0 && end <= self.source.len() {
                                            let ch = self.source.as_bytes()[end - 1];
                                            if ch != b';' {
                                                let trimmed = self.source[..end].trim_end();
                                                if !trimmed.ends_with(';') {
                                                    reps.push((end, end, ";".to_string()));
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                ExprKind::Call(call) => {
                    stack.push(&call.callee);
                    for arg in &call.args {
                        stack.push(arg);
                    }
                }
                ExprKind::Paren(inner) | ExprKind::NonNull(inner) => stack.push(inner),
                ExprKind::Cond(cond) => {
                    stack.push(&cond.test);
                    stack.push(&cond.consequent);
                    stack.push(&cond.alternate);
                }
                ExprKind::Binary(bin) => {
                    stack.push(&bin.left);
                    stack.push(&bin.right);
                }
                ExprKind::ArrayLit(elements) => {
                    for e in elements.iter().flatten() {
                        stack.push(e);
                    }
                }
                _ => {}
            }
        }
    }

    // -----------------------------------------------------------------------
    // Reflect.set transform for super access in decorated class static context
    // -----------------------------------------------------------------------

    /// Check if the LHS of an assignment (after unwrapping parens/type assertions)
    /// is a `super.x` or `super[expr]` access that needs Reflect.set transform.
    fn is_super_member_lhs(&self, expr: &Expr) -> bool {
        if self.static_super_base_alias.is_none() {
            return false;
        }
        match &expr.kind {
            ExprKind::Member(mem) => {
                !mem.optional
                    && matches!(&mem.object.kind, ExprKind::Super)
                    && mem.property != "<error>"
                    && !mem.property.starts_with('#')
            }
            ExprKind::ElemAccess(ea) => !ea.optional && matches!(&ea.object.kind, ExprKind::Super),
            ExprKind::Paren(inner) => self.is_super_member_lhs(inner),
            _ => false,
        }
    }

    /// Emit `Reflect.set(base, key, val, receiver)` for a super member assignment.
    /// `emit_val` is called to emit the value expression inside the Reflect.set call.
    fn emit_reflect_set_super(&mut self, lhs: &Expr, emit_val: impl FnOnce(&mut Self)) {
        let base = self.static_super_base_alias.clone().unwrap();
        let recv = self.static_super_receiver_alias.clone().unwrap();
        self.write("Reflect.set(");
        self.write(&base);
        self.write(", ");
        match &lhs.kind {
            ExprKind::Member(mem) => {
                self.write("\"");
                self.write(&mem.property);
                self.write("\"");
            }
            ExprKind::ElemAccess(ea) => {
                self.emit_expr(&ea.index);
            }
            ExprKind::Paren(inner) => {
                // Recurse to get the key from the inner member/elem access
                match &inner.kind {
                    ExprKind::Member(mem) => {
                        self.write("\"");
                        self.write(&mem.property);
                        self.write("\"");
                    }
                    ExprKind::ElemAccess(ea) => {
                        self.emit_expr(&ea.index);
                    }
                    _ => self.write("\"\""),
                }
            }
            _ => self.write("\"\""),
        }
        self.write(", ");
        emit_val(self);
        self.write(", ");
        self.write(&recv);
        self.write(")");
    }

    /// For computed key expressions (identifiers/non-literal), check if the key
    /// needs to be captured in a temp var to avoid double evaluation.
    fn super_elem_key_is_computed(&self, lhs: &Expr) -> bool {
        match &lhs.kind {
            ExprKind::ElemAccess(ea) => {
                // String literals and numeric literals don't need temp capture
                !matches!(&ea.index.kind, ExprKind::StrLit(_) | ExprKind::NumLit(_))
            }
            ExprKind::Paren(inner) => self.super_elem_key_is_computed(inner),
            _ => false, // member access uses string key, no temp needed
        }
    }

    /// Emit a super assignment expression with Reflect.set transform.
    /// Returns true if the transform was applied.
    pub(super) fn emit_super_reflect_assign(&mut self, assign: &AssignExpr) -> bool {
        if !self.is_super_member_lhs(&assign.left) {
            return false;
        }
        let in_field = self.super_reflect_in_field_init;
        // In static field initializer context, Reflect.set returns boolean not value,
        // so we wrap in IIFE: (() => { var _a; return Reflect.set(base, key, _a = val, recv), _a; })()
        let saved_temps = if in_field {
            let saved = (
                std::mem::take(&mut self.temp_var_names),
                self.temp_var_counter,
            );
            self.temp_var_counter = 0;
            Some(saved)
        } else {
            None
        };
        if in_field {
            self.write("(() => {");
            self.output.push('\n');
            self.out_line += 1;
            self.out_col = 0;
            self.at_line_start = true;
            self.indent += 1;
        }
        let var_insert_pos = if in_field {
            Some(self.output.len())
        } else {
            None
        };
        let base = self.static_super_base_alias.clone().unwrap();
        let recv = self.static_super_receiver_alias.clone().unwrap();
        if assign.op == AssignOp::Assign {
            if in_field {
                let val_temp = self.next_temp_var();
                self.write("return Reflect.set(");
                self.write(&base);
                self.write(", ");
                self.emit_super_lhs_key(&assign.left);
                self.write(", ");
                self.write(&val_temp);
                self.write(" = ");
                self.emit_expr(&assign.right);
                self.write(", ");
                self.write(&recv);
                self.write("), ");
                self.write(&val_temp);
                self.write(";");
            } else {
                // Non-IIFE: Reflect.set(base, key, val, receiver)
                self.emit_reflect_set_super(&assign.left, |s| {
                    s.emit_expr(&assign.right);
                });
            }
        } else {
            // Compound assignment
            let needs_key_temp = self.super_elem_key_is_computed(&assign.left);
            let key_temp = if needs_key_temp {
                Some(self.next_temp_var())
            } else {
                None
            };
            let bin_op = compound_assign_to_bin_op_str(assign.op);
            if in_field {
                let val_temp = self.next_temp_var();
                self.write("return Reflect.set(");
                self.write(&base);
                self.write(", ");
                if let Some(ref kt) = key_temp {
                    self.write(kt);
                    self.write(" = ");
                    self.emit_super_lhs_key(&assign.left);
                } else {
                    self.emit_super_lhs_key(&assign.left);
                }
                self.write(", ");
                self.write(&val_temp);
                self.write(" = Reflect.get(");
                self.write(&base);
                self.write(", ");
                if let Some(ref kt) = key_temp {
                    self.write(kt);
                } else {
                    self.emit_super_lhs_key(&assign.left);
                }
                self.write(", ");
                self.write(&recv);
                self.write(") ");
                self.write(bin_op);
                self.write(" ");
                self.emit_expr(&assign.right);
                self.write(", ");
                self.write(&recv);
                self.write("), ");
                self.write(&val_temp);
                self.write(";");
            } else {
                self.write("Reflect.set(");
                self.write(&base);
                self.write(", ");
                if let Some(ref kt) = key_temp {
                    self.write(kt);
                    self.write(" = ");
                    self.emit_super_lhs_key(&assign.left);
                } else {
                    self.emit_super_lhs_key(&assign.left);
                }
                self.write(", Reflect.get(");
                self.write(&base);
                self.write(", ");
                if let Some(ref kt) = key_temp {
                    self.write(kt);
                } else {
                    self.emit_super_lhs_key(&assign.left);
                }
                self.write(", ");
                self.write(&recv);
                self.write(") ");
                self.write(bin_op);
                self.write(" ");
                self.emit_expr(&assign.right);
                self.write(", ");
                self.write(&recv);
                self.write(")");
            }
        }
        if in_field {
            self.output.push('\n');
            self.out_line += 1;
            self.out_col = 0;
            self.at_line_start = true;
            // Insert var declaration at saved position
            if let Some(pos) = var_insert_pos {
                if !self.temp_var_names.is_empty() {
                    let indent_str = "    ".repeat(self.indent as usize);
                    let var_decl =
                        format!("{}var {};\n", indent_str, self.temp_var_names.join(", "));
                    self.output.insert_str(pos, &var_decl);
                }
            }
            self.indent -= 1;
            self.write("})()");
        }
        if let Some((prev_names, prev_counter)) = saved_temps {
            self.temp_var_names = prev_names;
            self.temp_var_counter = prev_counter;
        }
        true
    }

    /// Emit a super update expression with Reflect.set transform.
    /// Returns true if the transform was applied.
    pub(super) fn emit_super_reflect_update(&mut self, up: &UpdateExpr) -> bool {
        if !self.is_super_member_lhs(&up.argument) {
            return false;
        }
        let in_field = self.super_reflect_in_field_init;
        let saved_temps = if in_field {
            let saved = (
                std::mem::take(&mut self.temp_var_names),
                self.temp_var_counter,
            );
            self.temp_var_counter = 0;
            Some(saved)
        } else {
            None
        };
        if in_field {
            self.write("(() => {");
            self.output.push('\n');
            self.out_line += 1;
            self.out_col = 0;
            self.at_line_start = true;
            self.indent += 1;
        }
        let var_insert_pos = if in_field {
            Some(self.output.len())
        } else {
            None
        };
        let base = self.static_super_base_alias.clone().unwrap();
        let recv = self.static_super_receiver_alias.clone().unwrap();
        let needs_key_temp = self.super_elem_key_is_computed(&up.argument);
        let key_temp = if needs_key_temp {
            Some(self.next_temp_var())
        } else {
            None
        };
        let is_prefix = matches!(up.op, UpdateOp::PreInc | UpdateOp::PreDec);
        let op_str = match up.op {
            UpdateOp::PreInc | UpdateOp::PostInc => "++",
            UpdateOp::PreDec | UpdateOp::PostDec => "--",
        };
        if in_field {
            // IIFE mode: two value temps for proper return value
            // Postfix: (() => { var _a, _b; return Reflect.set(base, key, (_b = Reflect.get(...), _a = _b++, _b), recv), _a; })()
            // Prefix:  (() => { var _a, _b; return Reflect.set(base, key, (_b = Reflect.get(...), _a = ++_b), recv), _a; })()
            let result_temp = self.next_temp_var();
            let inter_temp = self.next_temp_var();
            self.write("return Reflect.set(");
            self.write(&base);
            self.write(", ");
            if let Some(ref kt) = key_temp {
                self.write(kt);
                self.write(" = ");
                self.emit_super_lhs_key(&up.argument);
            } else {
                self.emit_super_lhs_key(&up.argument);
            }
            self.write(", (");
            self.write(&inter_temp);
            self.write(" = Reflect.get(");
            self.write(&base);
            self.write(", ");
            if let Some(ref kt) = key_temp {
                self.write(kt);
            } else {
                self.emit_super_lhs_key(&up.argument);
            }
            self.write(", ");
            self.write(&recv);
            self.write("), ");
            if is_prefix {
                self.write(&result_temp);
                self.write(" = ");
                self.write(op_str);
                self.write(&inter_temp);
            } else {
                self.write(&result_temp);
                self.write(" = ");
                self.write(&inter_temp);
                self.write(op_str);
                self.write(", ");
                self.write(&inter_temp);
            }
            self.write("), ");
            self.write(&recv);
            self.write("), ");
            self.write(&result_temp);
            self.write(";");
        } else {
            // Non-IIFE: single temp
            let val_temp = self.next_temp_var();
            self.write("Reflect.set(");
            self.write(&base);
            self.write(", ");
            if let Some(ref kt) = key_temp {
                self.write(kt);
                self.write(" = ");
                self.emit_super_lhs_key(&up.argument);
            } else {
                self.emit_super_lhs_key(&up.argument);
            }
            self.write(", (");
            self.write(&val_temp);
            self.write(" = Reflect.get(");
            self.write(&base);
            self.write(", ");
            if let Some(ref kt) = key_temp {
                self.write(kt);
            } else {
                self.emit_super_lhs_key(&up.argument);
            }
            self.write(", ");
            self.write(&recv);
            self.write("), ");
            if is_prefix {
                self.write(op_str);
                self.write(&val_temp);
            } else {
                self.write(&val_temp);
                self.write(op_str);
                self.write(", ");
                self.write(&val_temp);
            }
            self.write("), ");
            self.write(&recv);
            self.write(")");
        }
        if in_field {
            self.output.push('\n');
            self.out_line += 1;
            self.out_col = 0;
            self.at_line_start = true;
            if let Some(pos) = var_insert_pos {
                if !self.temp_var_names.is_empty() {
                    let indent_str = "    ".repeat(self.indent as usize);
                    let var_decl =
                        format!("{}var {};\n", indent_str, self.temp_var_names.join(", "));
                    self.output.insert_str(pos, &var_decl);
                }
            }
            self.indent -= 1;
            self.write("})()");
        }
        if let Some((prev_names, prev_counter)) = saved_temps {
            self.temp_var_names = prev_names;
            self.temp_var_counter = prev_counter;
        }
        true
    }

    /// Emit the key for a super member/element access LHS.
    fn emit_super_lhs_key(&mut self, lhs: &Expr) {
        match &lhs.kind {
            ExprKind::Member(mem) => {
                self.write("\"");
                self.write(&mem.property);
                self.write("\"");
            }
            ExprKind::ElemAccess(ea) => {
                self.emit_expr(&ea.index);
            }
            ExprKind::Paren(inner) => self.emit_super_lhs_key(inner),
            _ => self.write("\"\""),
        }
    }
}

fn type_args_need_jsdoc_recovery(type_args: &[TypeNode]) -> bool {
    type_args.iter().any(type_node_needs_jsdoc_recovery)
}

fn type_node_needs_jsdoc_recovery(ty: &TypeNode) -> bool {
    match &ty.kind {
        TypeNodeKind::JSDocNullable(_) => true,
        TypeNodeKind::Reference(type_ref) => type_ref
            .type_args
            .as_ref()
            .is_some_and(|args| args.iter().any(type_node_needs_jsdoc_recovery)),
        TypeNodeKind::Array(inner)
        | TypeNodeKind::Paren(inner)
        | TypeNodeKind::Rest(inner)
        | TypeNodeKind::Keyof(inner)
        | TypeNodeKind::Unique(inner)
        | TypeNodeKind::Readonly(inner)
        | TypeNodeKind::TypeOperator(_, inner) => type_node_needs_jsdoc_recovery(inner),
        TypeNodeKind::Tuple(elements) => elements
            .iter()
            .any(|elem| type_node_needs_jsdoc_recovery(&elem.type_node)),
        TypeNodeKind::Union(types) | TypeNodeKind::Intersection(types) => {
            types.iter().any(type_node_needs_jsdoc_recovery)
        }
        TypeNodeKind::IndexedAccess(obj, idx) => {
            type_node_needs_jsdoc_recovery(obj) || type_node_needs_jsdoc_recovery(idx)
        }
        _ => false,
    }
}

/// Returns true when `expr` is a multi-line `Paren(Comma(…))` or bare
/// multi-line `Comma(…)` — these need structured emit (with newlines and
/// indentation) rather than source-copy, which would collapse or miss indent.
fn is_multiline_paren_comma(expr: &Expr, source: &str) -> bool {
    let check_span = |sp: Span| -> bool {
        let s = sp.start as usize;
        let e = sp.end as usize;
        s < e && e <= source.len() && source[s..e].contains('\n')
    };
    match &expr.kind {
        ExprKind::Paren(inner) if matches!(&inner.kind, ExprKind::Comma(_)) => {
            check_span(inner.span)
        }
        ExprKind::Comma(_) => check_span(expr.span),
        _ => false,
    }
}

/// Returns true when `expr` is a `New` whose callee is an error-recovery
/// placeholder.  The parser may assign a span to the `New` that overlaps
/// the closing paren of an enclosing expression, so source-copy would
/// include that `)` and produce invalid output.
fn new_has_error_callee(expr: &Expr) -> bool {
    if let ExprKind::New(new_expr) = &expr.kind {
        matches!(&new_expr.callee.kind, ExprKind::Ident(n) if n == "<error>")
    } else {
        false
    }
}

fn multiline_array_elision_tail_comment(source: &str, span: Span) -> Option<(String, bool)> {
    let start = span.start as usize;
    let end = span.end as usize;
    if start >= end || end > source.len() {
        return None;
    }
    let text = &source[start..end];
    let close_idx = text.rfind(']')?;
    let inner = &text[..close_idx];
    let last_comma = inner.rfind(',')?;
    let after_comma = &inner[last_comma + 1..];
    let comment_start = after_comma.find("/*")?;
    let tail = &after_comma[comment_start..];
    let comment_end = tail.find("*/")?;
    let comment = tail[..comment_end + 2].trim().to_string();
    if comment.is_empty() {
        return None;
    }
    let on_new_line =
        after_comma[..comment_start].contains('\n') || after_comma[..comment_start].contains('\r');
    Some((comment, on_new_line))
}

pub(crate) fn array_literal_top_level_comma_count(source: &str, span: Span) -> usize {
    let start = span.start as usize;
    let end = span.end as usize;
    if start >= end || end > source.len() {
        return 0;
    }
    let text = &source[start..end];
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
    let mut i = 0usize;
    let mut count = 0usize;
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut brace_depth = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        // Skip strings (single/double quotes) and template literals.
        if b == b'\'' || b == b'"' || b == b'`' {
            let quote = b;
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i += 2;
                    continue;
                }
                if bytes[i] == quote {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        // Skip comments.
        if b == b'/' && i + 1 < bytes.len() {
            if bytes[i + 1] == b'/' {
                i += 2;
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            if bytes[i + 1] == b'*' {
                i += 2;
                while i + 1 < bytes.len() {
                    if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                        i += 2;
                        break;
                    }
                    i += 1;
                }
                continue;
            }
        }
        match b {
            b'(' => paren_depth += 1,
            b')' => paren_depth = paren_depth.saturating_sub(1),
            b'[' => bracket_depth += 1,
            b']' => bracket_depth = bracket_depth.saturating_sub(1),
            b'{' => brace_depth += 1,
            b'}' => brace_depth = brace_depth.saturating_sub(1),
            b',' if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 => count += 1,
            _ => {}
        }
        i += 1;
    }
    count
}

/// Collapse multiline JSX expression containers `{ ... }` to single lines
/// when they contain no comments. Scans for `{` followed by newlines and
/// collapses to `{content}` with normalized whitespace.
fn collapse_jsx_expression_containers(text: &str) -> Option<String> {
    if !text.contains('\n') {
        return None;
    }
    let bytes = text.as_bytes();
    let mut result = Vec::with_capacity(bytes.len());
    let mut i = 0;
    let mut changed = false;
    while i < bytes.len() {
        // Look for `{` that starts a multiline expression container
        // (inside JSX children, after `>` or at start)
        if bytes[i] == b'{' {
            // Only collapse expression containers INSIDE JSX elements
            // (after `>` or at start of element children area).
            // Skip regular JS objects like `{ a: 1 }`.
            let prev_non_ws = result
                .iter()
                .rev()
                .find(|&&b| b != b' ' && b != b'\t' && b != b'\n' && b != b'\r');
            // Only collapse in JSX context: after `>` (JSX tag),
            // `}` (previous expression container), `=` (attribute value),
            // or at start. Skip regular JS objects.
            let after_jsx_tag = prev_non_ws.is_none()
                || prev_non_ws.is_some_and(|&b| b == b'>' || b == b'}' || b == b'=');
            if !after_jsx_tag {
                result.push(bytes[i]);
                i += 1;
                continue;
            }
            // Check if there's a newline before the matching `}`
            let mut depth = 1;
            let mut j = i + 1;
            let mut has_newline = false;
            let mut has_comment = false;
            while j < bytes.len() && depth > 0 {
                match bytes[j] {
                    b'{' => depth += 1,
                    b'}' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    b'\n' => has_newline = true,
                    b'/' if j + 1 < bytes.len()
                        && (bytes[j + 1] == b'/' || bytes[j + 1] == b'*') =>
                    {
                        has_comment = true;
                    }
                    _ => {}
                }
                j += 1;
            }
            if has_newline && !has_comment && depth == 0 && j < bytes.len() {
                // Collapse the content between { and }
                let inner = &text[i + 1..j];
                let has_blank_line = inner.lines().skip(1).any(|line| line.trim().is_empty());
                let is_multiline_attribute_value = prev_non_ws == Some(&b'=')
                    && inner.lines().filter(|line| !line.trim().is_empty()).count() > 1;
                // Skip collapsing if the content contains JSX closing tags
                // (`</`) — this indicates the `{...}` spans across JSX
                // boundaries and isn't a pure expression container.
                if has_blank_line
                    || is_multiline_attribute_value
                    || inner.contains("</") && !inner.contains("=>")
                {
                    result.push(bytes[i]);
                    i += 1;
                    continue;
                }
                let lines: Vec<&str> = inner.lines().collect();
                let collapsed: String = lines
                    .iter()
                    .map(|l| l.trim())
                    .filter(|l| !l.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ");
                // Also strip spaces after ( and before )
                let collapsed = collapsed.replace("( ", "(").replace(" )", ")");
                result.push(b'{');
                result.extend_from_slice(collapsed.as_bytes());
                result.push(b'}');
                i = j + 1;
                changed = true;
                continue;
            }
        }
        result.push(bytes[i]);
        i += 1;
    }
    if changed {
        Some(String::from_utf8(result).unwrap_or_else(|_| text.to_string()))
    } else {
        None
    }
}

fn compact_multiline_jsx_self_closing_layout(text: &str) -> String {
    if !text.contains('\n') && !text.contains('\r') {
        return text.to_string();
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    let mut brace_depth = 0usize;
    let mut in_string: Option<u8> = None;
    while i < bytes.len() {
        let ch = bytes[i];
        if let Some(quote) = in_string {
            out.push(ch);
            if ch == b'\\' && i + 1 < bytes.len() {
                i += 1;
                out.push(bytes[i]);
                i += 1;
                continue;
            }
            if ch == quote {
                in_string = None;
            }
            i += 1;
            continue;
        }
        if brace_depth > 0 {
            match ch {
                b'\'' | b'"' | b'`' => {
                    in_string = Some(ch);
                }
                b'{' => {
                    brace_depth += 1;
                }
                b'}' => {
                    brace_depth = brace_depth.saturating_sub(1);
                }
                _ => {}
            }
            out.push(ch);
            i += 1;
            continue;
        }
        // Track strings at the top level too — multiline string attribute
        // values like `value="\nfoo\n"` must preserve their newlines.
        if matches!(ch, b'\'' | b'"' | b'`') {
            in_string = Some(ch);
            out.push(ch);
            i += 1;
            continue;
        }
        if ch == b'{' {
            brace_depth = 1;
            out.push(ch);
            i += 1;
            continue;
        }
        if matches!(ch, b' ' | b'\t' | b'\n' | b'\r') {
            while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\r') {
                i += 1;
            }
            let prev = out.last().copied();
            let next = bytes.get(i).copied();
            let should_emit_space = !matches!(prev, None | Some(b' ') | Some(b'<') | Some(b'='))
                && !matches!(
                    next,
                    None | Some(b'>')
                        | Some(b'/')
                        | Some(b';')
                        | Some(b',')
                        | Some(b')')
                        | Some(b'=')
                );
            if should_emit_space {
                out.push(b' ');
            }
            continue;
        }
        out.push(ch);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_string())
}

/// After the outer opening-tag whitespace is compacted, retain meaningful
/// line breaks within multiline attribute expressions. The first expression
/// line follows `{` directly, binary continuation lines use two indent units,
/// and a line comment (plus the closing brace it forces onto the next line)
/// uses one indent unit, matching TypeScript's JSX-preserve printer.
fn normalize_multiline_jsx_attribute_expr_layout(text: &str) -> String {
    if !text.contains('\n') {
        return text.to_string();
    }
    let mut output: Vec<String> = Vec::new();
    for raw_line in text.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if output.is_empty() {
            output.push(trimmed.to_string());
            continue;
        }
        if output
            .last()
            .is_some_and(|line| line.trim_end().ends_with("={"))
        {
            output.last_mut().unwrap().push_str(trimmed);
            continue;
        }
        if trimmed.starts_with('}') {
            let previous_is_line_comment = output
                .last()
                .is_some_and(|line| line.trim_start().starts_with("//"));
            if previous_is_line_comment {
                output.push(format!("    {trimmed}"));
            } else {
                output.last_mut().unwrap().push_str(trimmed);
            }
        } else if trimmed.starts_with("//") {
            output.push(format!("    {trimmed}"));
        } else {
            output.push(format!("        {trimmed}"));
        }
    }
    output.join("\n")
}

fn normalize_jsx_opening_block_comment_layout(text: &str) -> String {
    if !text.contains('\n') || !text.contains("/*") {
        return text.to_string();
    }
    let lines: Vec<&str> = text.lines().collect();
    let Some(comment_index) = lines.iter().position(|line| line.contains("/*")) else {
        return text.to_string();
    };
    let comment_indent = lines[comment_index].len() - lines[comment_index].trim_start().len();
    let mut output = Vec::with_capacity(lines.len());
    for (index, line) in lines.iter().enumerate() {
        if index == 0 {
            output.push(format!("{} ", line.trim_end()));
        } else if index < comment_index {
            if !line.trim().is_empty() {
                output.push(line.trim().to_string());
            }
        } else if index == comment_index {
            output.push(line.trim_start().to_string());
        } else if line.contains("*/") && index > comment_index {
            let leading = line.len() - line.trim_start().len();
            output.push(format!(
                "{}{}",
                " ".repeat(leading.saturating_sub(comment_indent)),
                line.trim_start()
            ));
        } else {
            output.push(line.trim_start().to_string());
        }
    }
    output.join("\n")
}

fn normalize_jsx_self_closing_line_comment_layout(text: &str) -> String {
    if !text.contains('\n') || !text.contains("//") {
        return text.to_string();
    }
    let mut output: Vec<String> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if output.is_empty() {
            output.push(line.trim_end().to_string());
        } else if trimmed == "/>" {
            output.last_mut().unwrap().push_str("/>");
        } else {
            output.push(format!(" {trimmed}"));
        }
    }
    output.join("\n")
}

fn normalize_nested_jsx_self_closing_line_comment_layout(text: &str) -> String {
    if !text.contains("//") || !text.contains('\n') {
        return text.to_string();
    }
    let lines: Vec<&str> = text.lines().collect();
    let mut output: Vec<String> = Vec::with_capacity(lines.len());
    let mut index = 0usize;
    while index < lines.len() {
        let line = lines[index];
        if line.contains('<') && line.contains("//") && !line.trim_start().starts_with("//") {
            output.push(line.to_string());
            index += 1;
            let mut collected_attribute = false;
            while index < lines.len() {
                let trimmed = lines[index].trim();
                if trimmed == "/>" {
                    if collected_attribute {
                        output.last_mut().unwrap().push_str("/>");
                    } else {
                        output.push(" />".to_string());
                    }
                    index += 1;
                    break;
                }
                if !trimmed.is_empty() {
                    output.push(format!(" {trimmed}"));
                    collected_attribute = true;
                }
                index += 1;
            }
            continue;
        }
        output.push(line.to_string());
        index += 1;
    }
    output.join("\n")
}

/// Find the position of the opening `>` in a JSX element tag, skipping
/// over `>` inside `{...}` brace expressions and string attribute values.
fn find_jsx_opening_gt(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut brace_depth = 0usize;
    let mut in_string: Option<u8> = None;
    let mut i = 0usize;
    while i < bytes.len() {
        let ch = bytes[i];
        if let Some(quote) = in_string {
            if ch == b'\\' && i + 1 < bytes.len() {
                i += 2;
                continue;
            }
            if ch == quote {
                in_string = None;
            }
            i += 1;
            continue;
        }
        match ch {
            b'\'' | b'"' | b'`' => {
                in_string = Some(ch);
            }
            b'{' => {
                brace_depth += 1;
            }
            b'}' => {
                brace_depth = brace_depth.saturating_sub(1);
            }
            b'>' if brace_depth == 0 => {
                return Some(i);
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Returns true if `expr` is a chain of non-optional Member accesses whose
/// innermost object is an optional Member (i.e., an optional chain root).
/// Used to detect patterns like `a?.b.c.d` where the outer `.c.d` are
/// continuations of the optional chain started by `a?.b`.
fn is_oc_member_chain(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Member(mem) if mem.optional => true,
        ExprKind::Member(mem) => is_oc_member_chain(&mem.object),
        ExprKind::Call(call) if call.optional => true,
        ExprKind::ElemAccess(ea) if ea.optional => true,
        // Walk through type-stripping wrappers (NonNull!, as T, satisfies T)
        ExprKind::NonNull(inner) | ExprKind::Paren(inner) => is_oc_member_chain(inner),
        ExprKind::TypeAssertion(ta) => is_oc_member_chain(&ta.expr),
        ExprKind::As(a) => is_oc_member_chain(&a.expr),
        ExprKind::Satisfies(s) => is_oc_member_chain(&s.expr),
        _ => false,
    }
}

/// Returns true if `expr` is part of an optional chain (contains any optional=true
/// in its chain of Member/Call/ElemAccess/type-stripping links).
pub(crate) fn is_oc_chain(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Member(mem) if mem.optional => true,
        ExprKind::Member(mem) => is_oc_chain(&mem.object),
        ExprKind::Call(call) if call.optional => true,
        ExprKind::Call(call) => is_oc_chain(&call.callee),
        ExprKind::ElemAccess(ea) if ea.optional => true,
        ExprKind::ElemAccess(ea) => is_oc_chain(&ea.object),
        ExprKind::NonNull(inner) | ExprKind::Paren(inner) => is_oc_chain(inner),
        ExprKind::TypeAssertion(ta) => is_oc_chain(&ta.expr),
        ExprKind::As(a) => is_oc_chain(&a.expr),
        ExprKind::Satisfies(s) => is_oc_chain(&s.expr),
        _ => false,
    }
}

impl<'a> Emitter<'a> {
    fn collect_recovered_jsx_self_close_replacements(
        &self,
        children: &[JsxChild],
        ancestor_name: Option<&str>,
        replacements: &mut Vec<(usize, usize, String)>,
    ) {
        for (index, child) in children.iter().enumerate() {
            match child {
                JsxChild::Element(expr) => match &expr.kind {
                    ExprKind::JsxSelfClosing(element) => {
                        let start = expr.span.start as usize;
                        let end = (expr.span.end as usize).min(self.source.len());
                        if start >= end {
                            continue;
                        }
                        let raw = &self.source[start..end];
                        let trimmed = raw.trim_end();
                        if trimmed.ends_with("/>") {
                            continue;
                        }
                        let no_attrs = element.attributes.is_empty();
                        if raw.contains('\n') && !trimmed.ends_with('/') {
                            let mut compact_source = raw.to_string();
                            if element.type_args.is_some() {
                                let name_end = (element.name.span.end as usize)
                                    .saturating_sub(start)
                                    .min(compact_source.len());
                                if let Some(type_end) = compact_source[name_end..].find('>') {
                                    compact_source
                                        .replace_range(name_end..name_end + type_end + 1, "");
                                }
                            }
                            replacements.retain(|(replace_start, replace_end, _)| {
                                !(*replace_start >= start
                                    && *replace_end <= end
                                    && replace_start != replace_end)
                            });
                            let mut compact = compact_source
                                .split_whitespace()
                                .collect::<Vec<_>>()
                                .join(" ");
                            if compact.starts_with("< ") {
                                compact.remove(1);
                            }
                            compact.push_str(if no_attrs { " />" } else { "/>" });
                            replacements.push((start, end, compact));
                            continue;
                        }
                        let insert = start + trimmed.len();
                        if trimmed.ends_with('/') {
                            if no_attrs
                                && trimmed
                                    .as_bytes()
                                    .get(trimmed.len().saturating_sub(2))
                                    .is_some_and(|byte| *byte != b' ')
                            {
                                replacements.push((insert - 1, insert - 1, " ".to_string()));
                            }
                            replacements.push((insert, insert, ">".to_string()));
                        } else {
                            replacements.push((
                                insert,
                                insert,
                                if no_attrs { " />" } else { "/>" }.to_string(),
                            ));
                        }
                        // With a bare missing name or a simple `<Tag` (no
                        // type arguments/attributes/slash), TypeScript uses a
                        // following self-closing tag only as recovery context
                        // and does not retain it as a sibling.
                        if element.type_args.is_none()
                            && element.attributes.is_empty()
                            && !trimmed.ends_with('/')
                        {
                            if let Some(JsxChild::Element(next)) = children.get(index + 1) {
                                let next_start = next.span.start as usize;
                                let next_end = (next.span.end as usize).min(self.source.len());
                                if next_start < next_end
                                    && self.source[next_start..next_end].trim_end().ends_with("/>")
                                {
                                    replacements.push((end, next_end, String::new()));
                                }
                            }
                        }
                    }
                    ExprKind::JsxElement(element) => {
                        if let Some(position) = self.jsx_missing_close_before_parent(expr) {
                            replacements.push((position, position, "</>".to_string()));
                        } else if let ExprKind::Ident(open_name) = &element.name.kind {
                            let start = expr.span.start as usize;
                            let end = (expr.span.end as usize).min(self.source.len());
                            if start < end {
                                let raw = &self.source[start..end];
                                if let Some(close_rel) = raw.rfind("</") {
                                    let close_start = start + close_rel;
                                    let close_tail = &raw[close_rel + 2..];
                                    let close_name_end = close_tail
                                        .find(|ch: char| ch == '>' || ch.is_whitespace())
                                        .unwrap_or(close_tail.len());
                                    let close_name = &close_tail[..close_name_end];
                                    if !close_name.is_empty()
                                        && close_name != open_name.as_str()
                                        && ancestor_name == Some(close_name)
                                    {
                                        replacements.push((
                                            close_start,
                                            close_start,
                                            "</>".to_string(),
                                        ));
                                    }
                                }
                            }
                        }
                        self.collect_recovered_jsx_self_close_replacements(
                            &element.children,
                            match &element.name.kind {
                                ExprKind::Ident(name) => Some(name.as_str()),
                                _ => None,
                            },
                            replacements,
                        );
                    }
                    _ => {}
                },
                JsxChild::Fragment(fragment) => self.collect_recovered_jsx_self_close_replacements(
                    &fragment.children,
                    ancestor_name,
                    replacements,
                ),
                _ => {}
            }
        }
    }

    fn count_unclosed_jsx_elements(&self, expr: &Expr) -> usize {
        fn child_count(emitter: &Emitter<'_>, child: &JsxChild) -> usize {
            match child {
                JsxChild::Element(child) => emitter.count_unclosed_jsx_elements(child),
                JsxChild::Fragment(fragment) => fragment
                    .children
                    .iter()
                    .map(|child| child_count(emitter, child))
                    .sum(),
                _ => 0,
            }
        }
        let ExprKind::JsxElement(element) = &expr.kind else {
            return 0;
        };
        // Recovery records ownership in the tree. Counting source spellings
        // confuses qualified names and nested elements with the same name.
        usize::from(
            element.closing_tag_missing
                && self.jsx_missing_close_before_parent(expr).is_none()
                // An orphan closing delimiter can recover as an element,
                // but it did not open a tag that needs a synthetic close.
                && !self.source[expr.span.start as usize..].starts_with("</"),
        ) + element
            .children
            .iter()
            .map(|child| child_count(self, child))
            .sum::<usize>()
    }

    fn jsx_missing_close_before_parent(&self, expr: &Expr) -> Option<usize> {
        let ExprKind::JsxElement(element) = &expr.kind else {
            return None;
        };
        if !element.closing_tag_missing {
            return None;
        }
        let end = expr.span.end as usize;
        let tail = self.source.get(end..)?;
        let closing = tail.trim_start();
        // The parser leaves a matching parent close outside the child's span.
        // Insert at that boundary, preserving intervening JSX text whitespace.
        closing
            .starts_with("</")
            .then_some(end + tail.len() - closing.len())
    }
}
