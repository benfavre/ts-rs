//! Enum constant evaluation helpers.

use super::*;

/// Try to extract a constant integer value from an expression.
/// Handles numeric literals and unary minus applied to a numeric literal.
pub(crate) fn try_extract_numeric_value(expr: &Expr) -> Option<f64> {
    match &expr.kind {
        ExprKind::NumLit(s) => s.parse::<f64>().ok(),
        ExprKind::Unary(unary) if unary.op == UnaryOp::Neg => {
            if let ExprKind::NumLit(s) = &unary.argument.kind {
                s.parse::<f64>().ok().map(|v| -v)
            } else {
                None
            }
        }
        ExprKind::Unary(unary) if unary.op == UnaryOp::Pos => {
            if let ExprKind::NumLit(s) = &unary.argument.kind {
                s.parse::<f64>().ok()
            } else {
                None
            }
        }
        ExprKind::Paren(inner) => try_extract_numeric_value(inner),
        _ => None,
    }
}

/// Try to extract a numeric literal value for const-enum evaluation.
/// Unlike `try_extract_numeric_value`, this preserves non-integer literals.
pub(crate) fn try_extract_numeric_value_const_enum(expr: &Expr) -> Option<f64> {
    match &expr.kind {
        ExprKind::NumLit(s) => s.parse::<f64>().ok(),
        ExprKind::Unary(unary) if unary.op == UnaryOp::Neg => {
            if let ExprKind::NumLit(s) = &unary.argument.kind {
                s.parse::<f64>().ok().map(|v| -v)
            } else {
                None
            }
        }
        ExprKind::Unary(unary) if unary.op == UnaryOp::Pos => {
            if let ExprKind::NumLit(s) = &unary.argument.kind {
                s.parse::<f64>().ok()
            } else {
                None
            }
        }
        ExprKind::Paren(inner) => try_extract_numeric_value_const_enum(inner),
        _ => None,
    }
}

/// Normalize a JavaScript numeric literal to its canonical form.
/// E.g., `1.0` → `1`, `11e-1` → `1.1`, `0xF00D` → `61453`, `0o10` → `8`.
pub(crate) fn normalize_js_number(s: &str) -> String {
    let val: f64 = if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        i64::from_str_radix(hex, 16)
            .ok()
            .map(|v| v as f64)
            .unwrap_or(f64::NAN)
    } else if let Some(oct) = s.strip_prefix("0o").or_else(|| s.strip_prefix("0O")) {
        i64::from_str_radix(oct, 8)
            .ok()
            .map(|v| v as f64)
            .unwrap_or(f64::NAN)
    } else if let Some(bin) = s.strip_prefix("0b").or_else(|| s.strip_prefix("0B")) {
        i64::from_str_radix(bin, 2)
            .ok()
            .map(|v| v as f64)
            .unwrap_or(f64::NAN)
    } else {
        s.parse::<f64>().unwrap_or(f64::NAN)
    };
    if val.is_nan() || val.is_infinite() {
        return s.to_string();
    }
    if val == val.trunc() && val.abs() < (i64::MAX as f64) {
        format!("{}", val as i64)
    } else {
        // Use default f64 formatting which gives minimal representation
        format!("{}", val)
    }
}

/// Format an f64 enum value as a JavaScript number string.
pub(crate) fn format_enum_value(v: f64) -> String {
    if v == f64::INFINITY {
        "Infinity".to_string()
    } else if v == f64::NEG_INFINITY {
        "-Infinity".to_string()
    } else if v.is_nan() {
        "NaN".to_string()
    } else if v == v.trunc() && v.abs() < (i64::MAX as f64) {
        format!("{}", v as i64)
    } else {
        format!("{}", v)
    }
}

/// Evaluate an enum initializer expression to a constant integer value.
/// Supports binary ops, unary ops, parenthesized expressions, and
/// references to previously-defined enum members via a lookup map.
#[allow(dead_code)] // enum constant evaluation
pub(crate) fn try_eval_enum_expr(
    expr: &Expr,
    members: &std::collections::HashMap<String, f64>,
    merged_enum_values: &std::collections::HashMap<String, std::collections::HashMap<String, f64>>,
) -> Option<f64> {
    try_eval_enum_expr_with_consts(
        expr,
        members,
        merged_enum_values,
        &std::collections::HashMap::new(),
    )
}

/// Like `try_eval_enum_expr` but also resolves file-level `const` declarations
/// with known numeric values (e.g. `const EV = 1`).
pub(crate) fn try_eval_enum_expr_with_consts(
    expr: &Expr,
    members: &std::collections::HashMap<String, f64>,
    merged_enum_values: &std::collections::HashMap<String, std::collections::HashMap<String, f64>>,
    file_consts: &std::collections::HashMap<String, f64>,
) -> Option<f64> {
    match &expr.kind {
        ExprKind::NumLit(s) => parse_js_number(s),
        ExprKind::Unary(unary) => {
            let arg = try_eval_enum_expr_with_consts(
                &unary.argument,
                members,
                merged_enum_values,
                file_consts,
            )?;
            match unary.op {
                UnaryOp::Neg => Some(-arg),
                UnaryOp::Pos => Some(arg),
                UnaryOp::BitNot => Some((!js_to_int32(arg)) as f64),
                _ => None,
            }
        }
        ExprKind::Binary(bin) => {
            let l = try_eval_enum_expr_with_consts(
                &bin.left,
                members,
                merged_enum_values,
                file_consts,
            )?;
            let r = try_eval_enum_expr_with_consts(
                &bin.right,
                members,
                merged_enum_values,
                file_consts,
            )?;
            match bin.op {
                BinaryOp::Add => Some(l + r),
                BinaryOp::Sub => Some(l - r),
                BinaryOp::Mul => Some(l * r),
                BinaryOp::Div => Some(l / r),
                BinaryOp::Mod => Some(l % r),
                BinaryOp::Shl => {
                    let li = js_to_int32(l);
                    let ri = js_to_uint32(r) & 0x1F;
                    Some((li.wrapping_shl(ri)) as f64)
                }
                BinaryOp::Shr => {
                    let li = js_to_int32(l);
                    let ri = js_to_uint32(r) & 0x1F;
                    Some((li.wrapping_shr(ri)) as f64)
                }
                BinaryOp::UShr => {
                    let li = js_to_uint32(l);
                    let ri = js_to_uint32(r) & 0x1F;
                    Some((li.wrapping_shr(ri)) as f64)
                }
                BinaryOp::BitAnd => Some((js_to_int32(l) & js_to_int32(r)) as f64),
                BinaryOp::BitOr => Some((js_to_int32(l) | js_to_int32(r)) as f64),
                BinaryOp::BitXor => Some((js_to_int32(l) ^ js_to_int32(r)) as f64),
                BinaryOp::Exp => Some(l.powf(r)),
                _ => None,
            }
        }
        ExprKind::Paren(inner) => {
            try_eval_enum_expr_with_consts(inner, members, merged_enum_values, file_consts)
        }
        // Reference to another enum member: `EnumName.Member` or just `Member`
        ExprKind::Ident(name) => members
            .get(name.as_str())
            .copied()
            .or_else(|| file_consts.get(name.as_str()).copied()),
        ExprKind::Member(member) => {
            // First try current enum members
            if let Some(v) = members.get(member.property.as_str()) {
                return Some(*v);
            }
            // Cross-enum reference: `OtherEnum.Member`
            if let ExprKind::Ident(obj_name) = &member.object.kind {
                let composite = format!("{}.{}", obj_name, member.property);
                if let Some(v) = file_consts.get(composite.as_str()) {
                    return Some(*v);
                }
                if let Some(other_members) = merged_enum_values.get(obj_name.as_str()) {
                    return other_members.get(member.property.as_str()).copied();
                }
            }
            // Namespace-qualified cross-enum: `M.N.OtherEnum.Member`
            // The object is a chain; the rightmost name is the enum name.
            if let ExprKind::Member(inner) = &member.object.kind {
                let enum_ident = inner.property.as_str();
                if let Some(other_members) = merged_enum_values.get(enum_ident) {
                    return other_members.get(member.property.as_str()).copied();
                }
            }
            None
        }
        // Element access: `EnumName["member"]`
        ExprKind::ElemAccess(ea) => {
            if let ExprKind::StrLit(key) = &ea.index.kind {
                if let Some(v) = members.get(key.as_str()) {
                    return Some(*v);
                }
                // Cross-enum element access: `OtherEnum["Member"]`
                if let ExprKind::Ident(obj_name) = &ea.object.kind {
                    let composite = format!("{}.{}", obj_name, key);
                    if let Some(v) = file_consts.get(composite.as_str()) {
                        return Some(*v);
                    }
                    if let Some(other_members) = merged_enum_values.get(obj_name.as_str()) {
                        return other_members.get(key.as_str()).copied();
                    }
                }
                None
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Parse a JavaScript numeric literal (decimal, hex, octal, binary) to f64.
/// Handles `_` separators and prefixes like `0x`, `0o`, `0b`.
pub(crate) fn parse_js_number(s: &str) -> Option<f64> {
    // Strip numeric separators
    let s_clean: String;
    let s = if s.contains('_') {
        s_clean = s.replace('_', "");
        &s_clean
    } else {
        s
    };
    if s.starts_with("0x") || s.starts_with("0X") {
        u64::from_str_radix(&s[2..], 16).ok().map(|v| v as f64)
    } else if s.starts_with("0o") || s.starts_with("0O") {
        u64::from_str_radix(&s[2..], 8).ok().map(|v| v as f64)
    } else if s.starts_with("0b") || s.starts_with("0B") {
        u64::from_str_radix(&s[2..], 2).ok().map(|v| v as f64)
    } else {
        s.parse::<f64>().ok()
    }
}

/// JavaScript `ToInt32` — convert f64 to i32 using JS semantics (modulo 2^32, signed).
pub(crate) fn js_to_int32(v: f64) -> i32 {
    if v.is_nan() || v.is_infinite() || v == 0.0 {
        return 0;
    }
    // Truncate toward zero, then wrap modulo 2^32
    let n = v.trunc() % 4294967296.0; // 2^32
    let n = if n >= 0.0 { n } else { n + 4294967296.0 };
    // Now n is in [0, 2^32). Convert to i32 (values >= 2^31 become negative).
    if n >= 2147483648.0 {
        (n - 4294967296.0) as i32
    } else {
        n as i32
    }
}

/// JavaScript `ToUint32` — convert f64 to u32 using JS semantics (modulo 2^32, unsigned).
pub(crate) fn js_to_uint32(v: f64) -> u32 {
    js_to_int32(v) as u32
}

/// Evaluate a string enum initializer expression to a constant string value.
/// Supports string literals, references to known string-valued enum members,
/// cross-enum references, and string concatenation via `+`.
#[allow(dead_code)] // string enum constant evaluation
pub(crate) fn try_eval_string_enum_expr(
    expr: &Expr,
    string_member_values: &std::collections::HashMap<String, String>,
    merged_string_enum_values: &std::collections::HashMap<
        String,
        std::collections::HashMap<String, String>,
    >,
) -> Option<String> {
    try_eval_string_enum_expr_with_consts(
        expr,
        string_member_values,
        merged_string_enum_values,
        &std::collections::HashMap::new(),
    )
}

/// Like `try_eval_string_enum_expr` but also resolves file-level `const`
/// declarations with known string values (e.g. `const d = 'd'`).
pub(crate) fn try_eval_string_enum_expr_with_consts(
    expr: &Expr,
    string_member_values: &std::collections::HashMap<String, String>,
    merged_string_enum_values: &std::collections::HashMap<
        String,
        std::collections::HashMap<String, String>,
    >,
    file_string_consts: &std::collections::HashMap<String, String>,
) -> Option<String> {
    try_eval_string_enum_expr_full(
        expr,
        string_member_values,
        merged_string_enum_values,
        file_string_consts,
        &std::collections::HashMap::new(),
    )
}

/// Full version that also resolves numeric file-level constants for string
/// coercion (e.g. `\`${foo}\`` where `const foo = 2` → `"2"`).
pub(crate) fn try_eval_string_enum_expr_full(
    expr: &Expr,
    string_member_values: &std::collections::HashMap<String, String>,
    merged_string_enum_values: &std::collections::HashMap<
        String,
        std::collections::HashMap<String, String>,
    >,
    file_string_consts: &std::collections::HashMap<String, String>,
    file_num_consts: &std::collections::HashMap<String, f64>,
) -> Option<String> {
    match &expr.kind {
        ExprKind::StrLit(s) => Some(s.to_string()),
        ExprKind::NoSubstTemplate(s) => Some(s.to_string()),
        // Number-to-string coercion for cases like `"" + 2` or `2 + ""`
        ExprKind::NumLit(n) => {
            let val = parse_js_number(n)?;
            Some(format_enum_value(val))
        }
        // Template literal: `\`prefix${expr}suffix\``
        ExprKind::Template(tpl) => {
            let mut result = String::new();
            for (idx, quasi) in tpl.quasis.iter().enumerate() {
                result.push_str(quasi.cooked.as_deref().unwrap_or(&quasi.raw));
                if idx < tpl.exprs.len() {
                    // Try evaluating the interpolated expression as a string
                    let val = try_eval_string_enum_expr_full(
                        &tpl.exprs[idx],
                        string_member_values,
                        merged_string_enum_values,
                        file_string_consts,
                        file_num_consts,
                    )?;
                    result.push_str(&val);
                }
            }
            Some(result)
        }
        ExprKind::Binary(bin) if bin.op == BinaryOp::Add => {
            let l = try_eval_string_enum_expr_full(
                &bin.left,
                string_member_values,
                merged_string_enum_values,
                file_string_consts,
                file_num_consts,
            )?;
            let r = try_eval_string_enum_expr_full(
                &bin.right,
                string_member_values,
                merged_string_enum_values,
                file_string_consts,
                file_num_consts,
            )?;
            Some(format!("{}{}", l, r))
        }
        ExprKind::Paren(inner) => try_eval_string_enum_expr_full(
            inner,
            string_member_values,
            merged_string_enum_values,
            file_string_consts,
            file_num_consts,
        ),
        // Reference to another member in the same enum: `A` (bare identifier)
        ExprKind::Ident(id) => string_member_values
            .get(id.as_str())
            .cloned()
            .or_else(|| file_string_consts.get(id.as_str()).cloned())
            .or_else(|| {
                file_num_consts
                    .get(id.as_str())
                    .map(|v| format_enum_value(*v))
            }),
        // Member access: `EnumName.Member` or `EnumName["Member"]`
        ExprKind::Member(member) => {
            // Try current enum members first
            if let Some(v) = string_member_values.get(member.property.as_str()) {
                return Some(v.clone());
            }
            // Cross-enum reference: `OtherEnum.Member`
            if let ExprKind::Ident(obj_name) = &member.object.kind {
                if let Some(other_members) = merged_string_enum_values.get(obj_name.as_str()) {
                    return other_members.get(member.property.as_str()).cloned();
                }
            }
            // Namespace-qualified: `M.OtherEnum.Member`
            if let ExprKind::Member(inner) = &member.object.kind {
                let enum_ident = inner.property.as_str();
                if let Some(other_members) = merged_string_enum_values.get(enum_ident) {
                    return other_members.get(member.property.as_str()).cloned();
                }
            }
            None
        }
        // Element access: `EnumName["Member"]`
        ExprKind::ElemAccess(ea) => {
            if let ExprKind::StrLit(key) = &ea.index.kind {
                if let Some(v) = string_member_values.get(key.as_str()) {
                    return Some(v.clone());
                }
                // Cross-enum element access: `OtherEnum["Member"]`
                if let ExprKind::Ident(obj_name) = &ea.object.kind {
                    if let Some(other_members) = merged_string_enum_values.get(obj_name.as_str()) {
                        return other_members.get(key.as_str()).cloned();
                    }
                }
            }
            None
        }
        _ => None,
    }
}
