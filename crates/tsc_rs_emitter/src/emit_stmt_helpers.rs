//! Free helper functions for statement emission.

use super::*;

/// Check if a line ends with `{`, possibly followed by a trailing comment.
/// Matches: `... {`, `... { // comment`, `... { /* comment */`
pub(crate) fn line_ends_with_open_brace(line: &str) -> bool {
    let trimmed = line.trim_end();
    if trimmed.ends_with('{') {
        return true;
    }
    // Check for `{ // comment` or `{ /* comment */` at end
    if let Some(brace_pos) = trimmed.rfind('{') {
        let after = trimmed[brace_pos + 1..].trim_start();
        after.starts_with("//") || (after.starts_with("/*") && after.ends_with("*/"))
    } else {
        false
    }
}

/// Check if a line starts with a binary operator token followed by a space
/// or end-of-line, indicating the operator is a continuation of a previous
/// expression (the operator was placed at the start of a new line).
pub(crate) fn line_starts_with_binary_operator(line: &str) -> bool {
    let trimmed = line.trim_start();
    // Check multi-char operators first (to avoid matching `>>` as `>` + `>`)
    for op in &[
        ">>>",
        ">>",
        "<<",
        "===",
        "!==",
        "==",
        "!=",
        "<=",
        ">=",
        "&&",
        "||",
        "??",
        "instanceof ",
        "in ",
    ] {
        if trimmed.starts_with(op) {
            return true;
        }
    }
    // Single-char operators: must be followed by a space or end of line
    for op in &['+', '-', '*', '/', '%', '&', '|', '^', '<', '>'] {
        if trimmed.starts_with(*op) {
            let rest = &trimmed[op.len_utf8()..];
            if rest.is_empty() || rest.starts_with(' ') {
                // Exclude cases that aren't binary operators:
                // `--` (decrement), `++` (increment), `//` (comment), `/*` (comment)
                if (*op == '-' && rest.starts_with('-'))
                    || (*op == '+' && rest.starts_with('+'))
                    || (*op == '/' && (rest.starts_with('/') || rest.starts_with('*')))
                {
                    continue;
                }
                return true;
            }
        }
    }
    false
}

/// Check if a line is just a binary operator token (optionally followed
/// by comments), used for RHS continuation indentation.
pub(crate) fn line_is_binary_operator_only(line: &str) -> bool {
    let code = strip_trailing_comments(line);
    matches!(
        code,
        "+" | "-"
            | "*"
            | "/"
            | "%"
            | "&"
            | "|"
            | "^"
            | "&&"
            | "||"
            | "??"
            | "=="
            | "!="
            | "==="
            | "!=="
            | "<"
            | "<="
            | ">"
            | ">="
            | "<<"
            | ">>"
            | ">>>"
            | "in"
            | "instanceof"
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TrailingContinuationOp {
    Binary,
    TernaryQuestion,
    TernaryColon,
}

/// Detect continuation-driving trailing operators on a line (ignoring trailing
/// comments). This is used to stabilize indentation in source-copied fallback
/// statement emit.
pub(crate) fn line_trailing_continuation_op(line: &str) -> Option<TrailingContinuationOp> {
    let code = strip_trailing_comments(line);
    if code.ends_with("&&") || code.ends_with("||") || code.ends_with("??") {
        return Some(TrailingContinuationOp::Binary);
    }
    if code.ends_with('?') {
        return Some(TrailingContinuationOp::TernaryQuestion);
    }
    if code.ends_with(':') {
        return Some(TrailingContinuationOp::TernaryColon);
    }
    // Member access dot: `obj.\n    property` gets continuation indent.
    // Exclude `...` (spread operator) which ends with `.` but is not a member access.
    // Also exclude comment lines (lines starting with `*`, `//`, or `/*`)
    // where `.` is just a sentence-ending period.
    if code.ends_with('.')
        && !code.ends_with("...")
        && !code.trim_start().starts_with('*')
        && !code.trim_start().starts_with("//")
        && !code.trim_start().starts_with("/*")
    {
        return Some(TrailingContinuationOp::Binary);
    }
    None
}

fn strip_trailing_comments(line: &str) -> &str {
    let mut code = line.trim_end();
    if code.is_empty() {
        return code;
    }
    if let Some(line_comment_pos) = code.find("//") {
        code = code[..line_comment_pos].trim_end();
    }
    loop {
        if !code.ends_with("*/") {
            break;
        }
        if let Some(block_start) = code.rfind("/*") {
            code = code[..block_start].trim_end();
        } else {
            break;
        }
    }
    code.trim_end()
}

/// Check if an import-equals RHS expression tree contains `<error>` tokens
/// from parser error recovery (e.g. `import lol = Test5.Foo.` with trailing dot,
/// or `import n = 5;` where the RHS is not a valid identifier chain).
pub(crate) fn import_equals_rhs_has_error(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Ident(name) => name == "<error>",
        ExprKind::Member(mem) => {
            mem.property == "<error>" || import_equals_rhs_has_error(&mem.object)
        }
        _ => false,
    }
}

/// Check if an expression tree contains an `<error>` token from parser error
/// recovery — either a Member with `<error>` property (e.g. `Foo.` with trailing
/// dot) or an `<error>` identifier (e.g. `x = ` with missing RHS).
/// Used to detect when a semicolon should go on its own line in source-copy emit.
pub(crate) fn expr_has_error_member(expr: &Expr) -> bool {
    let mut stack = vec![expr];
    while let Some(expr) = stack.pop() {
        match &expr.kind {
            ExprKind::Ident(name) if name == "<error>" => return true,
            ExprKind::Member(mem) => {
                if mem.property == "<error>" {
                    return true;
                }
                stack.push(&mem.object);
            }
            ExprKind::Call(call) => {
                for arg in call.args.iter().rev() {
                    stack.push(arg);
                }
                stack.push(&call.callee);
            }
            ExprKind::ElemAccess(ea) => {
                stack.push(&ea.index);
                stack.push(&ea.object);
            }
            ExprKind::Binary(b) => {
                stack.push(&b.right);
                stack.push(&b.left);
            }
            ExprKind::Assign(a) => {
                stack.push(&a.right);
                stack.push(&a.left);
            }
            ExprKind::Unary(u) => stack.push(&u.argument),
            ExprKind::Update(u) => stack.push(&u.argument),
            ExprKind::Paren(inner) => stack.push(inner),
            ExprKind::Comma(exprs) => {
                for inner in exprs.iter().rev() {
                    stack.push(inner);
                }
            }
            ExprKind::Cond(c) => {
                stack.push(&c.alternate);
                stack.push(&c.consequent);
                stack.push(&c.test);
            }
            ExprKind::Spread(inner) => stack.push(inner),
            ExprKind::NonNull(inner) => stack.push(inner),
            ExprKind::As(a) => stack.push(&a.expr),
            ExprKind::TypeAssertion(ta) => stack.push(&ta.expr),
            ExprKind::Satisfies(s) => stack.push(&s.expr),
            ExprKind::Instantiation(inst) => stack.push(&inst.expr),
            ExprKind::Await(inner)
            | ExprKind::Typeof(inner)
            | ExprKind::Void(inner)
            | ExprKind::Delete(inner) => stack.push(inner),
            ExprKind::TaggedTemplate(tt) => stack.push(&tt.tag),
            ExprKind::Yield(_, Some(inner)) => stack.push(inner),
            _ => {}
        }
    }
    false
}

/// Check if a statement contains an expression with an `<error>` member.
pub(crate) fn stmt_has_error_member(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::Expr(expr) => expr_has_error_member(expr),
        StmtKind::Var(var) => var
            .declarations
            .iter()
            .any(|d| d.init.as_ref().map_or(false, |e| expr_has_error_member(e))),
        StmtKind::Return(Some(expr)) | StmtKind::Throw(expr) | StmtKind::ExportAssign(expr) => {
            expr_has_error_member(expr)
        }
        _ => false,
    }
}

/// Walk down through Call/Member/ElemAccess/TaggedTemplate to find the
/// leftmost (base) expression of a chain.
pub(crate) fn leftmost_expr(expr: &Expr) -> &Expr {
    match &expr.kind {
        ExprKind::Call(call) => leftmost_expr(&call.callee),
        ExprKind::Member(mem) => leftmost_expr(&mem.object),
        ExprKind::ElemAccess(ea) => leftmost_expr(&ea.object),
        ExprKind::TaggedTemplate(tt) => leftmost_expr(&tt.tag),
        _ => expr,
    }
}

/// Check if the expression chain contains a Member or ElemAccess node
/// (not just Call nodes).  Used to distinguish `(f as any)()` (IIFE, no wrap)
/// from `(f as any)().foo` (member access chain, wrap).
pub(crate) fn chain_has_member_or_elem(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Member(_) | ExprKind::ElemAccess(_) => true,
        ExprKind::Call(call) => chain_has_member_or_elem(&call.callee),
        ExprKind::TaggedTemplate(tt) => chain_has_member_or_elem(&tt.tag),
        _ => false,
    }
}

/// Check if an expression statement needs TypeScript-style outer paren wrapping.
/// This is the case when:
///   `ExprStmt(Member/ElemAccess(...Paren(TypeAssertion(ObjectLit/FnExpr/ClassExpr))...))`
/// TypeScript wraps the ENTIRE expression in parens: `({}.toString())` not `({}).toString()`.
/// Does NOT trigger for direct calls like `(function() {} as any)()` (IIFE pattern).
pub(crate) fn expr_stmt_needs_outer_paren_wrap(expr: &Expr) -> bool {
    // Must have a member/element access in the chain (not just calls).
    if !chain_has_member_or_elem(expr) {
        return false;
    }
    let base = leftmost_expr(expr);
    // Must not be the expression itself (must have member/call chain on top).
    if std::ptr::eq(base, expr) {
        return false;
    }
    if let ExprKind::Paren(inner) = &base.kind {
        // Only trigger for type assertion wrappers (paren must directly wrap a type layer).
        let is_type_wrapper = matches!(
            inner.kind,
            ExprKind::TypeAssertion(_)
                | ExprKind::As(_)
                | ExprKind::Satisfies(_)
                | ExprKind::NonNull(_)
                | ExprKind::Instantiation(_)
        );
        if !is_type_wrapper {
            return false;
        }
        let stripped = strip_type_layers(inner);
        matches!(
            stripped.kind,
            ExprKind::ObjectLit(_) | ExprKind::FnExpr(_) | ExprKind::ClassExpr(_)
        )
    } else {
        false
    }
}

/// Returns true when an expression statement needs disambiguation parens because
/// stripping a type assertion/`as` cast exposes an expression that starts with
/// `{`, `function`, or `class`, which would be ambiguous without parens.
/// E.g. `<any>{};` → `({});`, `<T>function(){}()` → `(function(){})();`
pub(crate) fn expr_stmt_needs_disambiguation_parens(expr: &Expr) -> bool {
    // Walk through the expression to find the outermost type layer.
    // If the expr is already wrapped in Paren, TypeScript keeps the parens,
    // so no disambiguation is needed.
    let stripped = strip_expr_type_layers(expr);
    // If nothing was stripped, no disambiguation needed.
    if std::ptr::eq(stripped, expr) {
        return false;
    }
    // After stripping, check what the leftmost expression is.
    // Note: TypeScript does NOT add disambiguation parens for class expressions
    // at the start of expression statements (this is known to emit invalid JS).
    let leftmost = leftmost_expr_for_disambig(stripped);
    matches!(leftmost.kind, ExprKind::ObjectLit(_) | ExprKind::FnExpr(_))
}

/// Returns true when disambiguation parens should wrap only the leftmost
/// function expression (via a flag), not the whole expression statement.
/// This is true when a FnExpr is inside a call/member chain:
/// `<T>function(){}()` → `(function(){})()` (wrap only fn, not call)
/// Object literals still use whole-expression wrapping:
/// `<T>{}.foo()` → `({}.foo())`
pub(crate) fn disambig_needs_deep_wrap(expr: &Expr) -> bool {
    let stripped = strip_expr_type_layers(expr);
    let leftmost = leftmost_expr_for_disambig(stripped);
    // Only deep-wrap for function expressions inside chains.
    // Object literals always get full outer wrapping.
    !std::ptr::eq(stripped, leftmost) && matches!(leftmost.kind, ExprKind::FnExpr(_))
}

/// Strip outer type assertion / as / satisfies / non-null layers from an expression.
/// Stops at Paren (since Paren already provides disambiguation).
fn strip_expr_type_layers(expr: &Expr) -> &Expr {
    match &expr.kind {
        ExprKind::NonNull(inner) => strip_expr_type_layers(inner),
        ExprKind::TypeAssertion(ta) => strip_expr_type_layers(&ta.expr),
        ExprKind::As(a) => strip_expr_type_layers(&a.expr),
        ExprKind::Satisfies(s) => strip_expr_type_layers(&s.expr),
        ExprKind::Instantiation(inst) => strip_expr_type_layers(&inst.expr),
        _ => expr,
    }
}

/// Get the leftmost expression for disambiguation purposes.
/// Traverses through call, member, elem access, assignment, etc.
fn leftmost_expr_for_disambig(expr: &Expr) -> &Expr {
    match &expr.kind {
        ExprKind::Call(call) => leftmost_expr_for_disambig(&call.callee),
        ExprKind::Member(mem) => leftmost_expr_for_disambig(&mem.object),
        ExprKind::ElemAccess(ea) => leftmost_expr_for_disambig(&ea.object),
        ExprKind::TaggedTemplate(tt) => leftmost_expr_for_disambig(&tt.tag),
        ExprKind::Assign(a) => leftmost_expr_for_disambig(&a.left),
        ExprKind::NonNull(inner) => leftmost_expr_for_disambig(inner),
        ExprKind::TypeAssertion(ta) => leftmost_expr_for_disambig(&ta.expr),
        ExprKind::As(a) => leftmost_expr_for_disambig(&a.expr),
        ExprKind::Satisfies(s) => leftmost_expr_for_disambig(&s.expr),
        ExprKind::Instantiation(inst) => leftmost_expr_for_disambig(&inst.expr),
        _ => expr,
    }
}

/// Returns true when a statement is an `ExprStmt` containing a multi-line
/// `Paren(Comma(…))`.  Source-copy would collapse the lines; structured emit
/// with the enhanced Comma handler preserves newlines + indentation.
pub(crate) fn has_multiline_paren_comma(stmt: &Stmt, source: &str) -> bool {
    if let StmtKind::Expr(expr) = &stmt.kind {
        let check = |sp: Span| -> bool {
            let s = sp.start as usize;
            let e = sp.end as usize;
            s < e && e <= source.len() && source[s..e].contains('\n')
        };
        match &expr.kind {
            ExprKind::Paren(inner) if matches!(&inner.kind, ExprKind::Comma(_)) => {
                return check(inner.span);
            }
            ExprKind::Comma(_) => {
                return check(expr.span);
            }
            _ => {}
        }
    }
    false
}

/// Check if a template element's raw text contains an invalid escape sequence.
/// Invalid escapes cause `cooked` to be `undefined` in ES2018+, but are syntax
/// errors in ES2015–ES2017 tagged templates.
pub(crate) fn template_element_has_invalid_escape(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        if bytes[i] == b'\\' {
            i += 1;
            if i >= len {
                return true; // trailing backslash
            }
            match bytes[i] {
                // Standard escapes — always valid
                b'\\' | b'\'' | b'"' | b'`' | b'n' | b'r' | b't' | b'v' | b'f' | b'b' => {
                    i += 1;
                }
                // Line continuations — valid
                b'\n' | b'\r' => {
                    i += 1;
                    if i < len && bytes[i - 1] == b'\r' && bytes[i] == b'\n' {
                        i += 1;
                    }
                }
                // Unicode escape: \uXXXX or \u{X...X}
                b'u' => {
                    i += 1;
                    if i < len && bytes[i] == b'{' {
                        i += 1;
                        let start = i;
                        while i < len && bytes[i] != b'}' {
                            if !bytes[i].is_ascii_hexdigit() {
                                return true;
                            }
                            i += 1;
                        }
                        if i >= len || i == start {
                            return true; // unclosed or empty
                        }
                        // Check codepoint value <= 0x10FFFF
                        if let Ok(val) = u32::from_str_radix(&raw[start..i], 16) {
                            if val > 0x10FFFF {
                                return true;
                            }
                        } else {
                            return true;
                        }
                        i += 1; // skip '}'
                    } else {
                        // \uXXXX — need exactly 4 hex digits
                        if i + 4 > len {
                            return true;
                        }
                        for j in 0..4 {
                            if !bytes[i + j].is_ascii_hexdigit() {
                                return true;
                            }
                        }
                        i += 4;
                    }
                }
                // Hex escape: \xXX
                b'x' => {
                    i += 1;
                    if i + 2 > len {
                        return true;
                    }
                    if !bytes[i].is_ascii_hexdigit() || !bytes[i + 1].is_ascii_hexdigit() {
                        return true;
                    }
                    i += 2;
                }
                // \0 — valid only if NOT followed by a digit (otherwise octal)
                b'0' => {
                    i += 1;
                    if i < len && bytes[i].is_ascii_digit() {
                        return true;
                    }
                }
                // Octal digits 1-7 are always invalid in template literals
                b'1'..=b'7' => {
                    return true;
                }
                // \8, \9 are invalid
                b'8' | b'9' => {
                    return true;
                }
                // Everything else is an identity escape — valid
                _ => {
                    i += 1;
                }
            }
        } else {
            i += 1;
        }
    }
    false
}

/// Check if a tagged template literal needs `__makeTemplateObject` lowering.
/// Returns true if ANY quasis element has an invalid escape sequence.
pub(crate) fn tagged_template_needs_lowering(quasi: &TemplateLit) -> bool {
    quasi
        .quasis
        .iter()
        .any(|q| template_element_has_invalid_escape(&q.raw))
}
