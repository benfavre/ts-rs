use super::*;

impl TypeChecker {
    pub(super) fn enum_marker_name(name: &str) -> String {
        format!("__tsrs_enumdecl__{name}")
    }

    /// Use the same constant values for local declarations and injected
    /// namespace members. Ambient non-const members without initializers are
    /// computed members, including those following an explicit constant.
    pub(super) fn enum_declaration_values(
        &self,
        declaration: &EnumDecl,
        inherited_ambient: bool,
    ) -> Vec<(String, Type)> {
        let ambient = inherited_ambient
            || declaration.modifiers & MOD_DECLARE != 0
            || self
                .injected_decl_file
                .as_deref()
                .or(self.current_file_name.as_deref())
                .is_some_and(|file| {
                    file.ends_with(".d.ts") || file.ends_with(".d.mts") || file.ends_with(".d.cts")
                });
        let automatic = declaration.is_const || !ambient;
        let mut next = automatic.then_some(0.0);
        let mut members = Vec::new();
        for member in &declaration.members {
            if matches!(&member.name, PropName::Computed(expression, _)
                if !matches!(expression.kind, ExprKind::StrLit(_) | ExprKind::NumLit(_)))
            {
                continue;
            }
            let name = self.prop_name_to_string(&member.name);
            let value = if let Some(initializer) = &member.initializer {
                if let Some(number) =
                    Self::evaluate_enum_constant(initializer, &declaration.name, &members)
                {
                    next = automatic.then_some(number + 1.0);
                    Type::NumberLiteral(Self::enum_value_text(number))
                } else {
                    next = None;
                    match &initializer.kind {
                        ExprKind::StrLit(text) | ExprKind::NoSubstTemplate(text) => {
                            Type::StringLiteral(
                                self.cooked_property_literal(text, initializer.span),
                            )
                        }
                        _ => Type::Number,
                    }
                }
            } else if let Some(number) = next {
                next = Some(number + 1.0);
                Type::NumberLiteral(Self::enum_value_text(number))
            } else {
                Type::Number
            };
            members.push((name, value));
        }
        members
    }

    fn enum_condition_member(&self, expression: &Expr) -> Option<Type> {
        let ExprKind::Member(member) = &expression.kind else {
            return None;
        };
        let (name, receiver) = match &member.object.kind {
            ExprKind::Ident(name) => {
                if self.file_import_binding_names.contains(name.as_str())
                    || self.nearer_binding_is(&Self::enum_marker_name(name), name) != Some(true)
                {
                    return None;
                }
                (name.as_str(), self.lookup_var(name)?.clone())
            }
            ExprKind::Member(access) => {
                let owner = self.import_equals_namespace_target_type(&access.object)?;
                let exported_type = match &owner {
                    Type::Module(module) => module
                        .file
                        .as_deref()
                        .and_then(|file| self.lookup_module_type_exports(file))
                        .is_some_and(|names| names.contains(access.property.as_str())),
                    _ => false,
                };
                let exports = match owner {
                    Type::Namespace(namespace) => namespace.exports,
                    Type::Module(module) => module.exports,
                    _ => return None,
                };
                let value = exports
                    .iter()
                    .find(|(name, _)| name == access.property)?
                    .1
                    .clone();
                if exported_type {
                    if let Type::ObjectType(info) = &value {
                        if let Some((_, value)) = info
                            .properties
                            .iter()
                            .find(|(name, _)| name == member.property)
                        {
                            if matches!(value.as_ref(), Type::EnumVariant { enum_name, .. } if enum_name.rsplit('.').next() == Some(access.property.as_str()))
                            {
                                return Some(value.as_ref().clone());
                            }
                        }
                    }
                }
                (access.property.as_str(), value)
            }
            _ => return None,
        };
        let Type::TypeReference(key, _) = receiver else {
            return None;
        };
        // A variable holding an enum object is not the enum declaration's
        // symbol. Namespace exports retain the original declaration name.
        if key.rsplit('.').next() != Some(name) {
            return None;
        }
        let value = self
            .enum_info
            .get(&key)?
            .iter()
            .find(|(name, _)| name == member.property)?
            .1
            .clone();
        Some(Type::EnumVariant {
            enum_name: key,
            variant_name: member.property.to_string(),
            value: Some(Arc::new(value)),
        })
    }

    pub(super) fn enum_literal_truthiness(ty: &Type) -> Option<bool> {
        let Type::EnumVariant {
            value: Some(value), ..
        } = ty
        else {
            return None;
        };
        match value.as_ref() {
            Type::NumberLiteral(number) => number
                .parse::<f64>()
                .ok()
                .map(|number| number != 0.0 && !number.is_nan()),
            Type::StringLiteral(text) => Some(!text.is_empty()),
            _ => None,
        }
    }

    fn enum_condition_type(&self, expression: &Expr) -> Type {
        let expression = Self::enum_skip_parentheses(expression);
        if let Some(member) = self.enum_condition_member(expression) {
            return member;
        }
        if let ExprKind::Binary(binary) = &expression.kind {
            let left = self.enum_condition_type(&binary.left);
            if let Some(truthy) = Self::enum_literal_truthiness(&left) {
                match binary.op {
                    BinaryOp::LogAnd if !truthy => return left,
                    BinaryOp::LogOr if truthy => return left,
                    BinaryOp::NullCoal => return left,
                    BinaryOp::LogAnd | BinaryOp::LogOr => {
                        return self.enum_condition_type(&binary.right)
                    }
                    _ => {}
                }
            }
        }
        self.infer_expr_type(expression)
    }

    fn enum_skip_parentheses(mut expression: &Expr) -> &Expr {
        while let ExprKind::Paren(inner) = &expression.kind {
            expression = inner;
        }
        expression
    }

    fn has_enum_condition_member(&self, expression: &Expr) -> bool {
        let expression = Self::enum_skip_parentheses(expression);
        match &expression.kind {
            ExprKind::Member(_) => self.enum_condition_member(expression).is_some(),
            ExprKind::Binary(binary)
                if matches!(
                    binary.op,
                    BinaryOp::LogAnd | BinaryOp::LogOr | BinaryOp::NullCoal
                ) =>
            {
                self.has_enum_condition_member(&binary.left)
                    || self.has_enum_condition_member(&binary.right)
            }
            _ => false,
        }
    }

    /// Mirrors the reference check's choice of the rightmost logical operand,
    /// followed by the left operands of ||/??. The condition's result type is
    /// retained when a left operand becomes the whole checked expression.
    pub(super) fn check_enum_known_condition(&mut self, expression: &Expr, ty: Option<&Type>) {
        if !self.strict_null_checks || !self.has_enum_condition_member(expression) {
            return;
        }
        let initial_type = ty
            .cloned()
            .unwrap_or_else(|| self.enum_condition_type(expression));
        let mut current = Self::enum_skip_parentheses(expression);
        loop {
            let location = match &current.kind {
                ExprKind::Binary(binary)
                    if matches!(
                        binary.op,
                        BinaryOp::LogAnd | BinaryOp::LogOr | BinaryOp::NullCoal
                    ) =>
                {
                    Self::enum_skip_parentheses(&binary.right)
                }
                _ => current,
            };
            if matches!(&location.kind, ExprKind::Binary(binary) if matches!(binary.op, BinaryOp::LogAnd | BinaryOp::LogOr | BinaryOp::NullCoal))
            {
                self.check_enum_known_condition(location, Some(&initial_type));
            } else if self.enum_condition_member(location).is_some() {
                let value = if std::ptr::eq(location, current) {
                    initial_type.clone()
                } else {
                    self.enum_condition_type(location)
                };
                if let Some(truthy) = Self::enum_literal_truthiness(&value) {
                    let message = format!("This condition will always return '{truthy}'.");
                    if !self.diagnostics.iter().any(|diagnostic| {
                        diagnostic.code == 2845
                            && diagnostic.span == Some(location.span)
                            && diagnostic.message == message
                    }) {
                        self.diagnostics.push(Diagnostic {
                            code: 2845,
                            message,
                            category: DiagnosticCategory::Error,
                            file_name: None,
                            span: Some(location.span),
                            related: None,
                        });
                    }
                }
            }
            match &current.kind {
                ExprKind::Binary(binary)
                    if matches!(binary.op, BinaryOp::LogOr | BinaryOp::NullCoal) =>
                {
                    current = Self::enum_skip_parentheses(&binary.left)
                }
                _ => break,
            }
        }
    }

    pub(super) fn check_enum_if_operands(&mut self, expression: &Expr) {
        let expression = Self::enum_skip_parentheses(expression);
        if let ExprKind::Binary(binary) = &expression.kind {
            if matches!(
                binary.op,
                BinaryOp::LogAnd | BinaryOp::LogOr | BinaryOp::NullCoal
            ) {
                self.check_enum_if_operands(&binary.left);
                self.check_enum_known_condition(&binary.left, None);
                self.check_enum_if_operands(&binary.right);
            }
        }
    }
}
