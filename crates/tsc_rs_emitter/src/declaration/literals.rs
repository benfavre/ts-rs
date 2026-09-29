use super::*;

impl DeclarationEmitter<'_> {
    pub(super) fn const_initializer_text(expr: &Expr) -> Option<String> {
        match &expr.kind {
            ExprKind::Paren(inner) => Self::const_initializer_text(inner),
            ExprKind::NumLit(text) => Self::numeric_literal_text(text, false),
            ExprKind::BigIntLit(text) => Self::bigint_literal_text(text),
            ExprKind::StrLit(text) | ExprKind::NoSubstTemplate(text) => Some(format!(
                "\"{}\"",
                crate::jsx::jsx_escape_string_decoded(
                    &crate::emit_class::decode_js_string_content(text)
                )
            )),
            ExprKind::BoolLit(value) => Some(value.to_string()),
            ExprKind::Unary(unary) => match (&unary.op, &unary.argument.kind) {
                (UnaryOp::Neg | UnaryOp::Pos, ExprKind::NumLit(text)) => {
                    Self::numeric_literal_text(text, unary.op == UnaryOp::Neg)
                }
                (UnaryOp::Neg, ExprKind::BigIntLit(text)) => {
                    let value = Self::bigint_literal_text(text)?;
                    Some(if value == "0n" {
                        value
                    } else {
                        format!("-{value}")
                    })
                }
                _ => None,
            },
            // Type assertions and arithmetic expressions have their own type
            // inference rules; they must not inherit a literal initializer.
            _ => None,
        }
    }

    fn numeric_literal_text(text: &str, negative: bool) -> Option<String> {
        let normalized = crate::Emitter::normalize_numeric_literal(text);
        let value = crate::enum_eval::parse_js_number(&normalized)?;
        Some(crate::Emitter::format_f64_as_js(if negative {
            -value
        } else {
            value
        }))
    }

    fn bigint_literal_text(text: &str) -> Option<String> {
        let clean = text.strip_suffix('n')?.replace('_', "");
        let (radix, digits) = if clean.starts_with("0x") || clean.starts_with("0X") {
            (16u32, &clean[2..])
        } else if clean.starts_with("0o") || clean.starts_with("0O") {
            (8u32, &clean[2..])
        } else if clean.starts_with("0b") || clean.starts_with("0B") {
            (2u32, &clean[2..])
        } else {
            (10u32, clean.as_str())
        };
        if digits.is_empty() {
            return None;
        }
        // Decimal digits avoid imposing a machine-integer limit on bigint.
        let mut decimal = vec![0u32];
        for digit in digits.chars() {
            let mut carry = digit.to_digit(radix)?;
            for place in &mut decimal {
                let value = *place * radix + carry;
                *place = value % 10;
                carry = value / 10;
            }
            while carry > 0 {
                decimal.push(carry % 10);
                carry /= 10;
            }
        }
        let mut value: String = decimal
            .iter()
            .rev()
            .map(|digit| char::from(b'0' + *digit as u8))
            .collect();
        value.push('n');
        Some(value)
    }
}
