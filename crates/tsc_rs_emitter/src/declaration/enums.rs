use super::*;

#[derive(Clone)]
pub(super) enum EnumValue {
    Number(f64),
    String(String),
}

impl EnumValue {
    fn text(&self) -> String {
        match self {
            Self::Number(value) if !value.is_finite() => {
                crate::enum_eval::format_enum_value(*value)
            }
            Self::Number(value) => crate::Emitter::format_f64_as_js(*value),
            Self::String(value) => value.clone(),
        }
    }
}

impl DeclarationEmitter<'_> {
    fn enum_path(expr: &Expr) -> Option<String> {
        match &expr.kind {
            ExprKind::Ident(name) => Some(name.to_string()),
            ExprKind::Member(member) => Some(format!(
                "{}.{}",
                Self::enum_path(&member.object)?,
                member.property
            )),
            _ => None,
        }
    }

    fn referenced_enum_value(&self, path: &str, member: &str) -> Option<EnumValue> {
        for depth in (0..=self.enum_scope.len()).rev() {
            let mut parts = self.enum_scope[..depth].to_vec();
            parts.push(path.to_string());
            if let Some(values) = self.enum_values.get(&parts.join(".")) {
                return values.get(member).cloned();
            }
        }
        None
    }

    fn enum_initializer_value(&self, expr: &Expr, current: &str) -> Option<EnumValue> {
        use crate::enum_eval::{parse_js_number, try_eval_enum_expr};
        let numeric_expr = |value: f64| Expr {
            kind: ExprKind::NumLit(EnumValue::Number(value).text().into()),
            span: expr.span,
        };
        match &expr.kind {
            ExprKind::NumLit(text) => {
                parse_js_number(&crate::Emitter::normalize_numeric_literal(text))
                    .map(EnumValue::Number)
            }
            ExprKind::StrLit(text) | ExprKind::NoSubstTemplate(text) => Some(EnumValue::String(
                crate::emit_class::decode_js_string_content(text),
            )),
            ExprKind::Ident(name) => self.enum_values.get(current)?.get(name.as_str()).cloned(),
            ExprKind::Member(member) => {
                self.referenced_enum_value(&Self::enum_path(&member.object)?, &member.property)
            }
            ExprKind::ElemAccess(member) => {
                let key = match &member.index.kind {
                    ExprKind::StrLit(text) => crate::emit_class::decode_js_string_content(text),
                    _ => return None,
                };
                self.referenced_enum_value(&Self::enum_path(&member.object)?, &key)
            }
            ExprKind::Paren(inner) => self.enum_initializer_value(inner, current),
            ExprKind::Unary(unary) => {
                let EnumValue::Number(value) =
                    self.enum_initializer_value(&unary.argument, current)?
                else {
                    return None;
                };
                let mut resolved = unary.clone();
                resolved.argument = Box::new(numeric_expr(value));
                let resolved = Expr {
                    kind: ExprKind::Unary(resolved),
                    span: expr.span,
                };
                try_eval_enum_expr(&resolved, &HashMap::new(), &HashMap::new())
                    .map(EnumValue::Number)
            }
            ExprKind::Binary(binary) => {
                let left = self.enum_initializer_value(&binary.left, current)?;
                let right = self.enum_initializer_value(&binary.right, current)?;
                match (left, right) {
                    (EnumValue::Number(left), EnumValue::Number(right)) => {
                        let mut resolved = binary.clone();
                        resolved.left = Box::new(numeric_expr(left));
                        resolved.right = Box::new(numeric_expr(right));
                        let resolved = Expr {
                            kind: ExprKind::Binary(resolved),
                            span: expr.span,
                        };
                        try_eval_enum_expr(&resolved, &HashMap::new(), &HashMap::new())
                            .map(EnumValue::Number)
                    }
                    (left, right) if binary.op == BinaryOp::Add => Some(EnumValue::String(
                        format!("{}{}", left.text(), right.text()),
                    )),
                    _ => None,
                }
            }
            ExprKind::Template(template) => {
                let mut text = String::new();
                for (index, quasi) in template.quasis.iter().enumerate() {
                    text.push_str(quasi.cooked.as_deref().unwrap_or(&quasi.raw));
                    if let Some(expr) = template.exprs.get(index) {
                        text.push_str(&self.enum_initializer_value(expr, current)?.text());
                    }
                }
                Some(EnumValue::String(text))
            }
            _ => None,
        }
    }

    pub(super) fn emit_enum_decl(&mut self, declaration: &EnumDecl) {
        if declaration.is_const {
            self.write("const ");
        }
        self.write("enum ");
        self.write(&declaration.name);
        self.writeln(" {");
        self.indent += 1;
        let mut path = self.enum_scope.clone();
        path.push(declaration.name.to_string());
        let path = path.join(".");
        self.enum_values.entry(path.clone()).or_default();
        let ambient = self.enum_ambient || declaration.modifiers & MOD_DECLARE != 0;
        let mut next = Some(0.0);
        for (index, member) in declaration.members.iter().enumerate() {
            let value = if let Some(initializer) = &member.initializer {
                self.enum_initializer_value(initializer, &path)
            } else if !ambient || declaration.is_const {
                next.map(EnumValue::Number)
            } else {
                None
            };
            next = match &value {
                Some(EnumValue::Number(value)) => Some(value + 1.0),
                _ => None,
            };
            self.emit_prop_name(&member.name);
            if let Some(value) = &value {
                self.write(" = ");
                match value {
                    EnumValue::Number(_) => self.write(&value.text()),
                    EnumValue::String(text) => {
                        self.write("\"");
                        self.write(&crate::emit_expr::escape_js_string_for_quote(text, '"'));
                        self.write("\"");
                    }
                }
            }
            let key = match &member.name {
                PropName::Ident(name, _) => Some(name.to_string()),
                PropName::String(name, _) => {
                    Some(crate::emit_class::decode_js_string_content(name))
                }
                PropName::Number(name, _) => Some(crate::enum_eval::normalize_js_number(name)),
                _ => None,
            };
            if let Some(key) = key {
                let values = self.enum_values.get_mut(&path).unwrap();
                if let Some(value) = value {
                    values.insert(key, value);
                } else {
                    values.remove(&key);
                }
            }
            if index + 1 < declaration.members.len() {
                self.writeln(",");
            } else {
                self.newline();
            }
        }
        self.indent -= 1;
        self.writeln("}");
    }
}
