//! Expression type checking methods for TypeChecker.

use std::sync::Arc;

use super::*;

#[derive(Clone, Copy)]
enum SyntacticTruthiness {
    Always,
    Never,
}

#[derive(Clone)]
enum Constructability {
    Dynamic,
    All(Vec<Vec<ConstructorType>>),
    Overloads(Vec<ConstructorType>),
    None,
}

struct FutureLibReceiver {
    owner: String,
    display: String,
}

impl TypeChecker {
    fn is_numeric_enum_members(members: &[(String, Type)]) -> bool {
        members
            .iter()
            .all(|(_, value)| matches!(value, Type::Number | Type::NumberLiteral(_)))
    }

    fn is_string_enum_members(members: &[(String, Type)]) -> bool {
        members
            .iter()
            .all(|(_, value)| matches!(value, Type::String | Type::StringLiteral(_)))
    }

    fn is_numeric_enum_type(&self, ty: &Type) -> bool {
        match ty {
            Type::TypeReference(name, args) if args.is_empty() => {
                if let Some(members) = self.enum_info.get(name.as_str()) {
                    return Self::is_numeric_enum_members(members);
                }
                if let Some((enum_name, member_name)) = name.rsplit_once('.') {
                    if let Some(members) = self.enum_info.get(enum_name) {
                        return members.iter().any(|(name, _)| name == member_name)
                            && Self::is_numeric_enum_members(members);
                    }
                }
                self.resolve_type_for_assignability(ty)
                    .filter(|resolved| resolved != ty)
                    .is_some_and(|resolved| self.is_numeric_enum_type(&resolved))
            }
            Type::EnumType(info) => Self::is_numeric_enum_members(&info.members),
            Type::EnumVariant { enum_name, .. } => self
                .enum_info
                .get(enum_name.as_str())
                .is_some_and(|members| Self::is_numeric_enum_members(members)),
            _ => false,
        }
    }

    fn is_string_enum_type(&self, ty: &Type) -> bool {
        match ty {
            Type::TypeReference(name, args) if args.is_empty() => {
                if let Some(members) = self.enum_info.get(name.as_str()) {
                    return Self::is_string_enum_members(members);
                }
                if let Some((enum_name, member_name)) = name.rsplit_once('.') {
                    if let Some(members) = self.enum_info.get(enum_name) {
                        return members.iter().any(|(name, value)| {
                            name == member_name
                                && matches!(value, Type::String | Type::StringLiteral(_))
                        });
                    }
                }
                self.resolve_type_for_assignability(ty)
                    .filter(|resolved| resolved != ty)
                    .is_some_and(|resolved| self.is_string_enum_type(&resolved))
            }
            Type::EnumType(info) => Self::is_string_enum_members(&info.members),
            Type::EnumVariant {
                enum_name, value, ..
            } => value.as_deref().map_or_else(
                || {
                    self.enum_info
                        .get(enum_name.as_str())
                        .is_some_and(|members| Self::is_string_enum_members(members))
                },
                |value| matches!(value, Type::String | Type::StringLiteral(_)),
            ),
            _ => false,
        }
    }

    fn binary_operator_type_without_strict_nullish(&self, ty: &Type) -> Type {
        if !self.strict_null_checks {
            return ty.clone();
        }
        match ty {
            Type::Null | Type::Undefined => Type::Never,
            Type::Optional(inner) => self.binary_operator_type_without_strict_nullish(inner),
            Type::Union(members) => {
                let non_nullish: Vec<Type> = members
                    .iter()
                    .filter(|member| !matches!(member, Type::Null | Type::Undefined))
                    .cloned()
                    .collect();
                if non_nullish.is_empty() {
                    Type::Never
                } else {
                    Type::flatten_union(non_nullish)
                }
            }
            _ => ty.clone(),
        }
    }

    fn binary_operator_type_has_unknown(&self, ty: &Type) -> bool {
        match ty {
            Type::Unknown => true,
            Type::Union(members) => members
                .iter()
                .any(|member| self.binary_operator_type_has_unknown(member)),
            Type::Intersection(members) => members
                .iter()
                .all(|member| self.binary_operator_type_has_unknown(member)),
            Type::Optional(inner) => self.binary_operator_type_has_unknown(inner),
            _ => false,
        }
    }

    fn binary_operator_type_is_dynamic(&self, ty: &Type) -> bool {
        match ty {
            Type::Any | Type::Never | Type::Error => true,
            Type::Union(members) => members
                .iter()
                .any(|member| self.binary_operator_type_is_dynamic(member)),
            Type::Optional(inner) => self.binary_operator_type_is_dynamic(inner),
            _ => false,
        }
    }

    fn binary_operator_type_is_string_like(&self, ty: &Type) -> bool {
        match ty {
            Type::String | Type::StringLiteral(_) | Type::TemplateLiteral { .. } => true,
            Type::Union(members) => {
                !members.is_empty()
                    && members
                        .iter()
                        .all(|member| self.binary_operator_type_is_string_like(member))
            }
            Type::Intersection(members) => members
                .iter()
                .any(|member| self.binary_operator_type_is_string_like(member)),
            Type::Optional(inner) => self.binary_operator_type_is_string_like(inner),
            Type::TypeParameter(_) => self
                .typeparam_constraint_apparent(ty)
                .is_some_and(|constraint| self.binary_operator_type_is_string_like(&constraint)),
            Type::TypeReference(_, args) if args.is_empty() => {
                self.is_string_enum_type(ty)
                    || self
                        .resolve_type_for_assignability(ty)
                        .filter(|resolved| resolved != ty)
                        .is_some_and(|resolved| self.binary_operator_type_is_string_like(&resolved))
                    || self
                        .typeparam_constraint_apparent(ty)
                        .is_some_and(|constraint| {
                            self.binary_operator_type_is_string_like(&constraint)
                        })
            }
            Type::EnumType(_) | Type::EnumVariant { .. } => self.is_string_enum_type(ty),
            _ => false,
        }
    }

    fn binary_operator_type_is_number_like(&self, ty: &Type) -> bool {
        match ty {
            Type::Number | Type::NumberLiteral(_) => true,
            Type::Union(members) => {
                !members.is_empty()
                    && members
                        .iter()
                        .all(|member| self.binary_operator_type_is_number_like(member))
            }
            Type::Intersection(members) => members
                .iter()
                .any(|member| self.binary_operator_type_is_number_like(member)),
            Type::Optional(inner) => self.binary_operator_type_is_number_like(inner),
            Type::TypeParameter(_) => self
                .typeparam_constraint_apparent(ty)
                .is_some_and(|constraint| self.binary_operator_type_is_number_like(&constraint)),
            Type::TypeReference(_, args) if args.is_empty() => {
                self.is_numeric_enum_type(ty)
                    || self
                        .resolve_type_for_assignability(ty)
                        .filter(|resolved| resolved != ty)
                        .is_some_and(|resolved| self.binary_operator_type_is_number_like(&resolved))
                    || self
                        .typeparam_constraint_apparent(ty)
                        .is_some_and(|constraint| {
                            self.binary_operator_type_is_number_like(&constraint)
                        })
            }
            Type::EnumType(_) | Type::EnumVariant { .. } => self.is_numeric_enum_type(ty),
            _ => false,
        }
    }

    fn binary_operator_type_is_bigint_like(&self, ty: &Type) -> bool {
        match ty {
            Type::BigInt | Type::BigIntLiteral(_) => true,
            Type::Union(members) => {
                !members.is_empty()
                    && members
                        .iter()
                        .all(|member| self.binary_operator_type_is_bigint_like(member))
            }
            Type::Intersection(members) => members
                .iter()
                .any(|member| self.binary_operator_type_is_bigint_like(member)),
            Type::Optional(inner) => self.binary_operator_type_is_bigint_like(inner),
            Type::TypeParameter(_) => self
                .typeparam_constraint_apparent(ty)
                .is_some_and(|constraint| self.binary_operator_type_is_bigint_like(&constraint)),
            Type::TypeReference(_, args) if args.is_empty() => {
                self.resolve_type_for_assignability(ty)
                    .filter(|resolved| resolved != ty)
                    .is_some_and(|resolved| self.binary_operator_type_is_bigint_like(&resolved))
                    || self
                        .typeparam_constraint_apparent(ty)
                        .is_some_and(|constraint| {
                            self.binary_operator_type_is_bigint_like(&constraint)
                        })
            }
            _ => false,
        }
    }

    fn binary_operator_type_is_numeric_like(&self, ty: &Type) -> bool {
        match ty {
            Type::Union(members) => {
                !members.is_empty()
                    && members
                        .iter()
                        .all(|member| self.binary_operator_type_is_numeric_like(member))
            }
            Type::Intersection(members) => members
                .iter()
                .any(|member| self.binary_operator_type_is_numeric_like(member)),
            _ => {
                self.binary_operator_type_is_number_like(ty)
                    || self.binary_operator_type_is_bigint_like(ty)
            }
        }
    }

    fn binary_operator_type_is_boolean_like(&self, ty: &Type) -> bool {
        match ty {
            Type::Boolean | Type::BooleanLiteral(_) => true,
            Type::Union(members) => {
                !members.is_empty()
                    && members
                        .iter()
                        .all(|member| self.binary_operator_type_is_boolean_like(member))
            }
            Type::Intersection(members) => members
                .iter()
                .any(|member| self.binary_operator_type_is_boolean_like(member)),
            Type::Optional(inner) => self.binary_operator_type_is_boolean_like(inner),
            Type::TypeParameter(_) => self
                .typeparam_constraint_apparent(ty)
                .is_some_and(|constraint| self.binary_operator_type_is_boolean_like(&constraint)),
            Type::TypeReference(_, args) if args.is_empty() => self
                .resolve_type_for_assignability(ty)
                .filter(|resolved| resolved != ty)
                .is_some_and(|resolved| self.binary_operator_type_is_boolean_like(&resolved)),
            _ => false,
        }
    }

    fn relational_type_has_symbol(&self, ty: &Type) -> bool {
        match ty {
            Type::Symbol | Type::UniqueSymbol(_) => true,
            Type::Union(members) | Type::Intersection(members) => members
                .iter()
                .any(|member| self.relational_type_has_symbol(member)),
            Type::Optional(inner) => self.relational_type_has_symbol(inner),
            _ => false,
        }
    }

    fn addition_types_are_compatible(&self, left: &Type, right: &Type) -> bool {
        self.binary_operator_type_is_dynamic(left)
            || self.binary_operator_type_is_dynamic(right)
            || self.binary_operator_type_is_string_like(left)
            || self.binary_operator_type_is_string_like(right)
            || (self.binary_operator_type_is_number_like(left)
                && self.binary_operator_type_is_number_like(right))
            || (self.binary_operator_type_is_bigint_like(left)
                && self.binary_operator_type_is_bigint_like(right))
    }

    fn contextual_unresolved_parameter_marker(name: &str) -> String {
        format!("__tsrs_contextual_unresolved_parameter__{name}")
    }

    pub(crate) fn generic_type_parameter_marker(name: &str) -> String {
        format!("__tsrs_generic_type_parameter__{name}")
    }

    fn contextual_type_is_unresolved_parameter(&self, ty: &Type) -> bool {
        match ty {
            Type::TypeParameter(_) => true,
            Type::TypeReference(name, args) => {
                args.is_empty()
                    && !matches!(
                        name.as_str(),
                        "Object"
                            | "Function"
                            | "String"
                            | "Number"
                            | "Boolean"
                            | "BigInt"
                            | "Symbol"
                    )
                    && !self.class_info.contains_key(name.as_str())
                    && !self.interface_info.contains_key(name.as_str())
                    && !self.enum_info.contains_key(name.as_str())
                    && !self.type_aliases.contains_key(name.as_str())
            }
            _ => false,
        }
    }

    fn addition_operand_is_contextual_recovery(&self, expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Ident(name) => matches!(
                self.nearer_binding_is(&Self::contextual_unresolved_parameter_marker(name), name),
                Some(true)
            ),
            ExprKind::Paren(inner) | ExprKind::NonNull(inner) => {
                self.addition_operand_is_contextual_recovery(inner)
            }
            ExprKind::As(assertion) => {
                self.addition_operand_is_contextual_recovery(&assertion.expr)
            }
            ExprKind::Satisfies(assertion) => {
                self.addition_operand_is_contextual_recovery(&assertion.expr)
            }
            ExprKind::TypeAssertion(assertion) => {
                self.addition_operand_is_contextual_recovery(&assertion.expr)
            }
            _ => false,
        }
    }

    fn addition_operand_is_callable_expando_recovery(&self, expr: &Expr) -> bool {
        let receiver = match &expr.kind {
            ExprKind::Member(member) => &member.object,
            ExprKind::ElemAccess(access) => &access.object,
            ExprKind::Paren(inner) | ExprKind::NonNull(inner) => {
                return self.addition_operand_is_callable_expando_recovery(inner);
            }
            ExprKind::As(assertion) => {
                return self.addition_operand_is_callable_expando_recovery(&assertion.expr);
            }
            ExprKind::Satisfies(assertion) => {
                return self.addition_operand_is_callable_expando_recovery(&assertion.expr);
            }
            ExprKind::TypeAssertion(assertion) => {
                return self.addition_operand_is_callable_expando_recovery(&assertion.expr);
            }
            _ => return false,
        };
        let Some(path) = Self::simple_expression_path(receiver) else {
            return false;
        };
        let root = path.split('.').next().unwrap_or(path.as_str());
        matches!(
            self.lookup_var(root),
            Some(Type::Function(_) | Type::Constructor(_) | Type::Namespace(_) | Type::Module(_))
        ) || matches!(
            self.lookup_var(root),
            Some(Type::ObjectType(info))
                if !info.call_signatures.is_empty() || !info.construct_signatures.is_empty()
        )
    }

    fn report_addition_operator_incompatibility(
        &mut self,
        op: &str,
        left: &Type,
        right: &Type,
        left_expr: &Expr,
        right_expr: &Expr,
        span: Span,
    ) -> bool {
        if self.addition_operand_is_contextual_recovery(left_expr)
            || self.addition_operand_is_contextual_recovery(right_expr)
            || self.addition_operand_is_callable_expando_recovery(left_expr)
            || self.addition_operand_is_callable_expando_recovery(right_expr)
        {
            return false;
        }
        if self.strict_null_checks
            && (self.binary_operator_type_has_unknown(left)
                || self.binary_operator_type_has_unknown(right))
        {
            return false;
        }
        let apparent_left =
            self.widen_type(&self.binary_operator_type_without_strict_nullish(left));
        let apparent_right =
            self.widen_type(&self.binary_operator_type_without_strict_nullish(right));
        if self.addition_types_are_compatible(&apparent_left, &apparent_right) {
            return false;
        }
        // tsc's getBaseTypesIfUnrelated: literal operands print as written
        // when their base types are both plausible `+` operands (number,
        // bigint, string, any); otherwise both widen (`1 + false` reports
        // 'number' and 'boolean').
        let left_written = self.binary_operator_type_without_strict_nullish(left);
        let right_written = self.binary_operator_type_without_strict_nullish(right);
        let close_enough = |ty: &Type| {
            matches!(
                ty,
                Type::Number | Type::BigInt | Type::String | Type::Any | Type::Unknown
            )
        };
        let (left_shown, right_shown) =
            if close_enough(&apparent_left) && close_enough(&apparent_right) {
                (left_written, right_written)
            } else {
                (apparent_left.clone(), apparent_right.clone())
            };
        self.diagnostics.push(error_operator_cannot_be_applied(
            op,
            &left_shown.display_string(),
            &right_shown.display_string(),
            span,
        ));
        true
    }

    fn relational_type_parameter_name<'a>(&self, ty: &'a Type) -> Option<&'a str> {
        match ty {
            Type::TypeParameter(name) => Some(name.as_str()),
            Type::TypeReference(name, args)
                if args.is_empty()
                    && self
                        .lookup_var(&Self::generic_type_parameter_marker(name))
                        .is_some()
                    && !self.class_info.contains_key(name.as_str())
                    && !self.interface_info.contains_key(name.as_str())
                    && !self.enum_info.contains_key(name.as_str())
                    && !self.type_aliases.contains_key(name.as_str()) =>
            {
                Some(name.as_str())
            }
            _ => None,
        }
    }

    fn relational_type_is_top_object(&self, ty: &Type) -> bool {
        match ty {
            Type::Object => true,
            Type::ObjectType(info) => {
                info.properties.is_empty()
                    && info.call_signatures.is_empty()
                    && info.construct_signatures.is_empty()
                    && info.index_signature.is_none()
            }
            Type::TypeReference(name, args) => args.is_empty() && name == "Object",
            _ => false,
        }
    }

    fn relational_type_is_confident(&self, ty: &Type) -> bool {
        match ty {
            Type::Any
            | Type::Unknown
            | Type::Number
            | Type::String
            | Type::Boolean
            | Type::Void
            | Type::Undefined
            | Type::Null
            | Type::Never
            | Type::Object
            | Type::Symbol
            | Type::BigInt
            | Type::NumberLiteral(_)
            | Type::StringLiteral(_)
            | Type::BooleanLiteral(_)
            | Type::BigIntLiteral(_)
            | Type::TypeParameter(_)
            | Type::EnumType(_)
            | Type::EnumVariant { .. }
            | Type::Error => true,
            Type::Union(members) | Type::Intersection(members) => members
                .iter()
                .all(|member| self.relational_type_is_confident(member)),
            Type::Optional(_) => false,
            Type::ObjectType(info) => {
                info.call_signatures.is_empty()
                    && info.construct_signatures.is_empty()
                    && info.properties.iter().all(|(name, property)| {
                        !name.starts_with('?')
                            && !name.ends_with('?')
                            && !matches!(
                                property.as_ref(),
                                Type::Function(_)
                                    | Type::Constructor(_)
                                    | Type::Optional(_)
                                    | Type::TypeParameter(_)
                            )
                    })
                    && info.index_signature.as_ref().is_none_or(|(key, value)| {
                        self.relational_type_is_confident(key)
                            && self.relational_type_is_confident(value)
                    })
            }
            Type::TypeReference(_, _) => {
                self.is_numeric_enum_type(ty)
                    || self.is_string_enum_type(ty)
                    || self
                        .resolve_type_for_assignability(ty)
                        .filter(|resolved| resolved != ty)
                        .is_some_and(|resolved| self.relational_type_is_confident(&resolved))
                    || self.relational_type_parameter_name(ty).is_some()
            }
            Type::TemplateLiteral { .. } => true,
            _ => false,
        }
    }

    fn relational_types_are_compatible(&self, left: &Type, right: &Type) -> bool {
        if left == right
            || self.binary_operator_type_is_dynamic(left)
            || self.binary_operator_type_is_dynamic(right)
            || (self.binary_operator_type_is_numeric_like(left)
                && self.binary_operator_type_is_numeric_like(right))
        {
            return true;
        }

        let left_parameter = self.relational_type_parameter_name(left);
        let right_parameter = self.relational_type_parameter_name(right);
        if left_parameter.is_some() || right_parameter.is_some() {
            if left_parameter.is_some() && right_parameter.is_some() {
                return left_parameter == right_parameter;
            }
            let other = if left_parameter.is_some() {
                right
            } else {
                left
            };
            return !self.binary_operator_type_is_numeric_like(other)
                || self.relational_type_is_top_object(other);
        }

        if (self.binary_operator_type_is_string_like(left)
            && self.binary_operator_type_is_string_like(right))
            || (self.binary_operator_type_is_boolean_like(left)
                && self.binary_operator_type_is_boolean_like(right))
        {
            return true;
        }

        let union_has_string = |ty: &Type| {
            matches!(
                ty,
                Type::Union(members)
                    if members
                        .iter()
                        .any(|member| self.binary_operator_type_is_string_like(member))
            )
        };
        if (self.binary_operator_type_is_string_like(left) && union_has_string(right))
            || (self.binary_operator_type_is_string_like(right) && union_has_string(left))
        {
            return true;
        }

        if let Type::Union(members) = left {
            return !members.is_empty()
                && members
                    .iter()
                    .all(|member| self.relational_types_are_compatible(member, right));
        }
        if let Type::Union(members) = right {
            return !members.is_empty()
                && members
                    .iter()
                    .all(|member| self.relational_types_are_compatible(left, member));
        }

        let left_numeric = self.binary_operator_type_is_numeric_like(left);
        let right_numeric = self.binary_operator_type_is_numeric_like(right);
        if left_numeric != right_numeric {
            return false;
        }

        self.is_assignable_to(left, right) || self.is_assignable_to(right, left)
    }

    fn report_relational_operator_incompatibility(
        &mut self,
        op: &str,
        left: &Type,
        right: &Type,
        span: Span,
    ) -> bool {
        if self.strict_null_checks
            && (self.binary_operator_type_has_unknown(left)
                || self.binary_operator_type_has_unknown(right))
        {
            return false;
        }
        if self.relational_type_has_symbol(left) || self.relational_type_has_symbol(right) {
            return false;
        }
        let apparent_left =
            self.widen_type(&self.binary_operator_type_without_strict_nullish(left));
        let apparent_right =
            self.widen_type(&self.binary_operator_type_without_strict_nullish(right));
        // Two closed object shapes relate exactly as under `==`: tsc's
        // relational rule is comparability in either direction (or both
        // sides numeric), so the no-overlap judgement decides.
        if self.comparable_object_shape(&apparent_left, 0).is_some()
            && self.comparable_object_shape(&apparent_right, 0).is_some()
        {
            if !self.object_shapes_have_no_overlap(&apparent_left, &apparent_right) {
                return false;
            }
            self.diagnostics.push(error_operator_cannot_be_applied(
                op,
                &apparent_left.display_string(),
                &apparent_right.display_string(),
                span,
            ));
            return true;
        }
        if !self.relational_type_is_confident(&apparent_left)
            || !self.relational_type_is_confident(&apparent_right)
        {
            return false;
        }
        if self.relational_types_are_compatible(&apparent_left, &apparent_right) {
            return false;
        }
        self.diagnostics.push(error_operator_cannot_be_applied(
            op,
            &self
                .base_type_for_comparison(&apparent_left)
                .display_string(),
            &self
                .base_type_for_comparison(&apparent_right)
                .display_string(),
            span,
        ));
        true
    }

    /// Whether the non-nullish part of an update operand is accepted by
    /// TypeScript's arithmetic-operand check. Null and undefined are ignored
    /// here because they receive TS18047/TS18048/TS18049 instead; an invalid
    /// sibling still receives TS2356 (`string | null` reports both).
    fn update_operand_is_arithmetic(&self, ty: &Type) -> bool {
        match ty {
            Type::Any
            | Type::Never
            | Type::Number
            | Type::BigInt
            | Type::Null
            | Type::Undefined
            | Type::NumberLiteral(_)
            | Type::BigIntLiteral(_) => true,
            Type::Union(members) => {
                let non_nullish: Vec<_> = members
                    .iter()
                    .filter(|member| !matches!(member, Type::Null | Type::Undefined))
                    .collect();
                non_nullish.is_empty()
                    || non_nullish
                        .into_iter()
                        .all(|member| self.update_operand_is_arithmetic(member))
            }
            // An intersection carrying a numeric constituent is numeric
            // (`number & Brand`). Impossible primitive intersections such as
            // `number & string` reduce to never in tsc and are also accepted.
            Type::Intersection(members) => members
                .iter()
                .any(|member| self.update_operand_is_arithmetic(member)),
            Type::Optional(inner) => self.update_operand_is_arithmetic(inner),
            Type::TypeParameter(_) => {
                self.typeparam_constraint_apparent(ty)
                    .is_some_and(|constraint| {
                        !matches!(constraint, Type::Any | Type::Unknown)
                            && self.update_operand_is_arithmetic(&constraint)
                    })
            }
            Type::TypeReference(_, args) if args.is_empty() => {
                if let Some(constraint) = self.typeparam_constraint_apparent(ty) {
                    return !matches!(constraint, Type::Any | Type::Unknown)
                        && self.update_operand_is_arithmetic(&constraint);
                }
                self.is_numeric_enum_type(ty)
                    || self
                        .resolve_type_for_assignability(ty)
                        .filter(|resolved| resolved != ty)
                        .is_some_and(|resolved| self.update_operand_is_arithmetic(&resolved))
            }
            Type::EnumType(_) | Type::EnumVariant { .. } => self.is_numeric_enum_type(ty),
            _ => false,
        }
    }

    fn update_operand_has_unknown(&self, ty: &Type) -> bool {
        match ty {
            Type::Unknown => true,
            Type::Union(members) => members
                .iter()
                .any(|member| self.update_operand_has_unknown(member)),
            // `unknown & T` reduces to T, so an unknown constituent does not
            // make the intersection itself unknown.
            Type::Intersection(members) => members
                .iter()
                .all(|member| self.update_operand_has_unknown(member)),
            Type::Optional(inner) => self.update_operand_has_unknown(inner),
            _ => false,
        }
    }

    /// Resolve the two enum element-access signatures without making every
    /// dynamic key precise: known member names keep the enum type, while a
    /// numeric (or any) index selects a numeric enum's string reverse map.
    /// Invalid/unknown string keys stay on the existing `any` fallback.
    fn enum_element_access_type(&self, argument: &Expr, index_type: &Type) -> Option<Type> {
        let ExprKind::ElemAccess(access) = &argument.kind else {
            return None;
        };
        let ExprKind::Ident(enum_name) = &access.object.kind else {
            return None;
        };
        let Some(Type::TypeReference(reference, args)) = self.lookup_var(enum_name) else {
            return None;
        };
        if !args.is_empty() || reference != enum_name.as_str() {
            return None;
        }
        let members = self.enum_info.get(enum_name.as_str())?;
        // The cooked literal type wins over raw source text (`"\u{44}"`).
        let member_name = match index_type {
            Type::StringLiteral(name) => Some(name.as_str()),
            _ => match &access.index.kind {
                ExprKind::StrLit(name) if !name.contains('\\') => Some(name.as_str()),
                _ => None,
            },
        };
        // `E["A"]` is the member's own literal type, like `E.A`.
        if let Some((member, value)) =
            member_name.and_then(|name| members.iter().find(|(member, _)| member == name))
        {
            return Some(Type::EnumVariant {
                enum_name: enum_name.to_string(),
                variant_name: member.clone(),
                value: Some(Arc::new(value.clone())),
            });
        }

        let has_numeric_reverse_map = members.is_empty()
            || members
                .iter()
                .any(|(_, value)| matches!(value, Type::Number | Type::NumberLiteral(_)));
        let numeric_index = match index_type {
            Type::Any | Type::Number | Type::NumberLiteral(_) => true,
            // An unresolved identifier is error-typed locally, but tsc
            // continues the enum lookup as `any` and reports TS2356 for the
            // resulting string reverse-map value alongside TS2304.
            Type::Error => matches!(access.index.kind, ExprKind::Ident(_)),
            Type::Union(indices) => indices.iter().all(|index| {
                matches!(index, Type::Number | Type::NumberLiteral(_))
                    || self.is_numeric_enum_type(index)
            }),
            other => self.is_numeric_enum_type(other),
        };
        (has_numeric_reverse_map && numeric_index).then_some(Type::String)
    }

    /// Const/class/enum/namespace bindings and enum members get a more
    /// specific assignment-target diagnostic from tsc, never TS2356.
    /// tsc `checkIdentifier` on an ASSIGNMENT TARGET (`=`, compound, `++`):
    /// a binding that is not a variable reports by its kind — enum, class,
    /// namespace, function (tsc's precedence) — and a constant reports
    /// TS2588. The target then has the error type, so no assignability
    /// diagnostic follows. JS files keep their expando leniency.
    /// The built-in `import.meta` shape (standard + runtime properties).
    fn import_meta_builtin_shape(&self) -> Type {
        Type::ObjectType(ObjectTypeInfo {
            properties: vec![
                ("url".to_string(), Arc::new(Type::String)),
                // Bun/Deno/Node extensions
                ("dir".to_string(), Arc::new(Type::String)),
                ("file".to_string(), Arc::new(Type::String)),
                ("path".to_string(), Arc::new(Type::String)),
                ("dirname".to_string(), Arc::new(Type::String)),
                ("filename".to_string(), Arc::new(Type::String)),
                (
                    "env".to_string(),
                    Arc::new(Type::ObjectType(ObjectTypeInfo {
                        properties: Vec::new(),
                        call_signatures: Vec::new(),
                        construct_signatures: Vec::new(),
                        index_signature: Some((
                            Arc::new(Type::String),
                            Arc::new(Type::Union(vec![Type::String, Type::Undefined].into())),
                        )),
                        index_signature_name: None,
                        method_names: Vec::new(),
                    })),
                ),
                (
                    "resolve".to_string(),
                    Arc::new(Type::Function(FunctionType {
                        type_param_constraints: Vec::new(),
                        params: vec![("specifier".to_string(), Type::String)],
                        return_type: Arc::new(Type::String),
                        type_params: Vec::new(),
                        type_param_defaults: Vec::new(),
                        type_predicate: None,
                    })),
                ),
                (
                    "resolve".to_string(),
                    Arc::new(Type::Function(FunctionType {
                        type_param_constraints: Vec::new(),
                        params: vec![("specifier".to_string(), Type::String)],
                        return_type: Arc::new(Type::String),
                        type_params: Vec::new(),
                        type_param_defaults: Vec::new(),
                        type_predicate: None,
                    })),
                ),
            ],
            call_signatures: Vec::new(),
            construct_signatures: Vec::new(),
            index_signature: None,
            index_signature_name: None,
            method_names: Vec::new(),
        })
    }

    fn report_invalid_assignment_target(&mut self, target: &Expr) -> bool {
        // tsc's getAssignmentTarget walks through parentheses and non-null
        // assertions only: `(Foo as any) = null` is not an assignment to the
        // class binding.
        let mut inner = target;
        loop {
            inner = match &inner.kind {
                ExprKind::Paren(e) | ExprKind::NonNull(e) => e,
                _ => break,
            };
        }
        // `import.meta = x` / `new.target = x`: a meta-property is not a
        // reference (tsc checkReferenceExpression, TS2364); the value is not
        // checked against it.
        if matches!(inner.kind, ExprKind::MetaProp(_)) {
            self.diagnostics.push(Diagnostic {
                code: 2364,
                message: "The left-hand side of an assignment expression must be a variable or a property access.".to_string(),
                category: DiagnosticCategory::Error,
                file_name: None,
                span: Some(target.span),
                related: None,
            });
            return true;
        }
        let ExprKind::Ident(name) = &inner.kind else {
            return false;
        };
        if self.current_file_is_js() {
            return false;
        }
        let binding = self.lookup_var(name).cloned();
        let self_ref = matches!(&binding, Some(Type::TypeReference(n, a))
            if a.is_empty() && (n == name.as_str() || n.strip_prefix("typeof ") == Some(name.as_str())));
        let is_function_decl =
            self.nearer_binding_is(&Self::function_marker_name(name), name) == Some(true);
        let diagnostic = if self_ref && self.enum_info.contains_key(name.as_str()) {
            error_cannot_assign_enum(name, inner.span)
        } else if self_ref && self.class_info.contains_key(name.as_str()) {
            error_cannot_assign_class(name, inner.span)
        } else if matches!(binding, Some(Type::Namespace(_)) | Some(Type::Module(_)))
            || (is_function_decl && self.namespace_paths.contains(name.as_str()))
        {
            crate::diagnostics::error_cannot_assign_binding(2631, name, inner.span)
        } else if is_function_decl {
            crate::diagnostics::error_cannot_assign_binding(2630, name, inner.span)
        } else if self.nearer_binding_is(&Self::const_marker_name(name), name) == Some(true) {
            crate::diagnostics::error_cannot_assign_binding(2588, name, inner.span)
        } else {
            return false;
        };
        if !self
            .diagnostics
            .iter()
            .any(|d| d.span == Some(inner.span) && d.code == diagnostic.code)
        {
            self.diagnostics.push(diagnostic);
        }
        true
    }

    fn update_target_has_specific_assignment_error(&self, argument: &Expr) -> bool {
        match &argument.kind {
            ExprKind::Paren(inner) | ExprKind::NonNull(inner) => {
                self.update_target_has_specific_assignment_error(inner)
            }
            ExprKind::As(expression) => {
                self.update_target_has_specific_assignment_error(&expression.expr)
            }
            ExprKind::Satisfies(expression) => {
                self.update_target_has_specific_assignment_error(&expression.expr)
            }
            ExprKind::TypeAssertion(expression) => {
                self.update_target_has_specific_assignment_error(&expression.expr)
            }
            ExprKind::Ident(name) => {
                if self.nearer_binding_is(&Self::const_marker_name(name), name) == Some(true) {
                    return true;
                }
                match self.lookup_var(name) {
                    Some(Type::TypeReference(reference, args)) if args.is_empty() => {
                        (self.class_info.contains_key(name.as_str())
                            && (reference == name
                                || reference.strip_prefix("typeof ") == Some(name.as_str())))
                            || (self.enum_info.contains_key(name.as_str()) && reference == name)
                    }
                    Some(Type::Namespace(_)) | Some(Type::Module(_)) => true,
                    _ => false,
                }
            }
            ExprKind::Member(member) => {
                let ExprKind::Ident(enum_name) = &member.object.kind else {
                    return false;
                };
                matches!(
                    self.lookup_var(enum_name),
                    Some(Type::TypeReference(reference, args))
                        if args.is_empty() && reference == enum_name.as_str()
                ) && self
                    .enum_info
                    .get(enum_name.as_str())
                    .is_some_and(|members| {
                        members
                            .iter()
                            .any(|(name, _)| name == member.property.as_str())
                    })
            }
            ExprKind::ElemAccess(access) => {
                let ExprKind::Ident(enum_name) = &access.object.kind else {
                    return false;
                };
                let is_enum_binding = matches!(
                    self.lookup_var(enum_name),
                    Some(Type::TypeReference(reference, args))
                        if args.is_empty() && reference == enum_name.as_str()
                );
                if !is_enum_binding {
                    return false;
                }
                let index_type = self.infer_expr_type(&access.index);
                let member_name = match &index_type {
                    Type::StringLiteral(name) => Some(name.as_str()),
                    _ => match &access.index.kind {
                        ExprKind::StrLit(name) if !name.contains('\\') => Some(name.as_str()),
                        _ => None,
                    },
                };
                member_name.is_some_and(|name| {
                    self.enum_info
                        .get(enum_name.as_str())
                        .is_some_and(|members| members.iter().any(|(member, _)| member == name))
                })
            }
            _ => false,
        }
    }

    pub(crate) fn non_global_scope_declares_name(&self, name: &str) -> bool {
        let mut scope = self.current_scope;
        while scope != 0 {
            if self.scopes[scope].vars.contains_key(name) {
                return true;
            }
            let Some(parent) = self.scopes[scope].parent else {
                break;
            };
            scope = parent;
        }
        false
    }

    pub(crate) fn lexical_scope_declares_type_name(&self, name: &str) -> bool {
        let mut scope = Some(self.current_scope);
        while let Some(index) = scope {
            if self.scopes[index].type_names.contains(name) {
                return true;
            }
            scope = self.scopes[index].parent;
        }
        false
    }

    /// VALUE-position twin of [`Self::report_future_lib_global`]: tsc's
    /// name-specific "Cannot find name" variants for well-known globals the
    /// program does not declare — DOM globals missing from the active libs
    /// (TS2584), node globals, which no lib declares (TS2591; JS files keep
    /// their CommonJS leniency) — and TS2585 for a standard global whose
    /// TYPE the libs declare but whose VALUE they do not (`Symbol()` with
    /// only es5). Type positions never take these.
    pub(crate) fn report_value_position_global(&mut self, name: &str, span: Span) -> bool {
        let Some(availability) = stdlib::stdlib_member_availability(&self.compiler_options) else {
            return false;
        };
        // Builtins pre-declare `console`/`document` as variables, so those
        // are gated on the lib model alone; a node global found by scope
        // lookup is a program declaration (`declare function require`).
        let variant_name = match name {
            "console" | "document" => !availability.declares_global(name),
            "process" | "require" | "Buffer" | "module" => {
                !self.current_file_is_js() && self.lookup_var(name).is_none()
            }
            _ => false,
        };
        let type_only = availability.type_only_value_recommendation(name);
        if !variant_name && type_only.is_none() {
            return false;
        }
        if self.local_value_names.contains(name)
            || self.non_global_scope_declares_name(name)
            || self.function_parameter_name_is_active(name)
            || self.user_global_value_names.contains(name)
        {
            return false;
        }
        if !self.diagnostics.iter().any(|diagnostic| {
            matches!(diagnostic.code, 2584 | 2585 | 2591) && diagnostic.span == Some(span)
        }) {
            let diagnostic = match type_only {
                Some(library) if !variant_name => {
                    crate::diagnostics::error_type_only_global_used_as_value(name, library, span)
                }
                _ => crate::diagnostics::error_cannot_find_name_in_expression(name, span),
            };
            self.diagnostics.push(diagnostic);
        }
        true
    }

    /// tsc `checkNonNullType` on a nullish LITERAL operand — the `null`
    /// keyword or the `undefined` identifier — reports TS18050 "The value
    /// 'x' cannot be used here." (a nullish-typed NAME gets TS18047/18048
    /// instead, which is not modeled here). Returns whether it reported, in
    /// which case the operand is an error type for the operator's checks.
    /// tsc `checkNonNullType` on an operand: a nullish LITERAL is TS18050;
    /// otherwise a nullish-typed operand reports like a dereference —
    /// `'y' is possibly 'undefined'` for entity names, "Object is possibly
    /// …" for other expressions (strictNullChecks only, since non-strict
    /// types never carry `undefined`). Returns whether it reported; the
    /// caller then continues with the non-nullable remainder.
    fn report_nullish_operand(&mut self, operand: &Expr, ty: &Type) -> bool {
        if self.report_nullish_literal_operand(operand) {
            return true;
        }
        if !self.strict_null_checks || !self.has_control_flow_narrowing {
            return false;
        }
        if !self.type_contains_null(ty) && !self.type_contains_undefined(ty) {
            return false;
        }
        let before = self.diagnostics.len();
        self.check_nullable_access(ty, operand);
        self.diagnostics.len() > before
    }

    /// tsc `checkDeleteExpression`: the operand (parentheses skipped) must be
    /// a property access (TS2703); under strictNullChecks a deleted declared
    /// property must admit `undefined` (TS2790).
    fn check_delete_operand(&mut self, operand: &Expr) {
        let mut inner = operand;
        while let ExprKind::Paren(e) = &inner.kind {
            inner = e;
        }
        let (object, property): (&Expr, Option<String>) = match &inner.kind {
            ExprKind::Member(member) => (&member.object, Some(member.property.to_string())),
            ExprKind::ElemAccess(access) => (
                &access.object,
                match &access.index.kind {
                    ExprKind::StrLit(name) => Some(name.to_string()),
                    _ => None,
                },
            ),
            _ => {
                // A BARE identifier operand (no parentheses) is also a
                // strict-mode violation (TS1102), reported before the
                // property-reference error.
                if matches!(operand.kind, ExprKind::Ident(_)) {
                    self.diagnostics.push(Diagnostic {
                        code: 1102,
                        message: "'delete' cannot be called on an identifier in strict mode."
                            .to_string(),
                        category: DiagnosticCategory::Error,
                        file_name: None,
                        span: Some(inner.span),
                        related: None,
                    });
                }
                self.diagnostics
                    .push(crate::diagnostics::error_delete_operand(2703, inner.span));
                return;
            }
        };
        if !self.strict_null_checks || self.current_file_is_js() {
            return;
        }
        let Some(property) = property else {
            return;
        };
        if property.starts_with('#') {
            return;
        }
        let object_type = self.infer_expr_type(object);
        // A read-only member reports TS2704 first (tsc checks readonly
        // before optionality).
        let readonly_member = match &object_type {
            Type::TypeReference(name, args) if args.is_empty() => {
                self.class_info
                    .get(name.as_str())
                    .is_some_and(|info| info.readonly_members.contains(property.as_str()))
                    || self
                        .interface_info
                        .get(name.as_str())
                        .is_some_and(|info| info.readonly_members.contains(property.as_str()))
            }
            _ => false,
        };
        if readonly_member {
            self.diagnostics
                .push(crate::diagnostics::error_delete_operand(2704, inner.span));
            return;
        }
        if self.delete_property_is_optional(&object_type, &property, 0) == Some(false) {
            self.diagnostics
                .push(crate::diagnostics::error_delete_operand(2790, inner.span));
        }
    }

    /// `Some(true)` when the declared property admits `undefined` (optional,
    /// nullable, any/unknown/never), `Some(false)` when it is required,
    /// `None` when the receiver shape is unknown or has no such member.
    fn delete_property_is_optional(
        &self,
        receiver: &Type,
        property: &str,
        depth: u8,
    ) -> Option<bool> {
        if depth > 4 {
            return None;
        }
        match receiver {
            Type::ObjectType(info) => {
                let (name, ty) = info
                    .properties
                    .iter()
                    .find(|(n, _)| n.trim_start_matches('?') == property)?;
                if name.starts_with('?') || matches!(ty.as_ref(), Type::Optional(_)) {
                    return Some(true);
                }
                Some(
                    matches!(ty.as_ref(), Type::Any | Type::Unknown | Type::Never)
                        || Self::type_includes_undefined(ty),
                )
            }
            Type::TypeReference(name, args) if args.is_empty() => {
                if self.interface_chain_has_optional_member(name, property, &mut HashSet::new()) {
                    return Some(true);
                }
                if self.class_info.contains_key(name.as_str())
                    || self.interface_info.contains_key(name.as_str())
                {
                    let instance = self.get_class_instance_type(name);
                    return self.delete_property_is_optional(&instance, property, depth + 1);
                }
                None
            }
            Type::Union(members) => {
                let verdicts: Vec<Option<bool>> = members
                    .iter()
                    .filter(|m| !matches!(m, Type::Null | Type::Undefined))
                    .map(|m| self.delete_property_is_optional(m, property, depth + 1))
                    .collect();
                if verdicts.iter().any(|v| *v == Some(false)) {
                    Some(false)
                } else if !verdicts.is_empty() && verdicts.iter().all(|v| *v == Some(true)) {
                    Some(true)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn interface_chain_has_optional_member(
        &self,
        name: &str,
        property: &str,
        seen: &mut HashSet<String>,
    ) -> bool {
        if !seen.insert(name.to_string()) {
            return false;
        }
        let Some(info) = self.interface_info.get(name) else {
            return false;
        };
        info.optional_props.iter().any(|p| p == property)
            || info
                .extends
                .iter()
                .any(|(base, _)| self.interface_chain_has_optional_member(base, property, seen))
    }

    fn report_nullish_literal_operand(&mut self, operand: &Expr) -> bool {
        let value = match &operand.kind {
            ExprKind::NullLit => "null",
            ExprKind::Ident(name) if name == "undefined" => "undefined",
            _ => return false,
        };
        if !self
            .diagnostics
            .iter()
            .any(|d| d.code == 18050 && d.span == Some(operand.span))
        {
            self.diagnostics
                .push(crate::diagnostics::error_value_cannot_be_used_here(
                    value,
                    operand.span,
                ));
        }
        true
    }

    pub(crate) fn report_future_lib_global(&mut self, name: &str, span: Span) -> bool {
        let Some(availability) = stdlib::stdlib_member_availability(&self.compiler_options) else {
            return false;
        };
        let Some(library) = availability.global_recommendation(name) else {
            return false;
        };
        if self.local_value_names.contains(name)
            || self.non_global_scope_declares_name(name)
            || self.lexical_scope_declares_type_name(name)
            || self.local_type_only_names.contains(name)
            || self.imported_type_sources.contains_key(name)
            || self.type_param_name_is_active(name)
            || self.function_parameter_name_is_active(name)
            || self.user_global_type_names.contains(name)
            || self.user_global_value_names.contains(name)
        {
            return false;
        }
        if !self
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == 2583 && diagnostic.span == Some(span))
        {
            self.diagnostics
                .push(error_name_requires_newer_lib(name, library, span));
        }
        true
    }

    fn interface_declares_member(&self, owner: &str, property: &str) -> bool {
        self.interface_info.get(owner).is_some_and(|interface| {
            interface.object_type.properties.iter().any(|(name, _)| {
                name.strip_prefix('?')
                    .or_else(|| name.strip_prefix("..."))
                    .unwrap_or(name)
                    == property
            })
        })
    }

    pub(crate) fn simple_expression_path(expr: &Expr) -> Option<String> {
        match &expr.kind {
            ExprKind::Ident(name) => Some(name.to_string()),
            ExprKind::Member(member) => Some(format!(
                "{}.{}",
                Self::simple_expression_path(&member.object)?,
                member.property
            )),
            ExprKind::Paren(inner) | ExprKind::NonNull(inner) => {
                Self::simple_expression_path(inner)
            }
            ExprKind::As(assertion) => Self::simple_expression_path(&assertion.expr),
            ExprKind::Satisfies(assertion) => Self::simple_expression_path(&assertion.expr),
            ExprKind::TypeAssertion(assertion) => Self::simple_expression_path(&assertion.expr),
            _ => None,
        }
    }

    pub(crate) fn standard_global_path_is_unshadowed(&self, path: &str) -> bool {
        let root = path.split('.').next().unwrap_or(path);
        if (root == "Intl" && self.file_shadows_global_intl)
            || (root == "Symbol" && self.file_shadows_global_symbol)
            || (root == "Array" && self.file_shadows_global_array)
        {
            return false;
        }
        match self.lookup_var(root) {
            None => true,
            Some(Type::Namespace(namespace)) => namespace.name == root,
            Some(Type::Module(_)) => false,
            Some(binding) => {
                if let Some(Type::ObjectType(global)) = self.builtins.global_values.get(root) {
                    binding == &Type::ObjectType(global.clone())
                } else if let Some(statics) = self.builtins.static_members.get(root) {
                    binding == &Type::ObjectType(statics.clone())
                } else {
                    false
                }
            }
        }
    }

    fn receiver_uses_unavailable_standard_value(
        &self,
        receiver: &Expr,
        availability: &stdlib::StdLibMemberAvailability,
    ) -> bool {
        let dependency = match &receiver.kind {
            ExprKind::Call(call) => Some(&call.callee),
            ExprKind::New(new_expression) => Some(&new_expression.callee),
            ExprKind::Member(member) => Some(&member.object),
            ExprKind::Paren(inner) | ExprKind::NonNull(inner) => {
                return self.receiver_uses_unavailable_standard_value(inner, availability);
            }
            ExprKind::As(assertion) => {
                return self
                    .receiver_uses_unavailable_standard_value(&assertion.expr, availability);
            }
            ExprKind::Satisfies(assertion) => {
                return self
                    .receiver_uses_unavailable_standard_value(&assertion.expr, availability);
            }
            ExprKind::TypeAssertion(assertion) => {
                return self
                    .receiver_uses_unavailable_standard_value(&assertion.expr, availability);
            }
            _ => None,
        };
        let Some(dependency) = dependency else {
            return false;
        };
        let Some(path) = Self::simple_expression_path(dependency) else {
            return self.receiver_uses_unavailable_standard_value(dependency, availability);
        };
        let root = path.split('.').next().unwrap_or(path.as_str());
        let standard_root = self.stdlib_value_alias_path(root).unwrap_or(root);
        let standard_path = if standard_root == root {
            path.clone()
        } else {
            path.replacen(root, standard_root, 1)
        };
        for candidate in [standard_path.as_str(), standard_root] {
            if availability.value_is_known(candidate)
                && !availability.value_is_active(candidate)
                && self.standard_global_path_is_unshadowed(standard_root)
            {
                return true;
            }
        }
        false
    }

    fn future_lib_receiver_from_type(
        &self,
        receiver: &Expr,
        ty: &Type,
        availability: &stdlib::StdLibMemberAvailability,
    ) -> Option<FutureLibReceiver> {
        if self.receiver_uses_unavailable_standard_value(receiver, availability) {
            return None;
        }
        if let Some(alias) = Self::simple_expression_path(receiver) {
            if let Some(value_path) = self.stdlib_value_alias_path(&alias) {
                if availability.value_is_active(value_path) {
                    if let Some(owner) = availability.value_owner(value_path) {
                        return Some(FutureLibReceiver {
                            owner: owner.to_string(),
                            display: owner.to_string(),
                        });
                    }
                }
            }
        }
        if matches!(ty, Type::TypeParameter(_))
            || matches!(ty, Type::TypeReference(_, arguments) if arguments.is_empty())
        {
            if let Some(constraint) = self.typeparam_constraint_apparent(ty) {
                let mut constrained =
                    self.future_lib_receiver_from_type(receiver, &constraint, availability)?;
                constrained.display = ty.display_string();
                return Some(constrained);
            }
        }
        let typed = match ty {
            Type::String | Type::StringLiteral(_) => Some(("String".to_string(), None)),
            Type::Symbol | Type::UniqueSymbol(_) => Some(("Symbol".to_string(), None)),
            Type::Array(_) | Type::Tuple(_) => Some(("Array".to_string(), None)),
            Type::TypeReference(name, _) => {
                if self.class_info.contains_key(name.as_str()) {
                    None
                } else {
                    let display = availability.constructor_instance(name).map(str::to_string);
                    Some((name.to_string(), display))
                }
            }
            Type::Namespace(namespace) => {
                let owner = format!("typeof {}", namespace.name);
                Some((owner.clone(), Some(owner)))
            }
            Type::ObjectType(info) => {
                let static_owner = self
                    .builtins
                    .static_members
                    .iter()
                    .find(|(_, builtin)| *builtin == info)
                    .and_then(|(value_name, _)| availability.value_owner(value_name))
                    .map(str::to_string);
                let global_owner =
                    self.builtins
                        .global_values
                        .iter()
                        .find_map(|(value_name, builtin)| match builtin {
                            Type::ObjectType(builtin) if builtin == info => {
                                availability.value_owner(value_name).map(str::to_string)
                            }
                            _ => None,
                        });
                static_owner
                    .or(global_owner)
                    .map(|owner| (owner.clone(), Some(owner)))
            }
            Type::Union(members) => {
                let mut found: Option<FutureLibReceiver> = None;
                for member in members
                    .iter()
                    .filter(|member| !matches!(member, Type::Null | Type::Undefined | Type::Never))
                {
                    let candidate =
                        self.future_lib_receiver_from_type(receiver, member, availability)?;
                    if found
                        .as_ref()
                        .is_some_and(|existing| existing.owner != candidate.owner)
                    {
                        return None;
                    }
                    found = Some(candidate);
                }
                return found;
            }
            _ => None,
        };
        if let Some((owner, display_override)) = typed {
            let user_declares_owner = availability.owner_is_ecmascript(&owner)
                && self
                    .interface_info
                    .get(&owner)
                    .is_some_and(|interface| !interface.decl_file.is_empty());
            if !availability.owner_is_active(&owner) && !user_declares_owner {
                return None;
            }
            return Some(FutureLibReceiver {
                owner,
                display: display_override.unwrap_or_else(|| ty.display_string()),
            });
        }

        // The compact baseline checker does not load the full lib graph, so
        // standard namespace values and their constructor results can be
        // represented as `any`. Recover only declaration-indexed global paths,
        // and reject every lexical/file shadow before doing so.
        if let ExprKind::New(new_expression) = &receiver.kind {
            if let Some(path) = Self::simple_expression_path(&new_expression.callee) {
                if availability.value_is_active(&path)
                    && self.standard_global_path_is_unshadowed(&path)
                {
                    if let Some(display) = availability.constructor_instance(&path) {
                        let owner = display
                            .split('<')
                            .next()
                            .unwrap_or(display)
                            .trim()
                            .to_string();
                        return Some(FutureLibReceiver {
                            owner,
                            display: display.to_string(),
                        });
                    }
                }
            }
        }
        if let ExprKind::Call(call) = &receiver.kind {
            if matches!(&call.callee.kind, ExprKind::Ident(name) if name == "Symbol")
                && availability.value_is_active("Symbol")
                && self.standard_global_path_is_unshadowed("Symbol")
            {
                return Some(FutureLibReceiver {
                    owner: "Symbol".to_string(),
                    display: "symbol".to_string(),
                });
            }
        }
        if let Some(path) = Self::simple_expression_path(receiver) {
            let standard_path = self.stdlib_value_alias_path(&path).unwrap_or(path.as_str());
            if availability.value_is_active(standard_path)
                && self.standard_global_path_is_unshadowed(standard_path)
            {
                if let Some(owner) = availability.value_owner(standard_path) {
                    return Some(FutureLibReceiver {
                        owner: owner.to_string(),
                        display: owner.to_string(),
                    });
                }
            }
        }
        None
    }

    fn report_future_lib_member(
        &mut self,
        receiver: &Expr,
        receiver_type: &Type,
        property: &str,
        property_span: Span,
    ) -> bool {
        let Some(availability) = stdlib::stdlib_member_availability(&self.compiler_options) else {
            return false;
        };
        let Some(receiver_info) =
            self.future_lib_receiver_from_type(receiver, receiver_type, &availability)
        else {
            // A standard namespace can be absent from the compact checker
            // altogether. The declaration index still knows its future
            // exports; this path is guarded against all visible shadows.
            let Some(path) = Self::simple_expression_path(receiver) else {
                return false;
            };
            let owner = format!("typeof {path}");
            if !availability.value_is_active(&path)
                || !self.standard_global_path_is_unshadowed(&path)
            {
                return false;
            }
            let Some(library) = availability.recommendation(&owner, property) else {
                return false;
            };
            if !self
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == 2550 && diagnostic.span == Some(property_span))
            {
                self.diagnostics.push(error_property_requires_newer_lib(
                    property,
                    &owner,
                    library,
                    property_span,
                ));
            }
            return true;
        };
        if self.interface_declares_member(&receiver_info.owner, property) {
            return false;
        }
        let Some(library) = availability.recommendation(&receiver_info.owner, property) else {
            return false;
        };
        if !self
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == 2550 && diagnostic.span == Some(property_span))
        {
            self.diagnostics.push(error_property_requires_newer_lib(
                property,
                &receiver_info.display,
                library,
                property_span,
            ));
        }
        true
    }

    fn constant_property_write_target(target: &Expr) -> Option<(&Expr, &str, Span)> {
        match &target.kind {
            ExprKind::Paren(inner) => Self::constant_property_write_target(inner),
            ExprKind::As(expression) => Self::constant_property_write_target(&expression.expr),
            ExprKind::Satisfies(expression) => {
                Self::constant_property_write_target(&expression.expr)
            }
            ExprKind::TypeAssertion(expression) => {
                Self::constant_property_write_target(&expression.expr)
            }
            ExprKind::NonNull(inner) => Self::constant_property_write_target(inner),
            ExprKind::Member(member) => {
                let end = target.span.end;
                let start = end.saturating_sub(member.property.len() as u32);
                Some((
                    &member.object,
                    member.property.as_str(),
                    Span::new(start, end),
                ))
            }
            ExprKind::ElemAccess(access) => {
                let ExprKind::StrLit(property) = &access.index.kind else {
                    return None;
                };
                Some((&access.object, property.as_str(), access.index.span))
            }
            _ => None,
        }
    }

    pub(super) fn report_readonly_property_write(&mut self, target: &Expr) -> bool {
        let inferred_index;
        let (receiver, property, property_span) = match Self::constant_property_write_target(target)
        {
            Some(target) => target,
            None => {
                let ExprKind::ElemAccess(access) = &target.kind else {
                    return false;
                };
                inferred_index = self.infer_expr_type(&access.index);
                let Type::StringLiteral(property) = &inferred_index else {
                    return false;
                };
                (access.object.as_ref(), property.as_str(), access.index.span)
            }
        };

        let diagnostics_start = self.diagnostics.len();
        let receiver_type = self.check_expr(receiver);
        self.diagnostics.truncate(diagnostics_start);
        let imported_module_export = matches!(
            &receiver_type,
            Type::Module(module)
                if module.exports.iter().any(|(name, _)| name == property)
        );
        let readonly_namespace_export = matches!(
            &receiver_type,
            Type::Namespace(namespace)
                if namespace.readonly_exports.iter().any(|name| name == property)
        );
        let known_virtual_export = match &receiver.kind {
            ExprKind::Ident(binding) => self
                .imported_namespace_export_names
                .get(binding.as_str())
                .is_some_and(|exports| exports.contains(property)),
            _ => false,
        };
        let readonly_namespace_alias_export =
            self.readonly_namespace_alias_receiver_has_export(receiver, property);
        let readonly_local_property = match &receiver.kind {
            ExprKind::Ident(binding) => {
                let marker = Self::readonly_property_marker_name(binding, property);
                self.nearer_binding_is(&marker, binding) == Some(true)
            }
            _ => false,
        };
        // Enum members are read-only properties of the enum object
        // (`E.A = 1`, `++E["A"]`).
        let enum_member = match &receiver.kind {
            ExprKind::Ident(binding) => {
                matches!(
                    self.lookup_var(binding),
                    Some(Type::TypeReference(reference, args))
                        if args.is_empty() && reference == binding.as_str()
                ) && self
                    .enum_info
                    .get(binding.as_str())
                    .is_some_and(|members| members.iter().any(|(name, _)| name == property))
            }
            _ => false,
        };
        if !imported_module_export
            && !readonly_namespace_export
            && !known_virtual_export
            && !readonly_namespace_alias_export
            && !readonly_local_property
            && !enum_member
        {
            return false;
        }

        self.diagnostics
            .push(error_cannot_assign_readonly(property, property_span));
        true
    }

    fn concrete_number_bigint_mismatch(&self, left: &Type, right: &Type) -> bool {
        matches!(
            (self.widen_type(left), self.widen_type(right)),
            (Type::Number, Type::BigInt) | (Type::BigInt, Type::Number)
        )
    }

    fn report_concrete_numeric_operator_incompatibility(
        &mut self,
        op: &str,
        left: &Type,
        right: &Type,
        span: Span,
    ) -> bool {
        let bigint_unsigned_shift = matches!(op, ">>>" | ">>>=")
            && matches!(
                (self.widen_type(left), self.widen_type(right)),
                (Type::BigInt, Type::BigInt)
            );
        if !self.concrete_number_bigint_mismatch(left, right) && !bigint_unsigned_shift {
            return false;
        }
        // Arithmetic operators report the base types of literal operands.
        self.diagnostics.push(error_operator_cannot_be_applied(
            op,
            &self.widen_literals_for_display(left).display_string(),
            &self.widen_literals_for_display(right).display_string(),
            span,
        ));
        true
    }

    pub(crate) fn is_const_assertion_expr(expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Paren(inner) | ExprKind::NonNull(inner) => {
                Self::is_const_assertion_expr(inner)
            }
            ExprKind::Satisfies(satisfies) => Self::is_const_assertion_expr(&satisfies.expr),
            ExprKind::As(assertion) => matches!(&assertion.type_node.kind,
                TypeNodeKind::Reference(type_ref) if matches!(&type_ref.name.kind,
                    ExprKind::Ident(name) if name == "const"
                )
            ),
            ExprKind::TypeAssertion(assertion) => matches!(&assertion.type_node.kind,
                TypeNodeKind::Reference(type_ref) if matches!(&type_ref.name.kind,
                    ExprKind::Ident(name) if name == "const"
                )
            ),
            _ => false,
        }
    }

    pub(crate) fn is_fresh_literal_expr(expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Paren(inner) | ExprKind::NonNull(inner) => Self::is_fresh_literal_expr(inner),
            ExprKind::Satisfies(satisfies) => Self::is_fresh_literal_expr(&satisfies.expr),
            ExprKind::As(_) | ExprKind::TypeAssertion(_) => false,
            ExprKind::StrLit(_)
            | ExprKind::NoSubstTemplate(_)
            | ExprKind::NumLit(_)
            | ExprKind::BigIntLit(_)
            | ExprKind::BoolLit(_)
            | ExprKind::ObjectLit(_)
            | ExprKind::ArrayLit(_) => true,
            _ => false,
        }
    }

    /// The contextual type for one position in a tuple. A trailing rest
    /// element supplies the context for every remaining array-literal item.
    /// The tuple or array shape an array literal is contextually typed by
    /// (tsc isTupleLikeType): tuples and arrays directly, references through
    /// their resolution, tuple-like object shapes (`{ 0: T; 1: U }`) as the
    /// tuple of their indexed properties, a union's tuple (else array)
    /// member, and any such member of an intersection.
    pub(crate) fn tuple_like_context(&self, ctx: &Type, depth: u32) -> Option<Type> {
        if depth > 4 {
            return None;
        }
        match ctx {
            Type::Tuple(_) | Type::Array(_) => Some(ctx.clone()),
            Type::Readonly(inner) => self.tuple_like_context(inner, depth + 1),
            Type::TypeReference(name, args) => {
                if args.is_empty() {
                    if let Some(interface) = self.interface_info.get(name.as_str()) {
                        return Self::tuple_from_indexed_members(&interface.object_type.properties);
                    }
                }
                let resolved = self.resolve_type_for_assignability(ctx)?;
                if resolved == *ctx {
                    return None;
                }
                self.tuple_like_context(&resolved, depth + 1)
            }
            Type::ObjectType(info) => Self::tuple_from_indexed_members(&info.properties),
            Type::Union(members) => members
                .iter()
                .find(|m| matches!(m, Type::Tuple(_)))
                .or_else(|| members.iter().find(|m| matches!(m, Type::Array(_))))
                .cloned()
                .or_else(|| {
                    members
                        .iter()
                        .find_map(|m| self.tuple_like_context(m, depth + 1))
                }),
            Type::Intersection(members) => members
                .iter()
                .find_map(|m| self.tuple_like_context(m, depth + 1)),
            _ => None,
        }
    }

    /// `{ 0: T; 1?: U }` as `[T, U?]`; None without a `0` member.
    fn tuple_from_indexed_members(members: &[(std::string::String, Arc<Type>)]) -> Option<Type> {
        let mut elements = Vec::new();
        loop {
            let index = elements.len().to_string();
            let found = members
                .iter()
                .find(|(name, _)| name.trim_start_matches('?') == index);
            match found {
                Some((name, ty)) if name.starts_with('?') => {
                    elements.push(Type::Optional(Arc::new(Type::clone(ty))))
                }
                Some((_, ty)) => elements.push(Type::clone(ty)),
                None => break,
            }
        }
        (!elements.is_empty()).then(|| Type::Tuple(elements.into()))
    }

    pub(crate) fn tuple_position_type(elements: &[Type], index: usize) -> Option<Type> {
        for (position, element) in elements.iter().enumerate() {
            match element {
                Type::Rest(inner) if index >= position => {
                    return Some(match inner.as_ref() {
                        Type::Array(item) => Type::clone(item),
                        other => other.clone(),
                    });
                }
                Type::Optional(inner) if index == position => {
                    return Some(Type::clone(inner));
                }
                other if index == position => return Some(other.clone()),
                _ => {}
            }
        }
        None
    }

    /// Map one argument position to its contextual parameter type. Rest
    /// parameters keep accepting arguments after their declared slot and use
    /// their array element (or positional tuple element) as the context.
    fn argument_context_type(params: &[(Type, bool)], index: usize) -> Option<Type> {
        let (param_index, (param_ty, is_rest)) = if let Some(param) = params.get(index) {
            (index, param)
        } else {
            params
                .iter()
                .enumerate()
                .rev()
                .find(|(_, (_, rest))| *rest)?
        };

        if !is_rest {
            return Some(param_ty.clone());
        }
        match param_ty {
            Type::Array(element) => Some(Type::clone(element)),
            Type::Tuple(elements) => elements.get(index - param_index).cloned(),
            other => Some(other.clone()),
        }
    }

    /// Whether a type is (or may be) `null` / `undefined` — used to decide
    /// whether an assignment's RHS proves the target non-nullish.
    fn type_is_nullish(ty: &Type) -> bool {
        match ty {
            Type::Null | Type::Undefined | Type::Void => true,
            Type::Union(members) => members.iter().any(Self::type_is_nullish),
            Type::Optional(_) => true,
            _ => false,
        }
    }

    /// Per-argument type for a REST parameter (`...args: T`). The declared
    /// type must be dealiased/unwrapped to its array form before taking the
    /// element type: `...args: LogArgs` where `type LogArgs = readonly any[]`
    /// (or a `readonly T[]` / `ReadonlyArray<T>`) must check each spread
    /// argument against the ELEMENT type, not the whole array (else every
    /// object/string/number argument is wrongly rejected against `readonly
    /// any[]`).
    /// For a rest parameter typed as a tuple, the type accepted at rest
    /// position `k`: the k-th element (an optional element admits
    /// `undefined`), or the element of a trailing rest element beyond the
    /// fixed length. `None` when the parameter is not a plain tuple (unions
    /// of tuples stay lenient through the array path).
    fn rest_tuple_position_type(&self, pty: &Type, k: usize) -> Option<Type> {
        let mut t = pty.clone();
        for _ in 0..4 {
            match &t {
                Type::Readonly(inner) => t = inner.as_ref().clone(),
                Type::TypeReference(..) => match self.resolve_type_for_assignability(&t) {
                    Some(resolved) if resolved != t => t = resolved,
                    _ => break,
                },
                _ => break,
            }
        }
        let Type::Tuple(elements) = &t else {
            return None;
        };
        if let Some(element) = elements.get(k) {
            return Some(match element {
                Type::Optional(inner) => {
                    Type::flatten_union(vec![Type::clone(inner), Type::Undefined])
                }
                Type::Rest(inner) => match inner.as_ref() {
                    Type::Array(item) => Type::clone(item),
                    other => other.clone(),
                },
                other => other.clone(),
            });
        }
        match elements.last() {
            Some(Type::Rest(inner)) => Some(match inner.as_ref() {
                Type::Array(item) => Type::clone(item),
                other => other.clone(),
            }),
            // Beyond a fixed-length tuple: arity reporting decides, the
            // argument itself is unconstrained here.
            _ => Some(Type::Any),
        }
    }

    fn rest_param_element_type(&self, pty: &Type) -> Type {
        let mut t = pty.clone();
        for _ in 0..8 {
            match &t {
                Type::Array(elem) => return elem.as_ref().clone(),
                Type::Readonly(inner) => {
                    let next = inner.as_ref().clone();
                    t = next;
                }
                Type::TypeReference(name, args) => {
                    let bare = name.rsplit('.').next().unwrap_or(name);
                    if (bare == "ReadonlyArray" || bare == "Array") && args.len() == 1 {
                        return args[0].clone();
                    }
                    match self.resolve_type_for_assignability(&t) {
                        Some(r) if r != t => t = r,
                        _ => break,
                    }
                }
                _ => break,
            }
        }
        // Fallback: prior behavior (element of a direct array, else the type).
        match pty {
            Type::Array(elem) => elem.as_ref().clone(),
            other => other.clone(),
        }
    }

    /// The effective type of a parameter for ARGUMENT assignability. Optional
    /// parameters (`p?: T`, or a defaulted `p: T = …`) are encoded with a `?`
    /// name prefix; their call-site type is `T | undefined` — passing
    /// `undefined` (or a `T | undefined` value) is allowed. Rest params keep
    /// their declared type (unwrapped elsewhere). Idempotent when `T` already
    /// admits `undefined`.
    fn optional_param_type(name: &str, t: Type) -> Type {
        if !name.starts_with('?') || Self::type_includes_undefined(&t) {
            return t;
        }
        match t {
            Type::Union(members) => {
                let mut v: Vec<Type> = members.iter().cloned().collect();
                v.push(Type::Undefined);
                Type::Union(v.into())
            }
            other => Type::Union(vec![other, Type::Undefined].into()),
        }
    }

    /// Check every argument exactly once, optionally with a context selected
    /// through the shared fixed/rest positional mapper above.
    fn check_arguments_contextual_once(
        &mut self,
        args: &[Box<Expr>],
        params: Option<&[(Type, bool)]>,
    ) -> Vec<Type> {
        args.iter()
            .enumerate()
            .map(|(index, arg)| {
                let context = params.and_then(|p| Self::argument_context_type(p, index));
                self.check_expr_contextual(arg, context.as_ref())
            })
            .collect()
    }

    /// Classify expressions whose truthiness is fixed by JavaScript syntax,
    /// independently of their inferred type. TypeScript deliberately keeps
    /// `true`, `false`, `0`, and `1` available for idiomatic control-flow, so
    /// those literals are not classified here.
    fn syntactic_truthiness(expr: &Expr) -> Option<SyntacticTruthiness> {
        match &expr.kind {
            ExprKind::Paren(inner) | ExprKind::NonNull(inner) => Self::syntactic_truthiness(inner),
            ExprKind::As(assertion) => Self::syntactic_truthiness(&assertion.expr),
            ExprKind::Satisfies(satisfies) => Self::syntactic_truthiness(&satisfies.expr),
            ExprKind::TypeAssertion(assertion) => Self::syntactic_truthiness(&assertion.expr),
            ExprKind::Instantiation(instantiation) => {
                Self::syntactic_truthiness(&instantiation.expr)
            }
            ExprKind::NullLit | ExprKind::Void(_) => Some(SyntacticTruthiness::Never),
            ExprKind::Ident(name) if name == "undefined" => Some(SyntacticTruthiness::Never),
            ExprKind::StrLit(value) | ExprKind::NoSubstTemplate(value) => {
                if value.is_empty() {
                    Some(SyntacticTruthiness::Never)
                } else {
                    Some(SyntacticTruthiness::Always)
                }
            }
            ExprKind::NumLit(value) if value != "0" && value != "1" => {
                Some(SyntacticTruthiness::Always)
            }
            ExprKind::BigIntLit(value) => {
                if value.trim_end_matches('n') == "0" {
                    Some(SyntacticTruthiness::Never)
                } else {
                    Some(SyntacticTruthiness::Always)
                }
            }
            ExprKind::RegexpLit(_)
            | ExprKind::ArrayLit(_)
            | ExprKind::ObjectLit(_)
            | ExprKind::FnExpr(_)
            | ExprKind::Arrow(_)
            | ExprKind::ClassExpr(_) => Some(SyntacticTruthiness::Always),
            _ => None,
        }
    }

    fn constructability(&self, ty: &Type) -> Constructability {
        self.constructability_inner(ty, 0)
    }

    fn is_standard_intl_constructor(&self, expr: &Expr) -> bool {
        let ExprKind::Member(member) = &expr.kind else {
            return false;
        };
        matches!(
            &member.object.kind,
            ExprKind::Ident(name)
                if name == "Intl"
                    && !self.file_shadows_global_intl
                    && self.lookup_var(name).is_some_and(
                        |ty| matches!(ty, Type::Namespace(_) | Type::Module(_))
                    )
        ) && matches!(
            member.property.as_str(),
            "Collator"
                | "DateTimeFormat"
                | "DisplayNames"
                | "DurationFormat"
                | "ListFormat"
                | "Locale"
                | "NumberFormat"
                | "PluralRules"
                | "RelativeTimeFormat"
                | "Segmenter"
        )
    }

    fn constructability_inner(&self, ty: &Type, depth: usize) -> Constructability {
        if depth >= 16 {
            return Constructability::None;
        }
        match ty {
            Type::Any | Type::Error => Constructability::Dynamic,
            Type::Constructor(constructor) => {
                Constructability::All(vec![vec![constructor.clone()]])
            }
            Type::TypeReference(name, args) => {
                if let Some(class_name) = name.strip_prefix("typeof ") {
                    return self
                        .class_constructor_signatures(class_name)
                        .map(Constructability::Overloads)
                        .unwrap_or(Constructability::None);
                }
                if let Some(constraint) = self.typeparam_constraint_apparent(ty) {
                    return self.constructability_inner(&constraint, depth + 1);
                }
                if let Some(resolved) = self
                    .resolve_type_reference_to_object(name, args)
                    .or_else(|| self.resolve_type_for_assignability(ty))
                {
                    if resolved != *ty {
                        return self.constructability_inner(&resolved, depth + 1);
                    }
                }
                let resolved = self.simplify_type(ty);
                if resolved != *ty {
                    return self.constructability_inner(&resolved, depth + 1);
                }
                // `new A.B.C()`: a namespace-qualified class surfaces as its
                // instance reference; the class itself is constructable.
                if let Some((_, last)) = name.rsplit_once('.') {
                    if let Some(signatures) = self.class_constructor_signatures(last) {
                        return Constructability::Overloads(signatures);
                    }
                }
                // A library interface we hold no structural model for
                // (`Int8ArrayConstructor`, ...) cannot be judged here.
                if !self.interface_info.contains_key(name.as_str())
                    && !self.class_info.contains_key(name.as_str())
                    && !self.type_aliases.contains_key(name.as_str())
                    && stdlib::any_lib_declares_global(name)
                {
                    return Constructability::Dynamic;
                }
                Constructability::None
            }
            Type::Typeof(name) => {
                if let Some(resolved) = self.lookup_var(name) {
                    return self.constructability_inner(resolved, depth + 1);
                }
                self.class_constructor_signatures(name)
                    .map(Constructability::Overloads)
                    .unwrap_or(Constructability::None)
            }
            Type::ObjectType(info) if !info.construct_signatures.is_empty() => {
                Constructability::Overloads(info.construct_signatures.clone())
            }
            Type::Union(members) if !members.is_empty() => {
                let mut groups = Vec::new();
                for member in members.iter() {
                    match self.constructability_inner(member, depth + 1) {
                        Constructability::Dynamic => return Constructability::Dynamic,
                        Constructability::All(mut member_groups) => {
                            groups.append(&mut member_groups);
                        }
                        Constructability::Overloads(member_signatures) => {
                            groups.push(member_signatures);
                        }
                        Constructability::None => return Constructability::None,
                    }
                }
                Constructability::All(groups)
            }
            Type::Intersection(members) if !members.is_empty() => {
                if members.iter().any(|member| matches!(member, Type::Any)) {
                    return Constructability::Dynamic;
                }
                let mut signatures = Vec::new();
                for member in members.iter() {
                    match self.constructability_inner(member, depth + 1) {
                        Constructability::Dynamic => return Constructability::Dynamic,
                        Constructability::All(member_groups) => {
                            for mut member_signatures in member_groups {
                                signatures.append(&mut member_signatures);
                            }
                        }
                        Constructability::Overloads(mut member_signatures) => {
                            signatures.append(&mut member_signatures);
                        }
                        Constructability::None => {}
                    }
                }
                if signatures.is_empty() {
                    Constructability::None
                } else {
                    Constructability::Overloads(signatures)
                }
            }
            Type::TypeParameter(_) => self
                .typeparam_constraint_apparent(ty)
                .map(|constraint| self.constructability_inner(&constraint, depth + 1))
                .unwrap_or(Constructability::None),
            _ => Constructability::None,
        }
    }

    fn validate_constructor_type_args(
        &mut self,
        resolution: &Constructability,
        explicit_type_args: &[Type],
        type_arg_spans: &[Span],
        expression_span: Span,
        callee_display: &str,
    ) -> bool {
        if explicit_type_args.is_empty() {
            return true;
        }
        if let Constructability::All(groups) = resolution {
            let arity_matches = |signature: &ConstructorType| {
                let required = signature
                    .type_params
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| {
                        signature
                            .type_param_defaults
                            .get(*index)
                            .and_then(|default| default.as_ref())
                            .is_none()
                    })
                    .count();
                explicit_type_args.len() >= required
                    && explicit_type_args.len() <= signature.type_params.len()
            };
            let constraints_match = |signature: &ConstructorType| {
                let mut substitutions = HashMap::new();
                for (index, (parameter, actual)) in signature
                    .type_params
                    .iter()
                    .zip(explicit_type_args.iter())
                    .enumerate()
                {
                    if let Some(Some(constraint)) = signature.type_param_constraints.get(index) {
                        let constraint = Self::substitute(constraint, &substitutions);
                        if !self.is_assignable_to(actual, &constraint) {
                            return false;
                        }
                    }
                    substitutions.insert(parameter.clone(), actual.clone());
                }
                true
            };
            if groups.len() > 1
                && groups.iter().all(|group| group.iter().any(arity_matches))
                && groups.iter().any(|group| {
                    !group
                        .iter()
                        .any(|signature| arity_matches(signature) && constraints_match(signature))
                })
            {
                self.diagnostics
                    .push(error_not_constructable(callee_display, expression_span));
                return false;
            }
        }
        let signatures = match resolution {
            Constructability::All(groups) => groups.iter().flatten().collect::<Vec<_>>(),
            Constructability::Overloads(signatures) => signatures.iter().collect(),
            Constructability::Dynamic | Constructability::None => return true,
        };
        let arity_matches = signatures
            .iter()
            .copied()
            .filter(|signature| {
                let required = signature
                    .type_param_defaults
                    .iter()
                    .enumerate()
                    .filter(|(index, default)| {
                        *index < signature.type_params.len() && default.is_none()
                    })
                    .count()
                    + signature
                        .type_params
                        .len()
                        .saturating_sub(signature.type_param_defaults.len());
                explicit_type_args.len() >= required
                    && explicit_type_args.len() <= signature.type_params.len()
            })
            .collect::<Vec<_>>();
        if arity_matches.is_empty() {
            if let Some(signature) = signatures.first() {
                let required = signature
                    .type_params
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| {
                        signature
                            .type_param_defaults
                            .get(*index)
                            .and_then(|default| default.as_ref())
                            .is_none()
                    })
                    .count();
                self.diagnostics.push(error_type_arg_count(
                    required,
                    signature.type_params.len(),
                    explicit_type_args.len(),
                    expression_span,
                ));
            }
            return false;
        }

        let mut first_violation = None;
        for signature in arity_matches {
            let mut substitutions = HashMap::new();
            let mut violation = None;
            for (index, (parameter, actual)) in signature
                .type_params
                .iter()
                .zip(explicit_type_args.iter())
                .enumerate()
            {
                if let Some(Some(constraint)) = signature.type_param_constraints.get(index) {
                    let effective_constraint = Self::substitute(constraint, &substitutions);
                    if !self.is_assignable_to(actual, &effective_constraint) {
                        violation = Some((
                            actual.display_string(),
                            effective_constraint.display_string(),
                            type_arg_spans
                                .get(index)
                                .copied()
                                .unwrap_or(expression_span),
                        ));
                        break;
                    }
                }
                substitutions.insert(parameter.clone(), actual.clone());
            }
            if violation.is_none() {
                return true;
            }
            if first_violation.is_none() {
                first_violation = violation;
            }
        }
        if let Some((actual, constraint, span)) = first_violation {
            self.diagnostics
                .push(error_type_arg_constraint(&actual, &constraint, span));
            return false;
        }
        true
    }

    fn instantiate_constructor_signature(
        &self,
        signature: &ConstructorType,
        explicit_type_args: &[Type],
        argument_types: &[Type],
        contextual_return: Option<&Type>,
    ) -> ConstructorType {
        if signature.type_params.is_empty() {
            return signature.clone();
        }
        let mut substitutions: HashMap<std::string::String, Type> = if explicit_type_args.is_empty()
        {
            // Type arguments inferred from fresh literal arguments widen
            // (`new Q(1, "a")` is `Q<number, string>`).
            let widened: Vec<Type> = argument_types
                .iter()
                .map(|t| self.widen_argument_for_inference(t))
                .collect();
            self.infer_type_arguments_for_call(&widened, &signature.params, &signature.type_params)
        } else {
            signature
                .type_params
                .iter()
                .zip(explicit_type_args.iter())
                .map(|(name, ty)| (name.clone(), ty.clone()))
                .collect()
        };
        // The contextual type of the `new` expression infers what the
        // arguments left unsolved (tsc: return-type priority).
        if let Some(contextual) = contextual_return {
            let contextual = match contextual {
                Type::Optional(inner) => Type::clone(inner),
                other => other.clone(),
            };
            let unsolved: Vec<std::string::String> = signature
                .type_params
                .iter()
                .filter(|name| {
                    !matches!(substitutions.get(*name), Some(ty) if !matches!(ty, Type::Unknown))
                })
                .cloned()
                .collect();
            if !unsolved.is_empty() {
                let from_return = self.infer_type_arguments_for_call(
                    std::slice::from_ref(&contextual),
                    &[("__return".to_string(), Type::clone(&signature.return_type))],
                    &signature.type_params,
                );
                for name in unsolved {
                    if let Some(candidate) = from_return.get(&name) {
                        if !matches!(candidate, Type::Unknown) {
                            substitutions.insert(name, candidate.clone());
                        }
                    }
                }
            }
        }
        for (index, parameter) in signature.type_params.iter().enumerate() {
            if substitutions.contains_key(parameter) {
                continue;
            }
            let fallback = signature
                .type_param_defaults
                .get(index)
                .and_then(|default| default.as_ref())
                .map(|default| Self::substitute(default, &substitutions))
                .unwrap_or(Type::Unknown);
            substitutions.insert(parameter.clone(), fallback);
        }
        for (index, parameter) in signature.type_params.iter().enumerate() {
            let Some(Some(constraint)) = signature.type_param_constraints.get(index) else {
                continue;
            };
            let effective_constraint = Self::substitute(constraint, &substitutions);
            let violates_constraint = substitutions
                .get(parameter)
                .is_some_and(|actual| !self.is_assignable_to(actual, &effective_constraint));
            if explicit_type_args.is_empty() && violates_constraint {
                substitutions.insert(parameter.clone(), effective_constraint);
            }
        }
        ConstructorType {
            is_abstract: signature.is_abstract,
            params: signature
                .params
                .iter()
                .map(|(name, ty)| (name.clone(), Self::substitute(ty, &substitutions)))
                .collect(),
            return_type: Arc::new(Self::substitute(&signature.return_type, &substitutions)),
            type_params: Vec::new(),
            type_param_constraints: Vec::new(),
            type_param_defaults: Vec::new(),
        }
    }

    fn constructor_signature_accepts(
        &self,
        signature: &ConstructorType,
        argument_types: &[Type],
    ) -> bool {
        let required = crate::constructor_required_count(signature);
        if argument_types.len() < required
            || crate::constructor_max_count(signature)
                .is_some_and(|maximum| argument_types.len() > maximum)
        {
            return false;
        }
        argument_types.iter().enumerate().all(|(index, argument)| {
            let Some(target) = crate::constructor_parameter_at(signature, index) else {
                return false;
            };
            self.is_assignable_to(argument, &target)
        })
    }

    /// tsc getErrorSpanForNode for an expression reported as a whole: an
    /// anonymous function or class expression marks only its first token
    /// (`function`, `async`, `class`), a named one its name, and an arrow
    /// whose block body spans several lines marks only its first line.
    pub(crate) fn error_span_for_expr(&self, expr: &Expr) -> Span {
        let source = self.current_source.as_deref();
        let first_token = |start: u32| -> Span {
            let len = source
                .and_then(|src| src.get(start as usize..))
                .map(|rest| rest.chars().take_while(|c| c.is_ascii_alphabetic()).count() as u32)
                .filter(|len| *len > 0)
                .unwrap_or(1);
            Span::new(start, start + len)
        };
        match &expr.kind {
            ExprKind::FnExpr(function) => match function.name_span {
                Some(name_span) => name_span,
                None => first_token(expr.span.start),
            },
            ExprKind::ClassExpr(class) => match class.name_span {
                Some(name_span) => name_span,
                None => first_token(expr.span.start),
            },
            ExprKind::Arrow(arrow) if matches!(arrow.body, ArrowBody::Block(_)) => {
                let Some(src) = source else {
                    return expr.span;
                };
                let (start, end) = (expr.span.start as usize, expr.span.end as usize);
                let Some(text) = src.get(start..end) else {
                    return expr.span;
                };
                let Some(arrow_at) = text.find("=>") else {
                    return expr.span;
                };
                match text[arrow_at..].find('\n') {
                    Some(newline) => {
                        let mut line_end = start + arrow_at + newline;
                        if src.as_bytes().get(line_end.wrapping_sub(1)) == Some(&b'\r') {
                            line_end -= 1;
                        }
                        Span::new(expr.span.start, line_end as u32)
                    }
                    None => expr.span,
                }
            }
            _ => expr.span,
        }
    }

    /// TS6210 note for a constructor call missing arguments: the first
    /// parameter (own or inherited constructor) left without one.
    fn constructor_missing_argument_note(
        &self,
        class_name: Option<&str>,
        provided: usize,
    ) -> Option<Vec<tsc_rs_ast::RelatedDiagnostic>> {
        let (param_name, param_span) = class_name
            .and_then(|name| self.function_param_spans.get(name))
            .and_then(|params| params.get(provided))?;
        Some(vec![self.missing_argument_note(param_name, *param_span)])
    }

    /// TS6210 for a named parameter, TS6211 for a binding pattern (recorded
    /// with an empty name).
    fn missing_argument_note(
        &self,
        param_name: &str,
        param_span: Span,
    ) -> tsc_rs_ast::RelatedDiagnostic {
        if param_name.is_empty() {
            tsc_rs_ast::RelatedDiagnostic {
                code: 6211,
                message: "An argument matching this binding pattern was not provided.".to_string(),
                file_name: self.current_file_name.clone(),
                span: Some(param_span),
            }
        } else {
            tsc_rs_ast::RelatedDiagnostic {
                code: 6210,
                message: format!("An argument for '{}' was not provided.", param_name),
                file_name: self.current_file_name.clone(),
                span: Some(param_span),
            }
        }
    }

    fn report_constructor_signature_mismatch(
        &mut self,
        signature: &ConstructorType,
        argument_types: &[Type],
        arguments: Option<&[Box<Expr>]>,
        expression_span: Span,
    ) {
        let required = crate::constructor_required_count(signature);
        let max = crate::constructor_max_count(signature).unwrap_or(usize::MAX);
        if argument_types.len() < required || argument_types.len() > max {
            let mut diagnostic =
                error_arg_count(required, max, argument_types.len(), expression_span);
            if argument_types.len() < required {
                let name = self.pending_new_callee_name.clone();
                diagnostic.related =
                    self.constructor_missing_argument_note(name.as_deref(), argument_types.len());
            }
            self.diagnostics.push(diagnostic);
            return;
        }
        for (index, argument) in argument_types.iter().enumerate() {
            let Some(target) = crate::constructor_parameter_at(signature, index) else {
                break;
            };
            if !self.is_assignable_to(argument, &target) {
                if let Some(expression) = arguments.and_then(|args| args.get(index)) {
                    let old = std::mem::replace(&mut self.elaborating_argument, true);
                    let elaborated = self.elaborate_error(expression, argument, &target, None);
                    self.elaborating_argument = old;
                    if elaborated {
                        return;
                    }
                }
                let span = arguments
                    .and_then(|args| args.get(index))
                    .map(|argument| self.error_span_for_expr(argument))
                    .unwrap_or(expression_span);
                self.diagnostics.push(error_arg_not_assignable(
                    &self
                        .widen_for_message(
                            arguments
                                .and_then(|args| args.get(index))
                                .map(|arg| arg.as_ref()),
                            argument,
                            &target,
                        )
                        .display_string(),
                    &target.display_string(),
                    span,
                ));
                return;
            }
        }
    }

    fn report_constructor_overload_mismatch(
        &mut self,
        signatures: &[ConstructorType],
        argument_types: &[Type],
        arguments: Option<&[Box<Expr>]>,
        expression_span: Span,
    ) {
        let candidates: Vec<_> = signatures
            .iter()
            .filter(|signature| {
                argument_types.len() >= crate::constructor_required_count(signature)
                    && crate::constructor_max_count(signature)
                        .is_none_or(|max| argument_types.len() <= max)
            })
            .collect();
        if candidates.len() > 1 {
            let functions = candidates
                .iter()
                .map(|signature| FunctionType {
                    params: signature.params.clone(),
                    return_type: signature.return_type.clone(),
                    type_params: signature.type_params.clone(),
                    type_param_constraints: signature.type_param_constraints.clone(),
                    type_param_defaults: signature.type_param_defaults.clone(),
                    type_predicate: None,
                })
                .collect::<Vec<_>>();
            self.diagnostics.push(self.no_overload_arguments(
                &functions,
                argument_types,
                arguments.unwrap_or_default(),
                expression_span,
            ));
        } else if candidates.is_empty() {
            let required = signatures
                .iter()
                .map(crate::constructor_required_count)
                .min()
                .unwrap_or(0);
            let maximum = signatures
                .iter()
                .map(crate::constructor_max_count)
                .collect::<Option<Vec<_>>>()
                .and_then(|counts| counts.into_iter().max());
            let below = signatures
                .iter()
                .filter_map(crate::constructor_max_count)
                .filter(|count| *count < argument_types.len())
                .max();
            let above = signatures
                .iter()
                .map(crate::constructor_required_count)
                .filter(|count| *count > argument_types.len())
                .min();
            let diagnostic = if let (Some(below), Some(above)) = (below, above) {
                Diagnostic {
                    code: 2575,
                    message: format!(
                        "No overload expects {} arguments, but overloads do exist that expect either {below} or {above} arguments.",
                        argument_types.len()
                    ),
                    category: DiagnosticCategory::Error,
                    file_name: None,
                    span: Some(expression_span),
                    related: None,
                }
            } else {
                error_arg_count(
                    required,
                    maximum.unwrap_or(usize::MAX),
                    argument_types.len(),
                    expression_span,
                )
            };
            self.diagnostics.push(diagnostic);
        } else if let Some(signature) = candidates.first().copied().or_else(|| signatures.first()) {
            self.report_constructor_signature_mismatch(
                signature,
                argument_types,
                arguments,
                expression_span,
            );
        }
    }

    fn check_constructability(
        &mut self,
        resolution: Constructability,
        explicit_type_args: &[Type],
        argument_types: &[Type],
        arguments: Option<&[Box<Expr>]>,
        expression_span: Span,
        contextual_return: Option<&Type>,
    ) -> Option<Type> {
        match resolution {
            Constructability::Dynamic => Some(Type::Any),
            Constructability::None => None,
            Constructability::All(groups) => {
                let mut returns = Vec::new();
                for group in groups {
                    let instantiated: Vec<_> = group
                        .iter()
                        .map(|signature| {
                            self.instantiate_constructor_signature(
                                signature,
                                explicit_type_args,
                                argument_types,
                                contextual_return,
                            )
                        })
                        .collect();
                    if let Some(selected) = instantiated.iter().find(|signature| {
                        self.constructor_signature_accepts(signature, argument_types)
                    }) {
                        returns.push(Type::clone(&selected.return_type));
                    } else if let Some(first) = instantiated.first() {
                        self.report_constructor_signature_mismatch(
                            first,
                            argument_types,
                            arguments,
                            expression_span,
                        );
                        returns.push(Type::clone(&first.return_type));
                    }
                }
                Some(Type::flatten_union(returns))
            }
            Constructability::Overloads(signatures) => {
                let instantiated: Vec<_> = signatures
                    .iter()
                    .map(|signature| {
                        self.instantiate_constructor_signature(
                            signature,
                            explicit_type_args,
                            argument_types,
                            contextual_return,
                        )
                    })
                    .collect();
                if let Some(selected) = instantiated
                    .iter()
                    .find(|signature| self.constructor_signature_accepts(signature, argument_types))
                {
                    return Some(Type::clone(&selected.return_type));
                }
                if let Some(first) = instantiated.first() {
                    if instantiated.len() > 1 {
                        self.report_constructor_overload_mismatch(
                            &instantiated,
                            argument_types,
                            arguments,
                            expression_span,
                        );
                    } else {
                        self.report_constructor_signature_mismatch(
                            first,
                            argument_types,
                            arguments,
                            expression_span,
                        );
                    }
                    Some(Type::clone(&first.return_type))
                } else {
                    None
                }
            }
        }
    }

    /// A type that is falsy for EVERY value it contains — safe to drop from the
    /// truthy branch of `||` / a truthiness guard. Only the literal falsy types
    /// qualify; wide `string`/`number`/`boolean` are NOT always-falsy (they
    /// contain truthy values) and are kept as-is, matching tsc.
    fn is_always_falsy(ty: &Type) -> bool {
        match ty {
            Type::Null | Type::Undefined | Type::Never => true,
            Type::BooleanLiteral(false) => true,
            Type::StringLiteral(s) => s.is_empty(),
            Type::NumberLiteral(n) => n.parse::<f64>().map(|v| v == 0.0).unwrap_or(false),
            Type::BigIntLiteral(n) => n
                .trim_end_matches('n')
                .parse::<i128>()
                .map(|v| v == 0)
                .unwrap_or(false),
            _ => false,
        }
    }

    fn check_syntactic_truthiness(&mut self, expr: &Expr) {
        let (code, message) = match Self::syntactic_truthiness(expr) {
            Some(SyntacticTruthiness::Always) => {
                (2872, "This kind of expression is always truthy.")
            }
            Some(SyntacticTruthiness::Never) => (2873, "This kind of expression is always falsy."),
            None => return,
        };
        // tsc's error span for a function / arrow / class expression is its
        // declaration NAME when it has one (`function named() {} || x`), and
        // otherwise its FIRST TOKEN (`function`, `async`, `class`, `(`).
        let first_token = |start: u32| -> Span {
            let len = self
                .current_source
                .as_deref()
                .and_then(|source| source.get(start as usize..))
                .map(|rest| {
                    let word = rest
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
                        .count();
                    if word == 0 {
                        1
                    } else {
                        word
                    }
                })
                .unwrap_or(1) as u32;
            Span::new(start, start + len)
        };
        let span = match &expr.kind {
            ExprKind::FnExpr(fn_decl) => fn_decl
                .name_span
                .unwrap_or_else(|| first_token(expr.span.start)),
            ExprKind::ClassExpr(class_decl) => class_decl
                .name_span
                .unwrap_or_else(|| first_token(expr.span.start)),
            ExprKind::Arrow(_) => first_token(expr.span.start),
            _ => expr.span,
        };
        self.diagnostics.push(Diagnostic {
            code,
            message: message.to_string(),
            category: DiagnosticCategory::Error,
            file_name: None,
            span: Some(span),
            related: None,
        });
    }

    pub(crate) fn check_decorator_expressions(&mut self, decorators: &[Expr]) {
        for decorator in decorators {
            let mut expression = decorator.clone();
            if self.current_source.as_ref().is_some_and(|source| {
                source.as_bytes().get(expression.span.start as usize) == Some(&b'@')
            }) {
                expression.span.start = expression.span.start.saturating_add(1);
            }
            self.check_expr(&expression);
        }
    }

    pub(crate) fn current_file_is_javascript(&self) -> bool {
        self.current_file_name.as_ref().is_some_and(|file_name| {
            let file_name = file_name.to_ascii_lowercase();
            file_name.ends_with(".js")
                || file_name.ends_with(".jsx")
                || file_name.ends_with(".mjs")
                || file_name.ends_with(".cjs")
        })
    }

    fn decorator_at_sign_start(&self, decorator: &Expr) -> u32 {
        let mut start = decorator.span.start;
        if self
            .current_source
            .as_ref()
            .is_some_and(|source| source.as_bytes().get(start as usize) != Some(&b'@'))
            && start > 0
            && self
                .current_source
                .as_ref()
                .is_some_and(|source| source.as_bytes().get(start as usize - 1) == Some(&b'@'))
        {
            start -= 1;
        }
        start
    }

    fn report_invalid_decorators(&mut self, decorators: &[Expr]) {
        let Some(decorator) = decorators.first() else {
            return;
        };
        let start = self.decorator_at_sign_start(decorator);
        let first_token_span = Span::new(start, start.saturating_add(1));
        let is_js = self.current_file_is_javascript();
        let check_js = self.compiler_options.check_js == Some(true);
        let mut push = |span| {
            self.diagnostics.push(Diagnostic {
                code: 1206,
                message: "Decorators are not valid here.".to_string(),
                category: DiagnosticCategory::Error,
                file_name: None,
                span: Some(span),
                related: None,
            });
        };
        if is_js {
            // JavaScript reports the option-independent syntactic diagnostic
            // on the whole decorator. With checkJs, the semantic grammar pass
            // also reports the first-token form before it.
            if check_js {
                push(first_token_span);
            }
            push(Span::new(
                start,
                decorator.span.end.max(start.saturating_add(1)),
            ));
        } else {
            push(first_token_span);
        }
    }

    fn report_js_syntactic_invalid_decorator(&mut self, decorators: &[Expr]) {
        let Some(decorator) = decorators.first() else {
            return;
        };
        if !self.current_file_is_javascript() {
            return;
        }
        let start = self.decorator_at_sign_start(decorator);
        self.diagnostics.push(Diagnostic {
            code: 1206,
            message: "Decorators are not valid here.".to_string(),
            category: DiagnosticCategory::Error,
            file_name: None,
            span: Some(Span::new(
                start,
                decorator.span.end.max(start.saturating_add(1)),
            )),
            related: None,
        });
    }

    fn report_decorator_on_method_overload(&mut self, decorators: &[Expr]) {
        let Some(decorator) = decorators.first() else {
            return;
        };
        if self.current_file_is_javascript() && self.compiler_options.check_js != Some(true) {
            return;
        }
        let start = self.decorator_at_sign_start(decorator);
        self.diagnostics.push(Diagnostic {
            code: 1249,
            message: "A decorator can only decorate a method implementation, not an overload."
                .to_string(),
            category: DiagnosticCategory::Error,
            file_name: None,
            span: Some(Span::new(start, start.saturating_add(1))),
            related: None,
        });
    }

    pub(crate) fn check_js_discarded_statement_decorator(&mut self, statement: &Stmt) {
        if !self.current_file_is_javascript() || self.compiler_options.check_js != Some(true) {
            return;
        }
        let start = statement.span.start;
        if !self
            .current_source
            .as_ref()
            .is_some_and(|source| source.as_bytes().get(start as usize) == Some(&b'@'))
        {
            return;
        }
        let invalid = match &statement.kind {
            StmtKind::Var(variable) => variable.declarations.iter().any(|declaration| {
                !matches!(&declaration.name.kind, PatKind::Ident(name) if name == "<error>")
                    || self.current_source.as_ref().is_some_and(|source| {
                        declaration.name.span.start as usize >= source.len()
                            || source.as_bytes().get(declaration.name.span.start as usize)
                                == Some(&b';')
                    })
            }),
            StmtKind::Expr(expression) if matches!(&expression.kind, ExprKind::Ident(name) if name == "let" || name == "using") => {
                self.current_source.as_ref().is_some_and(|source| {
                    source
                        .get(expression.span.end as usize..)
                        .is_some_and(|suffix| {
                            suffix.trim_start().is_empty() || suffix.trim_start().starts_with(';')
                        })
                })
            }
            StmtKind::InterfaceDecl(_)
            | StmtKind::TypeAlias(_)
            | StmtKind::EnumDecl(_)
            | StmtKind::ModuleDecl(_)
            | StmtKind::Import(_)
            | StmtKind::ImportEquals(_) => true,
            StmtKind::ExportAssign(_) => true,
            StmtKind::Export(export) => match &export.kind {
                ExportDeclKind::Decl(declaration) | ExportDeclKind::DefaultDecl(declaration) => {
                    !matches!(declaration.kind, StmtKind::ClassDecl(_))
                }
                ExportDeclKind::Default(_)
                | ExportDeclKind::Named { .. }
                | ExportDeclKind::All { .. } => true,
            },
            _ => false,
        };
        if invalid {
            self.diagnostics.push(Diagnostic {
                code: 1206,
                message: "Decorators are not valid here.".to_string(),
                category: DiagnosticCategory::Error,
                file_name: None,
                span: Some(Span::new(start, start.saturating_add(1))),
                related: None,
            });
        }
    }

    pub(crate) fn report_js_checked_export_decorator_position(
        &mut self,
        declaration_span: Span,
        decorators: &[Expr],
    ) {
        if !self.current_file_is_javascript() || self.compiler_options.check_js != Some(true) {
            return;
        }
        let (Some(first), Some(last), Some(source)) = (
            decorators.first(),
            decorators.last(),
            self.current_source.as_deref(),
        ) else {
            return;
        };
        let first_start = self.decorator_at_sign_start(first);
        if first_start < declaration_span.start || last.span.end > declaration_span.end {
            return;
        }
        let Some(prefix) = source.get(declaration_span.start as usize..first_start as usize) else {
            return;
        };
        let Some(suffix) = source.get(last.span.end as usize..declaration_span.end as usize) else {
            return;
        };
        if prefix.trim() != "export" || !suffix.trim_start().starts_with("default") {
            return;
        }
        self.diagnostics.push(Diagnostic {
            code: 1206,
            message: "Decorators are not valid here.".to_string(),
            category: DiagnosticCategory::Error,
            file_name: None,
            span: Some(Span::new(
                first_start,
                first.span.end.max(first_start.saturating_add(1)),
            )),
            related: None,
        });
    }

    fn report_js_checked_decorated_static_block(&mut self, member_span: Span) {
        if self.current_file_is_javascript()
            && self.compiler_options.check_js == Some(true)
            && self.current_source.as_ref().is_some_and(|source| {
                source.as_bytes().get(member_span.start as usize) == Some(&b'@')
            })
        {
            self.diagnostics.push(Diagnostic {
                code: 1206,
                message: "Decorators are not valid here.".to_string(),
                category: DiagnosticCategory::Error,
                file_name: None,
                span: Some(Span::new(
                    member_span.start,
                    member_span.start.saturating_add(1),
                )),
                related: None,
            });
        }
    }

    pub(crate) fn check_decorators_with_validity(&mut self, decorators: &[Expr], valid: bool) {
        if valid {
            self.check_decorator_expressions(decorators);
        } else {
            self.report_invalid_decorators(decorators);
        }
    }

    pub(crate) fn check_parameter_decorators(&mut self, parameters: &[Param], valid: bool) {
        for parameter in parameters {
            let is_this_parameter =
                matches!(&parameter.name.kind, PatKind::Ident(name) if name == "this");
            let is_missing_parameter =
                matches!(&parameter.name.kind, PatKind::Ident(name) if name == "<error>");
            // Decorators on `this` parameters use TS1433 regardless of the
            // surrounding function kind. Do not replace that higher-priority
            // grammar diagnostic with TS1206.
            if is_missing_parameter {
                self.report_js_syntactic_invalid_decorator(&parameter.decorators);
            } else if is_this_parameter {
                if self.compiler_options.experimental_decorators != Some(true) {
                    self.report_js_syntactic_invalid_decorator(&parameter.decorators);
                }
            } else {
                self.check_decorators_with_validity(&parameter.decorators, valid);
            }
        }
    }

    pub(crate) fn check_arrow_parameter_decorators(&mut self, parameters: &[Param]) {
        for parameter in parameters {
            self.report_js_syntactic_invalid_decorator(&parameter.decorators);
        }
    }

    pub(crate) fn check_parameter_runtime_expressions(
        &mut self,
        parameters: &[Param],
        check_initializers: bool,
        check_legacy_decorators: bool,
    ) {
        // Initializers may reference parameters declared before them; those
        // names are not bound yet at this point, so they are recorded as
        // provisional and resolve to `any` instead of TS2304.
        let saved_provisional = std::mem::take(&mut self.provisional_param_names);
        // TS2300: a parameter name declared twice in one list is reported at
        // EVERY occurrence (tsc's binder reports each declaration).
        {
            let mut all: Vec<(std::string::String, Span)> = Vec::new();
            for parameter in parameters {
                Self::pattern_binding_spans(&parameter.name, &mut all);
            }
            for (name, span) in &all {
                if name != "_"
                    && all.iter().filter(|(n, _)| n == name).count() > 1
                    && self
                        .reported_duplicate_spans
                        .insert((2300, span.start, span.end))
                {
                    self.diagnostics
                        .push(error_duplicate_identifier(name, *span));
                }
            }
        }
        // Every parameter of the list is in scope for every initializer
        // (later ones through deferred contexts, or TS2373 — not TS2304).
        for parameter in parameters {
            let mut bound = Vec::new();
            Self::pattern_binding_spans(&parameter.name, &mut bound);
            self.provisional_param_names
                .extend(bound.into_iter().map(|(name, _)| name));
        }
        let param_names: Vec<Vec<std::string::String>> = parameters
            .iter()
            .map(|parameter| {
                let mut bound = Vec::new();
                Self::pattern_binding_spans(&parameter.name, &mut bound);
                bound.into_iter().map(|(name, _)| name).collect()
            })
            .collect();
        for (index, parameter) in parameters.iter().enumerate() {
            if check_legacy_decorators {
                self.check_decorator_expressions(&parameter.decorators);
            }
            if check_initializers {
                self.check_binding_pattern_runtime_expressions(&parameter.name);
                if let Some(initializer) = &parameter.initializer {
                    // TS2372/TS2373: an initializer that immediately reads its
                    // own parameter or a later one.
                    let own = param_names[index].clone();
                    let later: Vec<std::string::String> =
                        param_names[index + 1..].iter().flatten().cloned().collect();
                    let mut immediate = Vec::new();
                    Self::immediate_identifier_reads(initializer, &mut immediate);
                    for (name, span) in immediate {
                        if own.iter().any(|n| *n == name) {
                            self.diagnostics.push(Diagnostic {
                                code: 2372,
                                message: format!("Parameter '{name}' cannot reference itself."),
                                category: DiagnosticCategory::Error,
                                file_name: None,
                                span: Some(span),
                                related: None,
                            });
                        } else if later.iter().any(|n| *n == name) {
                            let declared = own.first().cloned().unwrap_or_default();
                            self.diagnostics.push(Diagnostic {
                                code: 2373,
                                message: format!(
                                    "Parameter '{declared}' cannot reference identifier '{name}' declared after it."
                                ),
                                category: DiagnosticCategory::Error,
                                file_name: None,
                                span: Some(span),
                                related: None,
                            });
                        }
                    }
                    self.check_expr(initializer);
                }
            }
        }
        self.provisional_param_names = saved_provisional;
    }

    /// Identifier reads that execute when `expr` is evaluated (not inside
    /// nested functions, arrows or class bodies).
    /// Immediate reads in a statement list that executes right away (IIFE body).
    fn immediate_statement_reads(stmts: &[Stmt], out: &mut Vec<(std::string::String, Span)>) {
        for stmt in stmts {
            match &stmt.kind {
                StmtKind::Expr(expr) | StmtKind::Throw(expr) => {
                    Self::immediate_identifier_reads(expr, out)
                }
                StmtKind::Return(Some(expr)) => Self::immediate_identifier_reads(expr, out),
                StmtKind::Var(var) => {
                    for declarator in &var.declarations {
                        if let Some(init) = &declarator.init {
                            Self::immediate_identifier_reads(init, out);
                        }
                    }
                }
                StmtKind::If(if_stmt) => Self::immediate_identifier_reads(&if_stmt.test, out),
                StmtKind::Block(inner) => Self::immediate_statement_reads(inner, out),
                _ => {}
            }
        }
    }

    fn immediate_identifier_reads(expr: &Expr, out: &mut Vec<(std::string::String, Span)>) {
        match &expr.kind {
            ExprKind::Ident(name) => out.push((name.to_string(), expr.span)),
            ExprKind::Member(member) => Self::immediate_identifier_reads(&member.object, out),
            ExprKind::ElemAccess(access) => {
                Self::immediate_identifier_reads(&access.object, out);
                Self::immediate_identifier_reads(&access.index, out);
            }
            ExprKind::Call(call) => {
                // An immediately invoked function's body runs right away.
                let mut callee: &Expr = &call.callee;
                while let ExprKind::Paren(inner) = &callee.kind {
                    callee = inner;
                }
                match &callee.kind {
                    // Async and generator bodies only run later.
                    ExprKind::Arrow(arrow) if !arrow.is_async => match &arrow.body {
                        ArrowBody::Expr(body) => Self::immediate_identifier_reads(body, out),
                        ArrowBody::Block(stmts) => Self::immediate_statement_reads(stmts, out),
                    },
                    ExprKind::FnExpr(function) if !function.is_async && !function.is_generator => {
                        if let Some(body) = &function.body {
                            Self::immediate_statement_reads(body, out);
                        }
                    }
                    ExprKind::Arrow(_) | ExprKind::FnExpr(_) => {}
                    _ => Self::immediate_identifier_reads(&call.callee, out),
                }
                for arg in &call.args {
                    Self::immediate_identifier_reads(arg, out);
                }
            }
            ExprKind::New(new_expr) => {
                Self::immediate_identifier_reads(&new_expr.callee, out);
                for arg in new_expr.args.iter().flatten() {
                    Self::immediate_identifier_reads(arg, out);
                }
            }
            ExprKind::Binary(binary) => {
                Self::immediate_identifier_reads(&binary.left, out);
                Self::immediate_identifier_reads(&binary.right, out);
            }
            ExprKind::Assign(assign) => {
                Self::immediate_identifier_reads(&assign.left, out);
                Self::immediate_identifier_reads(&assign.right, out);
            }
            ExprKind::Cond(cond) => {
                Self::immediate_identifier_reads(&cond.test, out);
                Self::immediate_identifier_reads(&cond.consequent, out);
                Self::immediate_identifier_reads(&cond.alternate, out);
            }
            ExprKind::Unary(unary) => Self::immediate_identifier_reads(&unary.argument, out),
            ExprKind::Update(update) => Self::immediate_identifier_reads(&update.argument, out),
            ExprKind::Paren(inner)
            | ExprKind::NonNull(inner)
            | ExprKind::Spread(inner)
            | ExprKind::Await(inner)
            | ExprKind::Typeof(inner)
            | ExprKind::Void(inner)
            | ExprKind::Delete(inner) => Self::immediate_identifier_reads(inner, out),
            ExprKind::TypeAssertion(assertion) => {
                Self::immediate_identifier_reads(&assertion.expr, out)
            }
            ExprKind::As(as_expr) => Self::immediate_identifier_reads(&as_expr.expr, out),
            ExprKind::Satisfies(satisfies) => {
                Self::immediate_identifier_reads(&satisfies.expr, out)
            }
            ExprKind::ArrayLit(items) => {
                for item in items.iter().flatten() {
                    Self::immediate_identifier_reads(item, out);
                }
            }
            ExprKind::ObjectLit(properties) => {
                for property in properties {
                    match property {
                        ObjLitProp::Property(prop) => {
                            if let PropName::Computed(key, _) = &prop.key {
                                Self::immediate_identifier_reads(key, out);
                            }
                            Self::immediate_identifier_reads(&prop.value, out);
                        }
                        ObjLitProp::Shorthand(name, span) => out.push((name.to_string(), *span)),
                        ObjLitProp::ShorthandDefault(name, init, span) => {
                            out.push((name.to_string(), *span));
                            Self::immediate_identifier_reads(init, out);
                        }
                        ObjLitProp::Spread(inner, _) => {
                            Self::immediate_identifier_reads(inner, out)
                        }
                        ObjLitProp::Method(method) => {
                            if let PropName::Computed(key, _) = &method.name {
                                Self::immediate_identifier_reads(key, out);
                            }
                        }
                        ObjLitProp::Get(accessor) | ObjLitProp::Set(accessor) => {
                            if let PropName::Computed(key, _) = &accessor.name {
                                Self::immediate_identifier_reads(key, out);
                            }
                        }
                    }
                }
            }
            ExprKind::Template(template) => {
                for inner in &template.exprs {
                    Self::immediate_identifier_reads(inner, out);
                }
            }
            ExprKind::Comma(items) => {
                for item in items {
                    Self::immediate_identifier_reads(item, out);
                }
            }
            // Class expressions evaluate their heritage clause, computed member
            // names, static initializers and static blocks immediately.
            ExprKind::ClassExpr(class) => {
                if let Some(extends) = &class.extends {
                    Self::immediate_identifier_reads(extends, out);
                }
                for member in &class.members {
                    let is_static = Self::class_member_is_static(member);
                    match &member.kind {
                        ClassMemberKind::Property(property) => {
                            if let PropName::Computed(key, _) = &property.name {
                                Self::immediate_identifier_reads(key, out);
                            }
                            if is_static {
                                if let Some(init) = &property.initializer {
                                    Self::immediate_identifier_reads(init, out);
                                }
                            }
                        }
                        ClassMemberKind::Method(method) => {
                            if let PropName::Computed(key, _) = &method.name {
                                Self::immediate_identifier_reads(key, out);
                            }
                        }
                        ClassMemberKind::GetAccessor(accessor)
                        | ClassMemberKind::SetAccessor(accessor) => {
                            if let PropName::Computed(key, _) = &accessor.name {
                                Self::immediate_identifier_reads(key, out);
                            }
                        }
                        ClassMemberKind::StaticBlock(stmts) => {
                            Self::immediate_statement_reads(stmts, out);
                        }
                        ClassMemberKind::Constructor(_)
                        | ClassMemberKind::IndexSignature(_)
                        | ClassMemberKind::SemicolonClassElement => {}
                    }
                }
            }
            _ => {}
        }
    }

    pub(crate) fn check_binding_pattern_runtime_expressions(&mut self, pattern: &Pat) {
        match &pattern.kind {
            PatKind::Ident(_) => {}
            PatKind::Array(elements) => {
                for element in elements.iter().flatten() {
                    match element {
                        ArrayPatElem::Pat(pattern) | ArrayPatElem::Rest(pattern) => {
                            self.check_binding_pattern_runtime_expressions(pattern);
                        }
                    }
                }
            }
            PatKind::Object(properties) => {
                for property in properties {
                    match property {
                        ObjPatProp::KeyValue(name, pattern) => {
                            self.check_property_name_expression_in(name, true);
                            self.check_binding_pattern_runtime_expressions(pattern);
                        }
                        ObjPatProp::ShorthandAssign(_, initializer, _) => {
                            self.check_expr(initializer);
                        }
                        ObjPatProp::Rest(pattern) => {
                            self.check_binding_pattern_runtime_expressions(pattern);
                        }
                        ObjPatProp::Shorthand(_, _) => {}
                    }
                }
            }
            PatKind::Assign(pattern, initializer) => {
                self.check_binding_pattern_runtime_expressions(pattern);
                self.check_expr(initializer);
            }
            PatKind::Rest(pattern) => self.check_binding_pattern_runtime_expressions(pattern),
        }
    }

    /// TS2314 for annotations of function-like expressions (arrows, function
    /// expressions), mirroring the declaration-level walk.
    pub(crate) fn check_function_like_generic_arity(
        &mut self,
        params: &[Param],
        return_type: Option<&TypeNode>,
    ) {
        for param in params {
            if let Some(annotation) = &param.type_ann {
                self.check_type_node_generic_arity(annotation);
            }
        }
        if let Some(return_type) = return_type {
            self.check_type_node_generic_arity(return_type);
        }
    }

    /// tsc's checkGrammarObjectLiteralExpression name-conflict rules:
    /// repeated property assignments are TS1117, repeated methods TS2300,
    /// two getters (or two setters) TS1118, and any property/method vs
    /// accessor mix TS1119. Computed literal keys (`[1]`, `[+1]`, `["x"]`)
    /// take part; other computed keys do not.
    pub(crate) fn check_object_literal_name_conflicts(&mut self, props: &[ObjLitProp]) {
        const PROP: u8 = 1;
        const METHOD: u8 = 2;
        const GET: u8 = 4;
        const SET: u8 = 8;
        fn computed_key(checker: &TypeChecker, expr: &Expr) -> Option<std::string::String> {
            match &expr.kind {
                ExprKind::StrLit(value) => Some(value.to_string()),
                ExprKind::NumLit(value) => Some(TypeChecker::normalize_numeric_name(value)),
                ExprKind::Unary(unary) => match (&unary.op, &unary.argument.kind) {
                    (UnaryOp::Neg, ExprKind::NumLit(value)) => {
                        Some(format!("-{}", TypeChecker::normalize_numeric_name(value)))
                    }
                    (UnaryOp::Pos, ExprKind::NumLit(value)) => {
                        Some(TypeChecker::normalize_numeric_name(value))
                    }
                    _ => None,
                },
                ExprKind::Paren(inner) => computed_key(checker, inner),
                // A `const` with a literal type names the property too
                // (tsc's tryGetNameFromEntityNameExpression).
                ExprKind::Ident(name) => match checker.lookup_var(name) {
                    Some(Type::StringLiteral(value)) => Some(value.clone()),
                    Some(Type::NumberLiteral(value)) => {
                        Some(TypeChecker::normalize_numeric_name(value))
                    }
                    _ => None,
                },
                ExprKind::Member(member) => {
                    let ExprKind::Ident(enum_name) = &member.object.kind else {
                        return None;
                    };
                    let members = checker.enum_info.get(enum_name.as_str())?;
                    let (name, value) = members.iter().find(|(n, _)| *n == member.property)?;
                    Some(match value {
                        Type::StringLiteral(value) => value.clone(),
                        Type::NumberLiteral(value) => TypeChecker::normalize_numeric_name(value),
                        _ => name.clone(),
                    })
                }
                _ => None,
            }
        }
        let key_of =
            |checker: &TypeChecker, name: &PropName| -> Option<(std::string::String, Span)> {
                match name {
                    PropName::Computed(expr, span) => {
                        computed_key(checker, expr).map(|key| (key, *span))
                    }
                    _ => TypeChecker::property_name_key(name),
                }
            };
        let mut seen: HashMap<std::string::String, u8> = HashMap::new();
        for prop in props {
            let (key, span, kind) = match prop {
                ObjLitProp::Property(property) => match key_of(self, &property.key) {
                    Some((key, span)) => (key, span, PROP),
                    None => continue,
                },
                ObjLitProp::Shorthand(name, span) | ObjLitProp::ShorthandDefault(name, _, span) => {
                    (name.to_string(), *span, PROP)
                }
                ObjLitProp::Method(method) => match key_of(self, &method.name) {
                    Some((key, span)) => (key, span, METHOD),
                    None => continue,
                },
                ObjLitProp::Get(accessor) => match key_of(self, &accessor.name) {
                    Some((key, span)) => (key, span, GET),
                    None => continue,
                },
                ObjLitProp::Set(accessor) => match key_of(self, &accessor.name) {
                    Some((key, span)) => (key, span, SET),
                    None => continue,
                },
                ObjLitProp::Spread(..) => continue,
            };
            let Some(&existing) = seen.get(&key) else {
                seen.insert(key, kind);
                continue;
            };
            if kind == METHOD && existing & METHOD != 0 {
                let text = self
                    .current_source
                    .as_deref()
                    .and_then(|source| source.get(span.start as usize..span.end as usize))
                    .map(|text| text.to_string())
                    .unwrap_or_else(|| key.clone());
                self.diagnostics
                    .push(error_duplicate_identifier(&text, span));
            } else if kind == PROP && existing & PROP != 0 {
                self.diagnostics
                    .push(crate::diagnostics::error_object_literal_name_conflict(
                        1117, span,
                    ));
            } else if kind & (GET | SET) != 0 && existing & (GET | SET) != 0 {
                if existing != GET | SET && kind != existing {
                    seen.insert(key, existing | kind);
                } else {
                    self.diagnostics
                        .push(crate::diagnostics::error_object_literal_name_conflict(
                            1118, span,
                        ));
                }
            } else {
                self.diagnostics
                    .push(crate::diagnostics::error_object_literal_name_conflict(
                        1119, span,
                    ));
            }
        }
    }

    /// TS2341: a private member reached through an instance (`obj.x`,
    /// `this.x`) or constructor (`C.x`) from outside the body of the class
    /// that declares it. The nearest class in the chain that declares the
    /// member decides its visibility.
    /// TS18013: private names are lexically scoped. `x.#p` outside every
    /// class that declares `#p`, on a receiver whose class declares it.
    fn check_private_name_access(&mut self, object_type: &Type, property: &str, span: Span) {
        if self.current_file_is_js() {
            return;
        }
        let (class_name, is_static) = match object_type {
            Type::TypeReference(name, _) => match name.strip_prefix("typeof ") {
                Some(class) => (class.to_string(), true),
                None => (name.clone(), false),
            },
            Type::Typeof(name) => (name.clone(), true),
            _ => return,
        };
        // The declaring class: the receiver's class or one of its bases.
        let mut current = Some(class_name);
        let mut class_name = None;
        let mut hops = 0;
        while let Some(class) = current.take() {
            hops += 1;
            let Some(info) = self.class_info.get(class.as_str()) else {
                break;
            };
            let declared = if is_static {
                info.own_private_static_members.contains(property)
            } else {
                info.own_private_members.contains(property)
            };
            if declared {
                class_name = Some(class);
                break;
            }
            // A subclass constructor does not carry its base's static
            // private names (tsc reports TS2339 there instead).
            if hops < 32 && !is_static {
                current = info.extends.clone();
            }
        }
        let Some(class_name) = class_name else {
            return;
        };
        let lexically_visible = self.enclosing_class_names.iter().any(|enclosing| {
            self.class_info
                .get(enclosing.as_str())
                .is_some_and(|enclosing_info| {
                    enclosing_info.own_private_members.contains(property)
                        || enclosing_info.own_private_static_members.contains(property)
                })
        });
        if lexically_visible
            || !self
                .reported_duplicate_spans
                .insert((18013, span.start, span.end))
        {
            return;
        }
        let display = class_name.rsplit('.').next().unwrap_or(class_name.as_str());
        self.diagnostics.push(Diagnostic {
            code: 18013,
            message: format!(
                "Property '{property}' is not accessible outside class '{display}' because it has a private identifier."
            ),
            category: DiagnosticCategory::Error,
            file_name: None,
            span: Some(span),
            related: None,
        });
    }

    pub(crate) fn check_private_member_access(
        &mut self,
        object_type: &Type,
        property: &str,
        span: Span,
    ) {
        if property.starts_with('#') {
            self.check_private_name_access(object_type, property, span);
            return;
        }
        let (class_name, is_static) = match object_type {
            Type::This => match self.enclosing_class_names.last() {
                Some(name) => (name.clone(), false),
                None => return,
            },
            Type::TypeReference(name, _) => match name.strip_prefix("typeof ") {
                Some(class) => (class.to_string(), true),
                None => (name.clone(), false),
            },
            _ => return,
        };
        let mut current = Some(class_name);
        let mut hops = 0usize;
        while let Some(class) = current.take() {
            hops += 1;
            if hops > 64 {
                return;
            }
            let Some(info) = self.class_info.get(class.as_str()) else {
                return;
            };
            let declares_private = if is_static {
                info.own_private_static_members.contains(property)
            } else {
                info.own_private_members.contains(property)
            };
            if declares_private {
                if !self.enclosing_class_names.iter().any(|name| name == &class) {
                    // Classes inside namespaces are keyed by qualified name;
                    // tsc prints the declared name.
                    let display = class.rsplit('.').next().unwrap_or(class.as_str());
                    self.diagnostics
                        .push(error_private_member_access(property, display, span));
                }
                return;
            }
            let declares_member = if is_static {
                info.static_properties.iter().any(|(n, _)| n == property)
                    || info.static_methods.iter().any(|(n, _)| n == property)
            } else {
                info.instance_properties.iter().any(|(n, _)| n == property)
                    || info.instance_methods.iter().any(|(n, _)| n == property)
                    || info.own_protected_members.contains(property)
                    || info.accessor_props.contains(property)
            };
            if declares_member {
                return;
            }
            current = info.extends.clone();
        }
    }

    /// TS2454 for a read of `name` at `span`. Reads inside nested functions
    /// are only reported at the declaring function depth (a callback may run
    /// after a later assignment); the target of an assignment, a
    /// destructuring pattern, or a for-in/for-of head is a write, not a read,
    /// and marks the binding assigned.
    pub(crate) fn check_read_before_assignment(&mut self, name: &str, span: Span) {
        if self.assignment_target_span == Some(span) {
            self.uninitialized_vars.remove(name);
            return;
        }
        if self.unreachable_read_depth > 0 || self.type_query_depth > 0 {
            return;
        }
        if !self
            .uninitialized_vars
            .get(name)
            .is_some_and(|(depth, scope)| {
                *depth == self.fn_nesting_depth && self.nearest_binding_scope(name) == Some(*scope)
            })
        {
            return;
        }
        // Once a guard has established a narrowed value for this branch,
        // tsc does not repeat TS2454 on the guarded read; the diagnostic
        // belongs to the guard's original read. A falsy-branch refinement
        // (`false`) keeps the possibly-unassigned state, so it still reports.
        if let Some(narrowed) = self.lookup_narrowed(name) {
            let falsy_refinement = match &narrowed {
                Type::BooleanLiteral(false) => true,
                Type::Union(members) => members
                    .iter()
                    .any(|m| matches!(m, Type::BooleanLiteral(false))),
                _ => false,
            };
            if !falsy_refinement {
                return;
            }
        }
        // Expression inference can revisit the same identifier (notably as
        // a member-call receiver). tsc emits TS2454 once per source use.
        let already_reported = self
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == 2454 && diagnostic.span == Some(span));
        if !already_reported {
            self.diagnostics
                .push(error_used_before_assigned(name, span));
        }
    }

    /// tsc's checkComputedPropertyName: the key must be assignable to
    /// string | number | symbol (or be `any`). Every union member must be; a
    /// type parameter is judged by its constraint (none is invalid). Types we
    /// cannot classify are accepted.
    fn computed_key_type_invalid(&self, ty: &Type, depth: u8) -> bool {
        if depth > 8 {
            return false;
        }
        match ty {
            Type::BigInt
            | Type::BigIntLiteral(_)
            | Type::Boolean
            | Type::BooleanLiteral(_)
            | Type::Null
            | Type::Undefined
            | Type::Void
            | Type::ObjectType(_)
            | Type::Array(_)
            | Type::Tuple(_)
            | Type::Function(_) => true,
            Type::Union(members) => members
                .iter()
                .any(|member| self.computed_key_type_invalid(member, depth + 1)),
            Type::Intersection(members) => members
                .iter()
                .all(|member| self.computed_key_type_invalid(member, depth + 1)),
            Type::TypeParameter(_) => match self.declared_type_param_constraint(ty) {
                Some(constraint) => self.computed_key_type_invalid(&constraint, depth + 1),
                None => self.source_type_param_declared_unconstrained(ty),
            },
            Type::TypeReference(name, args)
                if args.is_empty() && self.type_param_name_is_active(name) =>
            {
                match self.declared_type_param_constraint(ty) {
                    Some(constraint) => self.computed_key_type_invalid(&constraint, depth + 1),
                    None => self.type_param_declared_unconstrained(name),
                }
            }
            _ => false,
        }
    }

    pub(crate) fn check_property_name_expression(&mut self, name: &PropName) {
        self.check_property_name_expression_in(name, false);
    }

    /// `in_binding_pattern`: a destructuring key is not a declared property
    /// name, so tsc does not apply TS2464 to it.
    pub(crate) fn check_property_name_expression_in(
        &mut self,
        name: &PropName,
        in_binding_pattern: bool,
    ) {
        if let PropName::Computed(expression, span) = name {
            let saved_computed = self.computed_name_depth.replace(self.fn_nesting_depth);
            let ty = self.check_expr(expression);
            self.computed_name_depth = saved_computed;
            // `[await]` without an operand has tsc's error type (`any`).
            let missing_await_operand = matches!(
                &expression.kind,
                ExprKind::Await(operand) if matches!(operand.kind, ExprKind::Omitted)
            );
            if in_binding_pattern || missing_await_operand {
                return;
            }
            // TS2464: a computed property name must be string, number,
            // symbol or any; bigint, boolean, nullish and object-like keys
            // are rejected (tsc checkComputedPropertyName).
            let invalid = self.computed_key_type_invalid(&ty, 0);
            if invalid {
                self.diagnostics.push(Diagnostic {
                    code: 2464,
                    message: "A computed property name must be of type 'string', 'number', 'symbol', or 'any'.".to_string(),
                    category: DiagnosticCategory::Error,
                    file_name: None,
                    span: Some(*span),
                    related: None,
                });
            }
        }
    }

    pub(crate) fn check_type_declaration_computed_names(&mut self, statements: &[Stmt]) {
        fn check_members(checker: &mut TypeChecker, members: &[TypeMember]) {
            fn check_signature(
                checker: &mut TypeChecker,
                parameters: &[Param],
                return_type: Option<&TypeNode>,
            ) {
                for parameter in parameters {
                    if let Some(annotation) = &parameter.type_ann {
                        check_type(checker, annotation);
                    }
                }
                if let Some(return_type) = return_type {
                    check_type(checker, return_type);
                }
            }

            for member in members {
                match &member.kind {
                    TypeMemberKind::PropertySig(property) => {
                        // `[K in T]: V` among other members is tsc's malformed
                        // mapped type (TS7061), not a computed property name.
                        let mapped_type_shape = matches!(
                            &property.name,
                            PropName::Computed(key, _)
                                if matches!(&key.kind, ExprKind::Binary(b) if b.op == BinaryOp::In)
                        );
                        checker
                            .check_property_name_expression_in(&property.name, mapped_type_shape);
                        if let Some(annotation) = &property.type_ann {
                            check_type(checker, annotation);
                        }
                    }
                    TypeMemberKind::MethodSig(method) => {
                        checker.check_property_name_expression(&method.name);
                        check_signature(checker, &method.params, method.return_type.as_ref());
                    }
                    TypeMemberKind::GetAccessorSig(accessor)
                    | TypeMemberKind::SetAccessorSig(accessor) => {
                        checker.check_property_name_expression(&accessor.name);
                        checker.check_accessor_signature_grammar(
                            matches!(member.kind, TypeMemberKind::GetAccessorSig(_)),
                            &accessor.name,
                            &accessor.params,
                            accessor.return_type.as_ref(),
                            false,
                        );
                        check_signature(checker, &accessor.params, accessor.return_type.as_ref());
                    }
                    TypeMemberKind::CallSig(signature) => {
                        check_signature(checker, &signature.params, signature.return_type.as_ref());
                    }
                    TypeMemberKind::ConstructSig(signature) => {
                        check_signature(checker, &signature.params, signature.return_type.as_ref());
                    }
                    TypeMemberKind::IndexSig(signature) => {
                        check_signature(checker, &signature.params, signature.type_ann.as_ref());
                    }
                }
            }
        }

        fn check_type(checker: &mut TypeChecker, node: &TypeNode) {
            match &node.kind {
                TypeNodeKind::Reference(reference) => {
                    if let Some(arguments) = &reference.type_args {
                        for argument in arguments {
                            check_type(checker, argument);
                        }
                    }
                }
                TypeNodeKind::TypeLit(members) => check_members(checker, members),
                TypeNodeKind::Array(inner)
                | TypeNodeKind::Readonly(inner)
                | TypeNodeKind::Keyof(inner)
                | TypeNodeKind::Unique(inner)
                | TypeNodeKind::Paren(inner)
                | TypeNodeKind::Rest(inner)
                | TypeNodeKind::Optional(inner)
                | TypeNodeKind::TypeOperator(_, inner)
                | TypeNodeKind::JSDocNullable(Some(inner)) => check_type(checker, inner),
                TypeNodeKind::NamedTupleMember(member) => {
                    check_type(checker, &member.type_node);
                }
                TypeNodeKind::Tuple(elements) => {
                    for element in elements {
                        check_type(checker, &element.type_node);
                    }
                }
                TypeNodeKind::Union(nodes) | TypeNodeKind::Intersection(nodes) => {
                    for node in nodes {
                        check_type(checker, node);
                    }
                }
                TypeNodeKind::Conditional(conditional) => {
                    check_type(checker, &conditional.check);
                    check_type(checker, &conditional.extends);
                    check_type(checker, &conditional.true_type);
                    check_type(checker, &conditional.false_type);
                }
                TypeNodeKind::Function(function) | TypeNodeKind::Constructor(function) => {
                    if let Some(type_parameters) = &function.type_params {
                        for parameter in type_parameters {
                            if let Some(constraint) = &parameter.constraint {
                                check_type(checker, constraint);
                            }
                            if let Some(default) = &parameter.default {
                                check_type(checker, default);
                            }
                        }
                    }
                    for parameter in &function.params {
                        if let Some(annotation) = &parameter.type_ann {
                            check_type(checker, annotation);
                        }
                    }
                    check_type(checker, &function.return_type);
                }
                TypeNodeKind::Mapped(mapped) => {
                    if let Some(constraint) = &mapped.type_param.constraint {
                        check_type(checker, constraint);
                    }
                    if let Some(default) = &mapped.type_param.default {
                        check_type(checker, default);
                    }
                    if let Some(name_type) = &mapped.name_type {
                        check_type(checker, name_type);
                    }
                    if let Some(annotation) = &mapped.type_ann {
                        check_type(checker, annotation);
                    }
                }
                TypeNodeKind::IndexedAccess(object, index) => {
                    check_type(checker, object);
                    check_type(checker, index);
                }
                TypeNodeKind::Infer(_, Some(inner)) => check_type(checker, inner),
                TypeNodeKind::TemplateLit(template) => {
                    for node in &template.types {
                        check_type(checker, node);
                    }
                }
                TypeNodeKind::ImportType(import) => {
                    check_type(checker, &import.argument);
                    if let Some(arguments) = &import.type_args {
                        for argument in arguments {
                            check_type(checker, argument);
                        }
                    }
                }
                TypeNodeKind::Predicate(predicate) => {
                    if let Some(annotation) = &predicate.type_ann {
                        check_type(checker, annotation);
                    }
                }
                TypeNodeKind::Keyword(_)
                | TypeNodeKind::TypeQuery(_)
                | TypeNodeKind::This
                | TypeNodeKind::JSDocNullable(None)
                | TypeNodeKind::Infer(_, None)
                | TypeNodeKind::Literal(_) => {}
            }
        }

        for statement in statements {
            match &statement.kind {
                StmtKind::InterfaceDecl(interface) => check_members(self, &interface.members),
                StmtKind::TypeAlias(alias) => check_type(self, &alias.type_ann),
                // Type literals in variable, parameter and return
                // annotations carry computed names too (`var v: { [e]: T }`).
                StmtKind::Var(var_stmt) => {
                    for declaration in &var_stmt.declarations {
                        if let Some(annotation) = &declaration.type_ann {
                            check_type(self, annotation);
                        }
                    }
                }
                StmtKind::FnDecl(function) => {
                    for parameter in &function.params {
                        if let Some(annotation) = &parameter.type_ann {
                            check_type(self, annotation);
                        }
                    }
                    if let Some(return_type) = &function.return_type {
                        check_type(self, return_type);
                    }
                }
                StmtKind::Export(export) => match &export.kind {
                    ExportDeclKind::Decl(statement) | ExportDeclKind::DefaultDecl(statement) => {
                        self.check_type_declaration_computed_names(std::slice::from_ref(statement));
                    }
                    _ => {}
                },
                StmtKind::ModuleDecl(module) => {
                    if let Some(ModuleBody::Block(statements)) = &module.body {
                        self.check_type_declaration_computed_names(statements);
                    }
                }
                _ => {}
            }
        }
    }

    /// Context-independent regular-expression escape diagnostics. Non-zero
    /// decimal escapes require capture-group and character-class analysis; a
    /// zero followed by any decimal digit always enters the legacy-octal
    /// grammar path.
    #[cold]
    fn check_regexp_escape_diagnostics(&mut self, pattern: &str, expr_span: Span) {
        if !pattern.as_bytes().contains(&b'\\') {
            return;
        }

        let bytes = pattern.as_bytes();
        let mut offset = 0usize;
        let mut suppressed_octal_starts = Vec::new();
        while offset + 1 < bytes.len() {
            if bytes[offset] != b'\\' {
                offset += 1;
                continue;
            }

            let first = bytes[offset + 1];
            if matches!(first, b'x' | b'u') {
                let mut failure = offset + 2;
                let invalid = if first == b'u' && bytes.get(failure) == Some(&b'{') {
                    failure += 1;
                    let digit_start = failure;
                    while bytes
                        .get(failure)
                        .is_some_and(|byte| byte.is_ascii_hexdigit())
                    {
                        failure += 1;
                    }
                    // Once a digit is present, malformed braced escapes use
                    // TS1199/TS1198 rather than TS1125.
                    failure == digit_start
                } else {
                    let required_end = if first == b'x' {
                        offset + 4
                    } else {
                        offset + 6
                    };
                    while failure < required_end
                        && bytes
                            .get(failure)
                            .is_some_and(|byte| byte.is_ascii_hexdigit())
                    {
                        failure += 1;
                    }
                    failure < required_end
                };

                if bytes.get(failure) == Some(&b'\\') {
                    suppressed_octal_starts.push(failure);
                }
                if invalid {
                    let failure = expr_span.start + 1 + failure as u32;
                    let failure_span = Span::new(failure, failure);
                    if !self.diagnostics.iter().any(|diagnostic| {
                        diagnostic.code == 1125 && diagnostic.span == Some(failure_span)
                    }) {
                        self.diagnostics.push(Diagnostic {
                            code: 1125,
                            message: "Hexadecimal digit expected.".to_string(),
                            category: DiagnosticCategory::Error,
                            file_name: None,
                            span: Some(failure_span),
                            related: None,
                        });
                    }
                }
                offset += 2;
                continue;
            }

            if first != b'0' {
                // Skip the escaped byte too, so `\\01` does not reinterpret
                // its second slash as the start of an octal escape.
                offset += 2;
                continue;
            }
            if offset + 2 >= bytes.len() {
                break;
            }
            if !bytes[offset + 2].is_ascii_digit() {
                offset += 2;
                continue;
            }
            if suppressed_octal_starts.contains(&offset) {
                offset += 2;
                continue;
            }

            let mut end = offset + 2;
            while end < bytes.len() && end < offset + 4 && (b'0'..=b'7').contains(&bytes[end]) {
                end += 1;
            }
            let mut value = 0u8;
            for digit in &bytes[offset + 1..end] {
                value = value * 8 + (*digit - b'0');
            }

            // The pattern begins immediately after the opening slash.
            let escape_span = Span::new(
                expr_span.start + 1 + offset as u32,
                expr_span.start + 1 + end as u32,
            );
            if !self.diagnostics.iter().any(|diagnostic| {
                (diagnostic.code == 1487 && diagnostic.span == Some(escape_span))
                    || (diagnostic.code == 1125
                        && diagnostic
                            .span
                            .is_some_and(|span| span.start == escape_span.start))
            }) {
                self.diagnostics.push(Diagnostic {
                    code: 1487,
                    message: format!(
                        "Octal escape sequences are not allowed. Use the syntax '\\x{value:02x}'."
                    ),
                    category: DiagnosticCategory::Error,
                    file_name: None,
                    span: Some(escape_span),
                    related: None,
                });
            }
            offset = end;
        }
    }

    /// Return the root binding of a property-access assignment target.
    ///
    /// In checked JavaScript, `ns.next = ...` is also a declaration of the
    /// expando chain. The root must therefore exist before the target (and a
    /// self-reference on the RHS) is checked. Restrict this walker to pure
    /// member/element chains so arbitrary assignment expressions do not gain
    /// an implicit binding.
    fn js_expando_assignment_root(expr: &Expr) -> Option<&str> {
        match &expr.kind {
            ExprKind::Ident(name) => Some(name.as_str()),
            ExprKind::Member(member) => Self::js_expando_assignment_root(&member.object),
            ExprKind::ElemAccess(access) => Self::js_expando_assignment_root(&access.object),
            ExprKind::Paren(inner) => Self::js_expando_assignment_root(inner),
            _ => None,
        }
    }

    /// If `name` is a static property or method of any enclosing class (innermost
    /// first), return that class's name. Used to refine an unresolved-name error
    /// from TS2304 to TS2662 ("Did you mean the static member 'C.name'?").
    fn enclosing_class_with_static_member(&self, name: &str) -> Option<std::string::String> {
        for class_name in self.enclosing_class_names.iter().rev() {
            if let Some(info) = self.class_info.get(class_name) {
                let is_static = info.static_properties.iter().any(|(n, _)| n == name)
                    || info.static_methods.iter().any(|(n, _)| n == name);
                if is_static {
                    return Some(class_name.clone());
                }
            }
        }
        None
    }

    /// Collapse duplicate keys in a freshly-built object-literal property
    /// list, keeping the LAST occurrence of each key — JS evaluation order,
    /// where `{ ...item, id: "x" }` resolves `id` to `"x"` and
    /// `{ id: "x", ...item }` resolves it to `item.id`. The literal builder
    /// pushes spread-contributed and explicit properties in source order
    /// without deduping, so an override like `{ ...item, id }` produced two
    /// `id` entries. A duplicate key combined with an `Optional` member
    /// confused structural assignability and tripped a phantom TS2322
    /// (real site: the CMS menus editor's `handleAddCustomLink`). Order is
    /// preserved by last occurrence.
    /// TS2367 for object shapes: tsc's comparable relation is two-way
    /// assignability minus the weak-type check, with union sources related
    /// through any member. Only CLOSED shapes take part — no unions,
    /// intersections, type parameters, generic signatures, or weak shapes —
    /// so that plain two-way assignability is a faithful stand-in.
    fn object_shapes_have_no_overlap(&self, left: &Type, right: &Type) -> bool {
        let Some(left_shape) = self.comparable_object_shape(left, 0) else {
            return false;
        };
        let Some(right_shape) = self.comparable_object_shape(right, 0) else {
            return false;
        };
        if assign::is_weak_object(&left_shape) || assign::is_weak_object(&right_shape) {
            return false;
        }
        !self.is_assignable_to(left, right) && !self.is_assignable_to(right, left)
    }

    /// The resolved closed object shape of `ty` for the comparison gate, or
    /// `None` when the type is not a simple closed shape.
    /// tsc's comparable relation for `==`/`switch` operands, as far as it
    /// is modelled: (no overlap, relation shape of each side). A shape is
    /// (type used for the relation, type shown in messages).
    #[allow(clippy::type_complexity)]
    pub(crate) fn comparison_no_overlap(
        &self,
        l_cmp: &Type,
        r_cmp: &Type,
    ) -> (bool, Option<(Type, Type)>, Option<(Type, Type)>) {
        // A type parameter compares through its apparent
        // constraint when that is a primitive shape; two
        // distinct unconstrained parameters never overlap.
        let type_param_name = |t: &Type| -> Option<String> {
            match t {
                Type::TypeParameter(p) => Some(p.clone()),
                Type::TypeReference(p, a) if a.is_empty() && self.type_param_name_is_active(p) => {
                    Some(p.clone())
                }
                _ => None,
            }
        };
        // (shape used for the relation, type shown in the
        // message). Enum types/members and `void` take part;
        // a type alias to a literal shape resolves first and
        // is displayed resolved (`a: A` with `type A = 1` →
        // '1'), while a type parameter relates through its
        // constraint but is displayed as itself.
        let comparable_shape = |t: &Type| -> Option<(Type, Type)> {
            // A computed member (`n = "n".length`) has the
            // enum type itself, not a literal member type.
            if let Type::EnumVariant {
                enum_name, value, ..
            } = t
            {
                if !matches!(value.as_deref(), Some(Type::NumberLiteral(_)))
                    && !matches!(value.as_deref(), Some(Type::StringLiteral(_)))
                {
                    let whole =
                        Type::TypeReference(enum_name.clone(), Arc::from(Vec::<Type>::new()));
                    return Some((whole.clone(), whole));
                }
            }
            if self.equality_comparable_shape(t) {
                Some((t.clone(), t.clone()))
            } else if type_param_name(t).is_some() {
                self.typeparam_constraint_apparent(t)
                    .filter(|c| self.equality_comparable_shape(c))
                    .map(|c| (c, t.clone()))
            } else if let Type::TypeReference(name, args) = t {
                if args.is_empty() && self.type_aliases.contains_key(name.as_str()) {
                    self.resolve_type_for_assignability(t)
                        .filter(|r| r != t && self.equality_comparable_shape(r))
                        .map(|r| (r.clone(), r))
                } else {
                    None
                }
            } else if let Type::Intersection(members) = t {
                // `T & number` relates through its primitive
                // constituent and is displayed whole.
                members
                    .iter()
                    .find(|m| self.equality_comparable_shape(m))
                    .map(|m| (m.clone(), t.clone()))
            } else {
                None
            }
        };
        // "Unconstrained" means no constraint at all — a
        // parameter constrained by another parameter
        // (`U extends T`) is comparable to it.
        let unconstrained_pair = match (type_param_name(l_cmp), type_param_name(r_cmp)) {
            (Some(l), Some(r)) => {
                l != r
                    && self.type_param_declared_unconstrained(&l)
                    && self.type_param_declared_unconstrained(&r)
            }
            _ => false,
        };
        self.comparable_depth
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let l_shape = comparable_shape(l_cmp);
        let r_shape = comparable_shape(r_cmp);
        let no_overlap = unconstrained_pair
            || match (&l_shape, &r_shape) {
                (Some((l, _)), Some((r, _))) => {
                    !self.is_assignable_to(l, r)
                        && !self.is_assignable_to(r, l)
                        && !self.union_members_overlap(l, r)
                }
                _ => self.object_shapes_have_no_overlap(l_cmp, r_cmp),
            };
        self.comparable_depth
            .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        (no_overlap, l_shape, r_shape)
    }

    /// The comparable relation is decided member by member: two unions
    /// overlap when some pair of members relates in either direction
    /// (`string` and `number | "hello"` overlap through `"hello"`).
    fn union_members_overlap(&self, left: &Type, right: &Type) -> bool {
        let members = |ty: &Type| match ty {
            Type::Union(members) => members.iter().cloned().collect::<Vec<_>>(),
            other => vec![other.clone()],
        };
        let (left, right) = (members(left), members(right));
        if left.len() == 1 && right.len() == 1 {
            return false;
        }
        left.iter().any(|l| {
            right
                .iter()
                .any(|r| self.is_assignable_to(l, r) || self.is_assignable_to(r, l))
        })
    }

    fn comparable_object_shape(&self, ty: &Type, depth: u8) -> Option<Type> {
        if depth > 3 {
            return None;
        }
        match ty {
            Type::ObjectType(info) => self.object_info_is_closed(info, depth).then(|| ty.clone()),
            Type::Function(ft) => (ft.type_params.is_empty()
                && ft
                    .params
                    .iter()
                    .all(|(_, p)| self.comparable_member_type_is_closed(p, depth + 1))
                && self.comparable_member_type_is_closed(&ft.return_type, depth + 1))
            .then(|| ty.clone()),
            Type::Constructor(ct) => (ct.type_params.is_empty()
                && ct
                    .params
                    .iter()
                    .all(|(_, p)| self.comparable_member_type_is_closed(p, depth + 1))
                && self.comparable_member_type_is_closed(&ct.return_type, depth + 1))
            .then(|| ty.clone()),
            Type::TypeReference(name, args) if args.is_empty() => {
                let program_interface = self
                    .interface_info
                    .get(name.as_str())
                    .is_some_and(|info| !info.decl_file.is_empty());
                if !self.class_info.contains_key(name.as_str()) && !program_interface {
                    return None;
                }
                // Interface optionality lives in `optional_props`, not in the
                // member types.
                if self.interface_chain_has_optional_props(name, &mut HashSet::new()) {
                    return None;
                }
                match self.get_class_instance_type(name) {
                    Type::ObjectType(info) if self.object_info_is_closed(&info, depth) => {
                        Some(Type::ObjectType(info))
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn interface_chain_has_optional_props(&self, name: &str, seen: &mut HashSet<String>) -> bool {
        if !seen.insert(name.to_string()) {
            return false;
        }
        let Some(info) = self.interface_info.get(name) else {
            return false;
        };
        !info.optional_props.is_empty()
            || info
                .extends
                .iter()
                .any(|(base, _)| self.interface_chain_has_optional_props(base, seen))
    }

    fn object_info_is_closed(&self, info: &ObjectTypeInfo, depth: u8) -> bool {
        // Optional properties relate through `undefined` in tsc's
        // comparable relation, so shapes with them are not closed.
        info.properties.iter().all(|(name, p)| {
            !name.starts_with('?')
                && !matches!(p.as_ref(), Type::Optional(_))
                && self.comparable_member_type_is_closed(p, depth + 1)
        }) && info.call_signatures.iter().all(|sig| {
            sig.type_params.is_empty()
                && sig
                    .params
                    .iter()
                    .all(|(_, p)| self.comparable_member_type_is_closed(p, depth + 1))
                && self.comparable_member_type_is_closed(&sig.return_type, depth + 1)
        }) && info.construct_signatures.iter().all(|sig| {
            sig.type_params.is_empty()
                && sig
                    .params
                    .iter()
                    .all(|(_, p)| self.comparable_member_type_is_closed(p, depth + 1))
                && self.comparable_member_type_is_closed(&sig.return_type, depth + 1)
        }) && info.index_signature.as_ref().is_none_or(|(key, value)| {
            self.comparable_member_type_is_closed(key, depth + 1)
                && self.comparable_member_type_is_closed(value, depth + 1)
        })
    }

    /// A member type that keeps a shape "closed": primitives, literals,
    /// enums, class/interface references, arrays of closed types, and
    /// nested closed shapes. Unions, intersections, type parameters, `any`,
    /// `unknown`, and computed types are not.
    fn comparable_member_type_is_closed(&self, ty: &Type, depth: u8) -> bool {
        if depth > 4 {
            return false;
        }
        match ty {
            // Literal member types relate to their base primitive in either
            // direction under comparability (`{ a: 1 }` vs `{ a: number }`),
            // so only base primitives keep a shape closed.
            Type::Number
            | Type::String
            | Type::Boolean
            | Type::BigInt
            | Type::Symbol
            | Type::Void
            | Type::EnumType(..) => true,
            Type::Array(inner) => self.comparable_member_type_is_closed(inner, depth + 1),
            Type::TypeReference(name, args) if args.is_empty() => {
                self.class_info.contains_key(name.as_str())
                    || self
                        .interface_info
                        .get(name.as_str())
                        .is_some_and(|info| !info.decl_file.is_empty())
            }
            Type::ObjectType(_) | Type::Function(_) | Type::Constructor(_) => {
                self.comparable_object_shape(ty, depth).is_some()
            }
            _ => false,
        }
    }

    /// TS2367 shape gate: primitive/literal types (and unions thereof) whose
    /// no-overlap judgement via two-way assignability mirrors tsc's
    /// comparability. Object/reference/nullish/any shapes are excluded — tsc
    /// is laxer there, and our inference is less precise.
    pub(crate) fn comparable_primitive_shape(ty: &Type) -> bool {
        match ty {
            Type::Number
            | Type::String
            | Type::Boolean
            | Type::BigInt
            | Type::NumberLiteral(_)
            | Type::StringLiteral(_)
            | Type::BooleanLiteral(_)
            | Type::BigIntLiteral(_) => true,
            Type::Union(members) => members.iter().all(Self::comparable_primitive_shape),
            _ => false,
        }
    }

    pub(crate) fn dedup_object_props_last_wins(
        props: Vec<(std::string::String, Arc<Type>)>,
    ) -> Vec<(std::string::String, Arc<Type>)> {
        // Fast path: no duplicates → return as-is (keeps allocations down on
        // the overwhelmingly common no-spread / no-override object literal).
        let mut keys = std::collections::HashSet::with_capacity(props.len());
        if props.iter().all(|(k, _)| keys.insert(k.as_str())) {
            return props;
        }
        let mut seen = std::collections::HashSet::with_capacity(props.len());
        let mut out: Vec<(std::string::String, Arc<Type>)> = Vec::with_capacity(props.len());
        for (k, v) in props.into_iter().rev() {
            if seen.insert(k.clone()) {
                out.push((k, v));
            }
        }
        out.reverse();
        out
    }

    pub(crate) fn check_expr_contextual(
        &mut self,
        expr: &Expr,
        contextual_type: Option<&Type>,
    ) -> Type {
        match &expr.kind {
            ExprKind::Arrow(_) | ExprKind::FnExpr(_) => {
                let ty = self.check_arrow_or_fn_contextual(expr, contextual_type);
                // Use or_insert to avoid overwriting parameter types that were
                // recorded at the same position (e.g., `a => 10` where the
                // arrow span starts at the parameter name)
                self.record_expr_type_if_absent(expr.span.start, &ty);
                return ty;
            }
            ExprKind::Paren(inner) => {
                let ty = self.check_expr_contextual(inner, contextual_type);
                self.record_expr_type(expr.span.start, &ty);
                return ty;
            }
            ExprKind::Call(_) | ExprKind::New(_) if contextual_type.is_some() => {
                self.contextual_call_returns
                    .insert(expr.span.start, contextual_type.cloned().unwrap());
                let ty = self.check_expr(expr);
                self.contextual_call_returns.remove(&expr.span.start);
                return ty;
            }
            ExprKind::Template(template) => {
                // tsc checkTemplateExpression: under a template-literal or
                // string-literal contextual type the expression has its OWN
                // template literal type (literal substitutions fold into the
                // text, the rest widen to their base type); the enclosing
                // relation then decides (`\`abc${string}\`` vs
                // `\`abc${number}\``).
                let template_context = contextual_type.is_some_and(|ct| {
                    let templateish = |t: &Type| {
                        matches!(t, Type::TemplateLiteral { .. } | Type::StringLiteral(_))
                    };
                    match ct {
                        Type::Union(members) => members.iter().any(templateish),
                        other => templateish(other),
                    }
                });
                if template_context {
                    let actual_quasis: Vec<std::string::String> = template
                        .quasis
                        .iter()
                        .map(|q| q.cooked.clone().unwrap_or_else(|| q.raw.clone()))
                        .collect();
                    let actual_types: Vec<Type> = template
                        .exprs
                        .iter()
                        .map(|part| self.check_expr(part))
                        .collect();
                    return self.template_literal_type_of_expression(actual_quasis, actual_types);
                }

                let quasis = template
                    .quasis
                    .iter()
                    .map(|q| q.cooked.clone().unwrap_or_else(|| q.raw.clone()))
                    .collect();
                let types = template
                    .exprs
                    .iter()
                    .map(|part| self.check_expr(part))
                    .collect();
                return Self::evaluate_template_literal_type(quasis, types);
            }
            ExprKind::ObjectLit(props) if contextual_type.is_some() => {
                for prop in props {
                    if let ObjLitProp::Property(p) = prop {
                        if let PropName::Number(n, sp) = &p.key {
                            if n.ends_with('n') {
                                self.diagnostics.push(error_bigint_property_name(*sp));
                            }
                        }
                    }
                }
                // Pass contextual member types to function-valued properties
                let ctx = contextual_type.unwrap();
                let mut properties = Vec::new();
                let mut any_spread = false;
                for prop in props {
                    match prop {
                        tsc_rs_ast::ObjLitProp::Property(p) => {
                            let key = self.prop_name_to_string(&p.key);
                            let member_ctx = self.extract_member_type_opt(ctx, &key);
                            // Recurse with contextual not just for Arrow/FnExpr
                            // — string / number literals nested inside an
                            // object literal that's assigned to a typed slot
                            // also need contextual narrowing so the literal
                            // stays narrow (`"asc"`) instead of widening to
                            // `string`. Real-world site: Prisma `orderBy:
                            // [{ createdAt: "asc" }]` against an
                            // `{ createdAt?: "asc" | "desc"; ... }[]` slot —
                            // without this, the inner object widens and
                            // tripped TS2322 across many `page.tsx` files
                            // (~12 errors in apps/app).
                            let val_ty = if member_ctx.is_some()
                                && matches!(
                                    p.value.kind,
                                    ExprKind::Arrow(_)
                                        | ExprKind::FnExpr(_)
                                        | ExprKind::ObjectLit(_)
                                        | ExprKind::ArrayLit(_)
                                        | ExprKind::StrLit(_)
                                        | ExprKind::NumLit(_)
                                        | ExprKind::BoolLit(_)
                                        | ExprKind::Template(_)
                                        | ExprKind::Paren(_)
                                        | ExprKind::Call(_)
                                        | ExprKind::New(_)
                                ) {
                                self.check_expr_contextual(&p.value, member_ctx.as_ref())
                            } else {
                                self.check_expr(&p.value)
                            };
                            properties.push((key, Arc::new(val_ty)));
                        }
                        tsc_rs_ast::ObjLitProp::Method(m) => {
                            let key = self.prop_name_to_string(&m.name);
                            let member_ctx = self.extract_member_type_opt(ctx, &key);
                            // Create a synthetic FnExpr to pass through contextual typing
                            let method_ty = self.check_method_contextual(m, member_ctx.as_ref());
                            properties.push((key, Arc::new(method_ty)));
                        }
                        tsc_rs_ast::ObjLitProp::Get(acc) => {
                            self.check_getter_missing_return(&acc.name, &acc.body);
                            self.check_accessor_signature_grammar(
                                true,
                                &acc.name,
                                &acc.params,
                                acc.return_type.as_ref(),
                                false,
                            );
                            let key = self.prop_name_to_string(&acc.name);
                            self.push_scope();
                            let saved_var_first_types = std::mem::take(&mut self.var_first_types);
                            self.declare_var("arguments", Type::Any);
                            self.declare_var(SUPER_PROPERTY_OK_MARKER, Type::Never);
                            self.declare_var(THIS_OK_MARKER, Type::Never);
                            self.bind_function_this(&[]);
                            for param in &acc.params {
                                self.declare_pattern_vars(&param.name, Type::Any);
                                self.completion_mark_parameter(param);
                            }
                            self.hoist_block_declarations(&acc.body);
                            self.jump_function_depth += 1;
                            for stmt in &acc.body {
                                self.check_stmt(stmt);
                            }
                            self.jump_function_depth -= 1;
                            self.var_first_types = saved_var_first_types;
                            self.pop_scope();
                            let getter_ty = acc
                                .return_type
                                .as_ref()
                                .map(|ty| self.resolve_type_node(ty))
                                .unwrap_or_else(|| self.infer_return_type_from_stmts(&acc.body));
                            properties.push((key, Arc::new(getter_ty)));
                        }
                        tsc_rs_ast::ObjLitProp::Set(acc) => {
                            self.check_accessor_signature_grammar(
                                false,
                                &acc.name,
                                &acc.params,
                                acc.return_type.as_ref(),
                                false,
                            );
                            self.check_setter_value_returns(&acc.body);
                            let key = self.prop_name_to_string(&acc.name);
                            let member_ctx = self.extract_member_type_opt(ctx, &key);
                            let mut setter_ty = member_ctx.clone().unwrap_or(Type::Any);
                            self.push_scope();
                            let saved_var_first_types = std::mem::take(&mut self.var_first_types);
                            self.declare_var("arguments", Type::Any);
                            self.declare_var(SUPER_PROPERTY_OK_MARKER, Type::Never);
                            self.declare_var(THIS_OK_MARKER, Type::Never);
                            self.bind_function_this(&[]);
                            for (index, param) in acc.params.iter().enumerate() {
                                let param_ty = param
                                    .type_ann
                                    .as_ref()
                                    .map(|ty| self.resolve_type_node(ty))
                                    .or_else(|| if index == 0 { member_ctx.clone() } else { None })
                                    .unwrap_or(Type::Any);
                                if index == 0 {
                                    setter_ty = param_ty.clone();
                                }
                                self.declare_pattern_vars(&param.name, param_ty);
                                self.completion_mark_parameter(param);
                            }
                            self.hoist_block_declarations(&acc.body);
                            self.jump_function_depth += 1;
                            for stmt in &acc.body {
                                self.check_stmt(stmt);
                            }
                            self.jump_function_depth -= 1;
                            self.var_first_types = saved_var_first_types;
                            self.pop_scope();
                            properties.push((key, Arc::new(setter_ty)));
                        }
                        tsc_rs_ast::ObjLitProp::Shorthand(name, _) => {
                            let ty = self.lookup_var(name).cloned().unwrap_or(Type::Any);
                            properties.push((name.to_string(), Arc::new(ty)));
                        }
                        tsc_rs_ast::ObjLitProp::Spread(e, _) => {
                            let spread_ty = self.check_expr(e);
                            any_spread |= self
                                .merge_spread_into_object_properties(&spread_ty, &mut properties);
                        }
                        _ => {}
                    }
                }
                // tsc's `getSpreadType`: spreading `any` makes the WHOLE object
                // literal `any`. Modelling it as an object with an `[x: string]:
                // any` index signature still failed structural checks against a
                // named target (and leaked into diagnostics), so collapse.
                if any_spread {
                    return Type::Any;
                }
                return Type::ObjectType(ObjectTypeInfo {
                    properties: Self::dedup_object_props_last_wins(properties),
                    call_signatures: Vec::new(),
                    construct_signatures: Vec::new(),
                    index_signature: None,
                    index_signature_name: None,
                    method_names: Vec::new(),
                });
            }
            ExprKind::ArrayLit(elems) if contextual_type.is_some() => {
                // A tuple alias (`type Robot = [number, string]`) or a union
                // with a tuple member contextually types the literal as a tuple.
                let resolved_ctx = {
                    let raw = contextual_type.unwrap();
                    self.tuple_like_context(raw, 0)
                        .unwrap_or_else(|| raw.clone())
                };
                let ctx = &resolved_ctx;
                let array_elem_ctx = match ctx {
                    Type::Array(elem) => Some(elem.as_ref().clone()),
                    _ => None,
                };
                let tuple_ctx = match ctx {
                    Type::Tuple(elements) => Some(elements.as_ref()),
                    _ => None,
                };
                let mut checked_types = Vec::new();
                for elem in elems.iter().flatten() {
                    // A spread of a tuple contributes its elements; a spread of
                    // an array is a rest element in tuple context (tsc
                    // checkArrayLiteral) and its element type otherwise.
                    if let ExprKind::Spread(inner) = &elem.kind {
                        let inner_ty = self.check_expr(inner);
                        let inner_ty = match &inner_ty {
                            Type::TypeReference(..) => self
                                .resolve_type_for_assignability(&inner_ty)
                                .unwrap_or(inner_ty),
                            _ => inner_ty,
                        };
                        match inner_ty {
                            Type::Tuple(elements) => {
                                checked_types.extend(elements.iter().cloned());
                            }
                            Type::Array(item) => {
                                if tuple_ctx.is_some() {
                                    checked_types.push(Type::Rest(Arc::new(Type::Array(item))));
                                } else {
                                    checked_types.push(Type::clone(&item));
                                }
                            }
                            other => checked_types.push(other),
                        }
                        continue;
                    }
                    let index = checked_types.len();
                    let elem_ctx = tuple_ctx
                        .and_then(|elements| Self::tuple_position_type(elements, index))
                        .or_else(|| array_elem_ctx.clone());
                    let ty = if elem_ctx.is_some()
                        && matches!(
                            elem.kind,
                            ExprKind::Arrow(_)
                                | ExprKind::FnExpr(_)
                                | ExprKind::ObjectLit(_)
                                | ExprKind::ArrayLit(_)
                                | ExprKind::Paren(_)
                        ) {
                        self.check_expr_contextual(elem, elem_ctx.as_ref())
                    } else {
                        self.check_expr(elem)
                    };
                    checked_types.push(ty);
                }
                // Preserve the literal's positional shape under a tuple
                // context. This lets assignability enforce required tuple
                // slots (`[]` is not a `[string, ...string[]]`) instead of
                // weakening the source back to an ordinary `any[]`.
                if tuple_ctx.is_some() {
                    // `[...arr]` alone normalizes to the array type.
                    if let [Type::Rest(rest)] = checked_types.as_slice() {
                        if let Type::Array(_) = rest.as_ref() {
                            return Type::clone(rest);
                        }
                    }
                    return Type::Tuple(checked_types.into());
                }
                let mut elem_types = Vec::new();
                for ty in checked_types {
                    if !elem_types.iter().any(|existing: &Type| existing == &ty) {
                        elem_types.push(ty);
                    }
                }
                let elem_ty = if elem_types.is_empty() {
                    Type::Any
                } else if elem_types.len() == 1 {
                    elem_types.into_iter().next().unwrap()
                } else {
                    Type::Union(elem_types.into())
                };
                return Type::Array(Arc::new(elem_ty));
            }
            _ => {}
        }
        self.check_expr(expr)
    }

    /// Check an arrow function or function expression with an optional
    /// contextual type providing parameter types for un-annotated parameters.
    /// Binds `this` for a non-arrow function-like body: an explicit `this`
    /// parameter's annotation, otherwise the plain `this` marker (plain
    /// functions rebind `this`, shadowing an outer explicit binding).
    pub(crate) fn bind_function_this(&mut self, params: &[Param]) {
        let explicit = params.first().and_then(|p| match &p.name.kind {
            PatKind::Ident(n) if n == "this" => p.type_ann.as_ref(),
            _ => None,
        });
        let ty = match explicit {
            Some(ann) => self.resolve_type_node(ann),
            None => Type::This,
        };
        self.declare_var("this", ty);
        self.declare_var(crate::INSTANCE_MEMBER_MARKER, Type::Never);
    }

    /// Mark the current scope as the body of a non-static member of the
    /// innermost enclosing class (see `INSTANCE_MEMBER_MARKER`).
    pub(crate) fn declare_instance_member_marker(&mut self) {
        if let Some(class_name) = self.enclosing_class_names.last().cloned() {
            self.declare_var(
                crate::INSTANCE_MEMBER_MARKER,
                Type::TypeReference(class_name, Arc::from([] as [Type; 0])),
            );
        }
    }

    /// TS2663: `name` is an instance member (own or inherited) of the class
    /// whose non-static member is the current `this` container.
    fn instance_member_class_for_missing_name(&self, name: &str) -> Option<std::string::String> {
        if name.starts_with('#') {
            return None;
        }
        let Some(Type::TypeReference(class_name, _)) =
            self.lookup_var(crate::INSTANCE_MEMBER_MARKER)
        else {
            return None;
        };
        let mut current = Some(class_name.clone());
        let mut guard = 0;
        while let Some(class) = current {
            guard += 1;
            if guard > 32 {
                break;
            }
            let info = self.class_info.get(class.as_str())?;
            // Declared members only: `this.x = …` assignments in a TS
            // constructor add no member (tsc reports TS2304 there).
            if (info.instance_properties.iter().any(|(n, _)| n == name)
                || info.instance_methods.iter().any(|(n, _)| n == name))
                && info.member_locations.contains_key(name)
            {
                return Some(class_name.clone());
            }
            current = info.extends.clone();
        }
        None
    }

    pub(crate) fn check_arrow_or_fn_contextual(
        &mut self,
        expr: &Expr,
        contextual_type: Option<&Type>,
    ) -> Type {
        // Contextual function types commonly arrive through an alias
        // (`type Handler = (value: string) => void`). Expand that alias
        // before extracting parameters; assignability already expands it,
        // but contextual inference must see the same callable shape.
        let resolved_contextual =
            contextual_type.and_then(|ty| self.resolve_type_for_assignability(ty));
        let contextual_type = resolved_contextual.as_ref().or(contextual_type);
        // Extract the contextual function type's params if available
        let ctx_params: Option<&Vec<(std::string::String, Type)>> =
            contextual_type.and_then(|ct| match ct {
                Type::Function(ft) => Some(&ft.params),
                // A callable object type provides contextual params only when
                // its call signatures agree on them (tsc getIntersectedSignatures);
                // an overloaded target leaves the lambda uncontextualized.
                Type::ObjectType(info) => {
                    let first = info.call_signatures.first().map(|s| &s.params);
                    if info
                        .call_signatures
                        .iter()
                        .all(|s| Some(&s.params) == first)
                    {
                        first
                    } else {
                        None
                    }
                }
                _ => None,
            });
        // tsc getContextualSignature: a function expression without its own
        // type parameters that is contextually typed by a GENERIC signature
        // takes over that signature's type parameters — it is generic itself
        // and relates to the signature as such.
        let ctx_generics: Option<(Vec<std::string::String>, Vec<Option<Type>>)> = contextual_type
            .and_then(|ct| {
                let signature = match ct {
                    Type::Function(ft) => Some(ft),
                    Type::ObjectType(info) if info.call_signatures.len() == 1 => {
                        info.call_signatures.first()
                    }
                    _ => None,
                }?;
                if signature.type_params.is_empty() {
                    return None;
                }
                Some((
                    signature.type_params.clone(),
                    signature.type_param_constraints.clone(),
                ))
            });

        match &expr.kind {
            ExprKind::Arrow(arrow) => {
                self.push_scope();
                if let Some(type_params) = &arrow.type_params {
                    for type_param in type_params {
                        self.declare_var(
                            &Self::generic_type_parameter_marker(&type_param.name),
                            Type::Never,
                        );
                    }
                }
                let pushed_type_params =
                    self.push_active_type_param_names(arrow.type_params.as_deref());
                self.check_active_type_param_declarations(arrow.type_params.as_deref());
                self.check_function_like_future_lib_globals(
                    &arrow.params,
                    arrow.return_type.as_ref(),
                );
                self.check_function_like_generic_arity(&arrow.params, arrow.return_type.as_ref());
                // TS17009: tsc's `this` container for the check is the arrow
                // itself (getThisContainer with includeArrowFunctions), so a
                // `this` inside any arrow is never reported against super().
                self.declare_var(THIS_OK_MARKER, Type::Never);
                self.fn_nesting_depth += 1;
                self.jump_function_depth += 1;
                self.arrow_nesting_depth += 1;
                let saved_var_first_types = std::mem::take(&mut self.var_first_types);
                // TS2369: check for parameter properties in arrow functions
                for p in &arrow.params {
                    self.check_parameter_property(p);
                }
                // A leading `this` pseudo-parameter takes no positional slot
                // when aligning to contextual params.
                let this_skew = usize::from(matches!(
                    arrow.params.first().map(|p| &p.name.kind),
                    Some(PatKind::Ident(n)) if n == "this"
                ));
                // A leading `this` in the CONTEXTUAL signature takes no
                // positional slot either: `x => …` against `(this: T, x:
                // number) => number` types `x` as `number`.
                let ctx_this: Option<Type> = ctx_params
                    .and_then(|cps| cps.first())
                    .filter(|(n, _)| n == "this")
                    .map(|(_, t)| t.clone());
                let ctx_positional: Option<Vec<(std::string::String, Type)>> =
                    ctx_params.map(|cps| match cps.first() {
                        Some((n, _)) if n == "this" => cps[1..].to_vec(),
                        _ => cps.clone(),
                    });
                let ctx_params = ctx_positional.as_ref();
                let params: Vec<(std::string::String, Type)> = arrow
                    .params
                    .iter()
                    .enumerate()
                    .map(|(i, p)| {
                        let is_this_param = this_skew == 1 && i == 0;
                        let i = i.saturating_sub(this_skew);
                        let base_name = match &p.name.kind {
                            PatKind::Ident(n) => n.to_string(),
                            _ => "_".to_string(),
                        };
                        // Encode rest/optional in the param name so the
                        // call-site rest-handling at `check_call_against_fn_type`
                        // can detect `...args: any[]` and treat each
                        // variadic argument against the array's element
                        // type instead of the array itself. Without this
                        // prefix every `logSection("a", "b")` /
                        // `console.log("foo")` tripped phantom TS2345
                        // ("string is not assignable to any[]").
                        let pname = if p.dotdotdot {
                            format!("...{}", base_name)
                        } else if p.optional || p.initializer.is_some() {
                            format!("?{}", base_name)
                        } else {
                            base_name
                        };
                        let pty = if is_this_param && p.type_ann.is_none() {
                            // An unannotated own `this` parameter takes the
                            // contextual `this` type, never a positional slot.
                            ctx_this.clone().unwrap_or(Type::Any)
                        } else if let Some(ref ann) = p.type_ann {
                            // Explicit annotation always wins
                            self.resolve_type_node(ann)
                        } else if p.dotdotdot && arrow.type_params.is_none() && ctx_params.is_some()
                        {
                            // Un-annotated REST param: tsc types it as a TUPLE
                            // of the REMAINING contextual parameters (empty
                            // tuple when none remain), not a single positional
                            // slot.
                            let cps = ctx_params.unwrap();
                            let rest: Vec<Type> =
                                cps.iter().skip(i).map(|(_, t)| t.clone()).collect();
                            Type::Tuple(rest.into())
                        } else if let Some(cp) = ctx_params
                            // TypeScript skips contextual parameter typing for a
                            // function expression that declares its OWN type
                            // parameters (a generic arrow/fn is not assignable-
                            // contextual in the same way), so those params stay
                            // implicit `any`.
                            .filter(|_| arrow.type_params.is_none())
                            .and_then(|cps| Self::contextual_param_at(cps, i))
                        {
                            // Use contextual parameter type; a `?` on the
                            // parameter adds `undefined` under strictNullChecks.
                            if p.optional && self.strict_null_checks {
                                Type::flatten_union(vec![cp.1.clone(), Type::Undefined])
                            } else {
                                cp.1.clone()
                            }
                        } else if let Some(ref init) = p.initializer {
                            // Infer type from default value initializer
                            self.infer_expr_type(init)
                        } else {
                            // TS7006: a contextual type was available but supplies
                            // no parameter at this position (or no signature at
                            // all), so the parameter is implicitly `any`.
                            self.report_uncontextualized_parameter(
                                p,
                                // Only a resolved contextual signature that is too
                                // short (and has no rest parameter) proves the
                                // parameter is implicitly `any`.
                                ctx_params.is_some_and(|cps| {
                                    !cps.iter().any(|(name, _)| name.starts_with("..."))
                                }),
                            );
                            Type::Any
                        };
                        if ctx_params.is_some()
                            && p.type_ann.is_none()
                            && arrow.type_params.is_none()
                            && self.contextual_type_is_unresolved_parameter(&pty)
                        {
                            if let PatKind::Ident(name) = &p.name.kind {
                                self.declare_var(
                                    &Self::contextual_unresolved_parameter_marker(name),
                                    Type::Never,
                                );
                            }
                        }
                        self.record_pattern_types(&p.name, &pty);
                        self.declare_pattern_vars(&p.name, pty.clone());
                        self.completion_mark_parameter(p);
                        self.mark_optional_parameter(p);
                        (pname, pty)
                    })
                    .collect();
                // Push declared return type for TS2322 checking in return statements
                let declared_ret_ctx = arrow
                    .return_type
                    .as_ref()
                    .map(|rt| self.resolve_type_node(rt));
                let declared_ret_ctx = self.unwrap_async_return(declared_ret_ctx, arrow.is_async);
                self.return_type_stack.push(declared_ret_ctx);
                self.return_is_async_stack.push(arrow.is_async);
                self.generator_stack.push(false);
                // Extract contextual return type for passing to body expression
                let ctx_ret_type: Option<Type> = contextual_type.and_then(|ct| match ct {
                    Type::Function(ft) => Some(Type::clone(&ft.return_type)),
                    _ => None,
                });
                let ret_type = match &arrow.body {
                    ArrowBody::Expr(e) => {
                        // Pass contextual return type to nested function/object expressions
                        let inferred = if ctx_ret_type.is_some()
                            && matches!(
                                e.kind,
                                ExprKind::Arrow(_) | ExprKind::FnExpr(_) | ExprKind::ObjectLit(_)
                            ) {
                            self.check_expr_contextual(e, ctx_ret_type.as_ref())
                        } else {
                            self.check_expr(e)
                        };
                        // The contextual return type decides literal widening here.
                        let inferred = if ctx_ret_type
                            .as_ref()
                            .is_some_and(|ret| self.contextual_type_keeps_literals(ret, 3))
                        {
                            inferred
                        } else {
                            self.widen_fresh_return_expr_type(e, inferred)
                        };
                        // Without strictNullChecks a nullish return widens to
                        // `any` (tsc getWidenedType), whatever the contextual
                        // return type.
                        let inferred = if !self.strict_null_checks
                            && matches!(inferred, Type::Null | Type::Undefined)
                        {
                            Type::Any
                        } else {
                            inferred
                        };
                        // TS2322: check arrow expression body against declared return type
                        if let Some(Some(expected_ret)) = self.return_type_stack.last().cloned() {
                            if !self.is_assignable_to(&inferred, &expected_ret)
                                && !matches!(expected_ret, Type::Any | Type::Error)
                                && !matches!(inferred, Type::Any | Type::Error)
                            {
                                let widened =
                                    self.widen_for_message(Some(e), &inferred, &expected_ret);
                                let return_span = self.error_span_for_expr(e);
                                self.push_not_assignable(&widened, &expected_ret, return_span);
                            }
                        }
                        match arrow.return_type.as_ref() {
                            Some(t) => self.resolve_type_node(t),
                            // Async concise-body arrow (`async () => 42`)
                            // returns `Promise<42>`, not `42`.
                            None if arrow.is_async => Self::wrap_async_inferred_return(inferred),
                            None => inferred,
                        }
                    }
                    ArrowBody::Block(stmts) => {
                        self.hoist_block_declarations(stmts);
                        for s in stmts {
                            self.check_stmt(s);
                        }
                        self.check_function_completion(
                            stmts,
                            arrow.return_type.as_ref(),
                            self.error_span_for_expr(expr),
                            arrow.is_async,
                            false,
                        );
                        match arrow.return_type.as_ref() {
                            Some(t) => self.resolve_type_node(t),
                            None => {
                                let inferred = self.infer_return_type_from_block(stmts);
                                // An async arrow always returns a Promise, so
                                // an INFERRED body-completion type must be
                                // wrapped — otherwise `async () => { await x; }`
                                // types as `() => void` and fails assignment to
                                // `() => Promise<void>`.
                                if arrow.is_async {
                                    Self::wrap_async_inferred_return(inferred)
                                } else {
                                    inferred
                                }
                            }
                        }
                    }
                };
                self.return_type_stack.pop();
                self.return_is_async_stack.pop();
                self.generator_stack.pop();
                let arrow_predicate = arrow
                    .return_type
                    .as_ref()
                    .and_then(|rt| self.extract_type_predicate(rt));
                self.fn_nesting_depth -= 1;
                self.jump_function_depth -= 1;
                self.arrow_nesting_depth -= 1;
                self.var_first_types = saved_var_first_types;
                self.pop_active_type_param_names(pushed_type_params);
                self.pop_scope();
                let (type_params, type_param_constraints) =
                    match (&arrow.type_params, &ctx_generics) {
                        (None, Some((names, constraints))) => (names.clone(), constraints.clone()),
                        _ => (
                            arrow
                                .type_params
                                .as_ref()
                                .map(|tps| tps.iter().map(|tp| tp.name.clone()).collect())
                                .unwrap_or_default(),
                            self.resolve_type_param_constraints(arrow.type_params.as_deref()),
                        ),
                    };
                Type::Function(FunctionType {
                    type_param_constraints,
                    params,
                    return_type: Arc::new(ret_type),
                    type_params,
                    type_param_defaults: self
                        .resolve_type_param_defaults(arrow.type_params.as_deref()),
                    type_predicate: arrow_predicate,
                })
            }
            ExprKind::FnExpr(fn_decl) => {
                self.push_scope();
                if let Some(type_params) = &fn_decl.type_params {
                    for type_param in type_params {
                        self.declare_var(
                            &Self::generic_type_parameter_marker(&type_param.name),
                            Type::Never,
                        );
                    }
                }
                let pushed_type_params =
                    self.push_active_type_param_names(fn_decl.type_params.as_deref());
                self.check_active_type_param_declarations(fn_decl.type_params.as_deref());
                self.check_function_like_future_lib_globals(
                    &fn_decl.params,
                    fn_decl.return_type.as_ref(),
                );
                self.check_function_like_generic_arity(
                    &fn_decl.params,
                    fn_decl.return_type.as_ref(),
                );
                self.fn_nesting_depth += 1;
                self.jump_function_depth += 1;
                let saved_var_first_types = std::mem::take(&mut self.var_first_types);
                // `arguments` is implicitly available inside all non-arrow functions
                self.declare_var("arguments", Type::Any);
                self.declare_var(SUPER_PROPERTY_BARRIER_MARKER, Type::Never);
                self.declare_var(THIS_OK_MARKER, Type::Never);
                self.bind_function_this(&fn_decl.params);
                // Named function expression: name is available inside the body
                if let Some(ref name) = fn_decl.name {
                    self.declare_var(name, Type::Any);
                }
                // TS2369: check for parameter properties in function expressions
                for p in &fn_decl.params {
                    self.check_parameter_property(p);
                }
                let this_skew = usize::from(matches!(
                    fn_decl.params.first().map(|p| &p.name.kind),
                    Some(PatKind::Ident(n)) if n == "this"
                ));
                // A leading `this` in the CONTEXTUAL signature takes no
                // positional slot either: `x => …` against `(this: T, x:
                // number) => number` types `x` as `number`.
                let ctx_this: Option<Type> = ctx_params
                    .and_then(|cps| cps.first())
                    .filter(|(n, _)| n == "this")
                    .map(|(_, t)| t.clone());
                let ctx_positional: Option<Vec<(std::string::String, Type)>> =
                    ctx_params.map(|cps| match cps.first() {
                        Some((n, _)) if n == "this" => cps[1..].to_vec(),
                        _ => cps.clone(),
                    });
                let ctx_params = ctx_positional.as_ref();
                // A contextual `this` types the body's `this` when the
                // function declares no `this` parameter of its own.
                if let Some(ctx_this) = ctx_this.clone() {
                    if this_skew == 0 {
                        self.declare_var("this", ctx_this);
                    }
                }
                let params: Vec<(std::string::String, Type)> = fn_decl
                    .params
                    .iter()
                    .enumerate()
                    .map(|(i, p)| {
                        let is_this_param = this_skew == 1 && i == 0;
                        let i = i.saturating_sub(this_skew);
                        let base_name = match &p.name.kind {
                            PatKind::Ident(n) => n.to_string(),
                            _ => "_".to_string(),
                        };
                        // See the matching block in `ExprKind::Arrow`
                        // above for the rationale: `...` / `?` prefixes
                        // are how the call-site rest/optional handling
                        // recognizes variadic and skippable parameters.
                        let pname = if p.dotdotdot {
                            format!("...{}", base_name)
                        } else if p.optional || p.initializer.is_some() {
                            format!("?{}", base_name)
                        } else {
                            base_name
                        };
                        let pty = if is_this_param && p.type_ann.is_none() {
                            // An unannotated own `this` parameter takes the
                            // contextual `this` type, never a positional slot.
                            ctx_this.clone().unwrap_or(Type::Any)
                        } else if let Some(ref ann) = p.type_ann {
                            self.resolve_type_node(ann)
                        } else if p.dotdotdot
                            && fn_decl.type_params.is_none()
                            && ctx_params.is_some()
                        {
                            // Rest param: tuple of remaining ctx params (see
                            // the Arrow arm).
                            let cps = ctx_params.unwrap();
                            let rest: Vec<Type> =
                                cps.iter().skip(i).map(|(_, t)| t.clone()).collect();
                            Type::Tuple(rest.into())
                        } else if let Some(cp) = ctx_params
                            // See the Arrow arm: a function expression with its
                            // own type parameters skips contextual param typing.
                            .filter(|_| fn_decl.type_params.is_none())
                            .and_then(|cps| Self::contextual_param_at(cps, i))
                        {
                            // A `?` on the parameter adds `undefined` under strict.
                            if p.optional && self.strict_null_checks {
                                Type::flatten_union(vec![cp.1.clone(), Type::Undefined])
                            } else {
                                cp.1.clone()
                            }
                        } else if let Some(ref init) = p.initializer {
                            self.infer_expr_type(init)
                        } else {
                            self.report_uncontextualized_parameter(
                                p,
                                // Only a resolved contextual signature that is too
                                // short (and has no rest parameter) proves the
                                // parameter is implicitly `any`.
                                ctx_params.is_some_and(|cps| {
                                    !cps.iter().any(|(name, _)| name.starts_with("..."))
                                }),
                            );
                            Type::Any
                        };
                        if ctx_params.is_some()
                            && p.type_ann.is_none()
                            && fn_decl.type_params.is_none()
                            && self.contextual_type_is_unresolved_parameter(&pty)
                        {
                            if let PatKind::Ident(name) = &p.name.kind {
                                self.declare_var(
                                    &Self::contextual_unresolved_parameter_marker(name),
                                    Type::Never,
                                );
                            }
                        }
                        self.record_pattern_types(&p.name, &pty);
                        self.declare_pattern_vars(&p.name, pty.clone());
                        self.completion_mark_parameter(p);
                        self.mark_optional_parameter(p);
                        (pname, pty)
                    })
                    .collect();
                // Push declared return type for TS2322 checking in return statements
                let declared_ret_fn = fn_decl
                    .return_type
                    .as_ref()
                    .map(|rt| self.resolve_type_node(rt));
                let declared_ret_fn = self.unwrap_async_return(declared_ret_fn, fn_decl.is_async);
                self.return_type_stack.push(declared_ret_fn);
                self.return_is_async_stack.push(fn_decl.is_async);
                self.generator_stack.push(fn_decl.is_generator);
                if let Some(ref body) = fn_decl.body {
                    self.hoist_block_declarations(body);
                    for s in body {
                        self.check_stmt(s);
                    }
                    self.check_function_completion(
                        body,
                        fn_decl.return_type.as_ref(),
                        self.error_span_for_expr(expr),
                        fn_decl.is_async,
                        fn_decl.is_generator,
                    );
                }
                self.return_type_stack.pop();
                self.return_is_async_stack.pop();
                self.generator_stack.pop();
                let ret = if let Some(ref rt) = fn_decl.return_type {
                    self.resolve_type_node(rt)
                } else if let Some(ref body) = fn_decl.body {
                    self.infer_return_type_from_block(body)
                } else {
                    Type::Any
                };
                let fn_predicate = fn_decl
                    .return_type
                    .as_ref()
                    .and_then(|rt| self.extract_type_predicate(rt));
                self.fn_nesting_depth -= 1;
                self.jump_function_depth -= 1;
                self.var_first_types = saved_var_first_types;
                self.pop_active_type_param_names(pushed_type_params);
                self.pop_scope();
                let (type_params, type_param_constraints) =
                    match (&fn_decl.type_params, &ctx_generics) {
                        (None, Some((names, constraints))) => (names.clone(), constraints.clone()),
                        _ => (
                            fn_decl
                                .type_params
                                .as_ref()
                                .map(|tps| tps.iter().map(|tp| tp.name.clone()).collect())
                                .unwrap_or_default(),
                            self.resolve_type_param_constraints(fn_decl.type_params.as_deref()),
                        ),
                    };
                Type::Function(FunctionType {
                    type_param_constraints,
                    params,
                    return_type: Arc::new(ret),
                    type_params,
                    type_param_defaults: self
                        .resolve_type_param_defaults(fn_decl.type_params.as_deref()),
                    type_predicate: fn_predicate,
                })
            }
            _ => self.check_expr(expr),
        }
    }

    pub(crate) fn check_expr(&mut self, expr: &Expr) -> Type {
        if !self.enter_recursion() {
            return Type::Any;
        }
        let result = self.check_expr_inner(expr);
        self.exit_recursion();
        result
    }

    pub(crate) fn check_expr_inner(&mut self, expr: &Expr) -> Type {
        let ty = match &expr.kind {
            ExprKind::NumLit(val) => Type::NumberLiteral(val.to_string()),
            ExprKind::BigIntLit(val) => Type::BigIntLiteral(Self::normalize_bigint_literal(val)),
            ExprKind::StrLit(val) => {
                Type::StringLiteral(self.cooked_property_literal(val, expr.span))
            }
            ExprKind::BoolLit(v) => Type::BooleanLiteral(*v),
            ExprKind::NullLit => Type::Null,
            ExprKind::NoSubstTemplate(val) => {
                Type::StringLiteral(self.cooked_property_literal(val, expr.span))
            }
            ExprKind::Template(tpl) => {
                for e in &tpl.exprs {
                    self.check_expr(e);
                }
                Type::String
            }
            ExprKind::TaggedTemplate(tt) => {
                // A tag is called like a callee (`(0, obj.fn)\`\`` is an
                // indirect call, not an unused comma operand).
                let saved_callee_pos = self.in_callee_position;
                self.in_callee_position = true;
                let tag_ty = self.check_expr(&tt.tag);
                self.in_callee_position = saved_callee_pos;
                for e in &tt.quasi.exprs {
                    self.check_expr(e);
                }
                // Infer return type from tag function
                match tag_ty {
                    Type::Function(ft) => Type::clone(&ft.return_type),
                    _ => Type::Any,
                }
            }
            ExprKind::This => {
                // TS17009: `this` before `super()` in a derived-class ctor.
                // Marker race: PENDING nearer than THIS_OK ⇒ error. Pierces
                // arrows; regular fns/methods declare THIS_OK (this rebinds).
                // Only `this` at the constructor's own nesting depth counts:
                // inside any nested function or arrow the container is that
                // function, which tsc never checks against super().
                if self.super_pending_depth == Some(self.fn_nesting_depth)
                    && matches!(
                        self.nearer_binding_is(SUPER_PENDING_MARKER, THIS_OK_MARKER),
                        Some(true)
                    )
                {
                    self.diagnostics.push(error_this_before_super(expr.span));
                }
                // In a static member `this` is the class constructor itself.
                match self.static_this_class.clone() {
                    Some(class_name) => Type::TypeReference(
                        format!("typeof {class_name}"),
                        Arc::from(Vec::<Type>::new()),
                    ),
                    // An explicit `this` parameter (or a contextual `this`
                    // type) binds `this` in the function's scope; arrows see
                    // through to it.
                    None => match self
                        .lookup_narrowed("this")
                        .or_else(|| self.lookup_var("this").cloned())
                    {
                        Some(bound) if !matches!(bound, Type::This) => bound,
                        _ => Type::This,
                    },
                }
            }
            ExprKind::Super => {
                // Resolve to parent class instance type
                if let Some(class_name) = self.enclosing_class_names.last() {
                    let base = self
                        .class_info
                        .get(class_name)
                        .and_then(|info| info.extends.clone());
                    if let Some(base_name) = base {
                        self.get_class_instance_type(&base_name)
                    } else {
                        Type::Any
                    }
                } else {
                    Type::Any
                }
            }
            ExprKind::Ident(name) => {
                // TS2815: `arguments` referenced in a property initializer or
                // static block. The marker-vs-`arguments` scope race encodes
                // "no function-like between the reference and the initializer"
                // (arrows bind nothing; functions/methods declare `arguments`,
                // which wins the race and legalizes the reference).
                if name == "arguments"
                    && matches!(
                        self.nearer_binding_is(ARGS_BLOCKED_MARKER, "arguments"),
                        Some(true)
                    )
                {
                    self.diagnostics
                        .push(error_arguments_in_class_field(expr.span));
                    return Type::Error;
                }
                // TS2708: a namespace with no value meaning in value position.
                if self.uninstantiated_namespace_names.contains(name.as_str())
                    && self.binding_scope_is_root(name).unwrap_or(true)
                {
                    if self
                        .reported_duplicate_spans
                        .insert((2708, expr.span.start, expr.span.end))
                    {
                        self.diagnostics.push(Diagnostic {
                            code: 2708,
                            message: format!("Cannot use namespace '{name}' as a value."),
                            category: DiagnosticCategory::Error,
                            file_name: None,
                            span: Some(expr.span),
                            related: None,
                        });
                    }
                    return Type::Error;
                }
                // NOTE: used_names population is handled by collect_all_used_names()
                // which correctly distinguishes read vs write-only positions.
                // TS2454: check if used before being assigned.
                self.check_read_before_assignment(name, expr.span);
                if let Some(ty) = self.lookup_var(name).cloned() {
                    // TS2301: the name resolves outside the initializer but a
                    // constructor local of the same name would capture it in
                    // the emitted constructor body.
                    if let Some((member, locals, initializer_scope)) =
                        self.ctor_params_in_initializer.as_ref()
                    {
                        let bound_inside_initializer = self
                            .nearest_binding_scope(name)
                            .is_some_and(|scope| scope > *initializer_scope);
                        if !bound_inside_initializer
                            && locals.iter().any(|local| local == name.as_str())
                        {
                            let member = member.clone();
                            if !self.diagnostics.iter().any(|diagnostic| {
                                diagnostic.code == 2301 && diagnostic.span == Some(expr.span)
                            }) {
                                self.diagnostics.push(
                                    crate::diagnostics::error_initializer_references_ctor_param(
                                        &member, name, expr.span,
                                    ),
                                );
                            }
                            return Type::Error;
                        }
                    }
                    if self.report_value_position_global(name, expr.span)
                        || (stdlib::is_future_lib_global_diagnostic_name(name)
                            && self.report_future_lib_global(name, expr.span))
                    {
                        Type::Error
                    } else {
                        ty
                    }
                } else {
                    // Special built-in identifiers
                    match name.as_str() {
                        "undefined" => Type::Undefined,
                        "NaN" | "Infinity" => Type::Number,
                        // JS/TS built-in globals
                        "console" | "Math" | "JSON" | "Object" | "Array" | "String" | "Number"
                        | "Boolean" | "Symbol" | "Promise" | "Map" | "Set" | "WeakMap"
                        | "WeakSet" | "Error" | "TypeError" | "RangeError"
                        | "EvalError" | "ReferenceError" | "SyntaxError" | "URIError"
                        | "Date" | "RegExp"
                        | "globalThis" | "window" | "document" | "process" | "require"
                        | "module" | "exports" | "setTimeout" | "setInterval" | "clearTimeout"
                        | "clearInterval" | "parseInt" | "parseFloat" | "isNaN" | "isFinite"
                        // Web API globals
                        | "URL" | "URLSearchParams" | "Headers" | "Request" | "Response"
                        | "fetch" | "FormData" | "Blob" | "File" | "FileReader"
                        | "AbortController" | "AbortSignal" | "Event" | "CustomEvent"
                        | "EventTarget" | "WebSocket" | "Worker" | "SharedWorker"
                        | "ReadableStream" | "WritableStream" | "TransformStream"
                        | "TextEncoder" | "TextDecoder" | "Crypto" | "crypto"
                        | "performance" | "navigator" | "location" | "history"
                        | "localStorage" | "sessionStorage" | "indexedDB"
                        | "alert" | "confirm" | "prompt" | "atob" | "btoa"
                        | "queueMicrotask" | "structuredClone" | "reportError"
                        | "requestAnimationFrame" | "cancelAnimationFrame"
                        | "requestIdleCallback" | "cancelIdleCallback"
                        | "MutationObserver" | "IntersectionObserver" | "ResizeObserver"
                        | "PerformanceObserver" | "MessageChannel" | "MessagePort"
                        | "BroadcastChannel" | "Cache" | "CacheStorage"
                        | "XMLHttpRequest" | "Image" | "Audio" | "MediaRecorder"
                        // Deprecated but still used
                        | "unescape" | "escape"
                        // Node.js globals
                        | "Buffer" | "global" | "__dirname" | "__filename"
                        | "setImmediate" | "clearImmediate"
                        // TypeScript/JS utility globals
                        | "Proxy" | "Reflect" | "WeakRef" | "FinalizationRegistry"
                        // ES2015+ iterator-protocol globals (esnext declares
                        // Iterator as a real global class; builtinIterator FPs)
                        | "Iterator" | "AsyncIterator" | "IteratorObject"
                        | "AsyncIteratorObject" | "AggregateError"
                        | "Intl" | "Atomics" | "SharedArrayBuffer" | "ArrayBuffer"
                        | "DataView" | "Float16Array" | "Float32Array" | "Float64Array"
                        | "Int8Array" | "Int16Array" | "Int32Array"
                        | "Uint8Array" | "Uint16Array" | "Uint32Array"
                        | "Uint8ClampedArray" | "BigInt64Array" | "BigUint64Array"
                        | "BigInt" | "AggregateError"
                        // Encoding/utility
                        | "encodeURIComponent" | "decodeURIComponent"
                        | "encodeURI" | "decodeURI"
                        // Dynamic import (treated as function-like keyword)
                        | "import"
                        // DOM
                        | "HTMLElement" | "Element" | "Node" | "NodeList"
                        | "DocumentFragment" | "DOMParser" | "XMLSerializer"
                        | "getComputedStyle" | "matchMedia"
                        | "ResizeObserverEntry" | "IntersectionObserverEntry"
                        // HTML/DOM element types
                        | "HTMLDivElement" | "HTMLImageElement" | "HTMLInputElement"
                        | "HTMLTextAreaElement" | "HTMLButtonElement" | "HTMLFormElement"
                        | "HTMLSelectElement" | "HTMLCanvasElement" | "HTMLVideoElement"
                        | "HTMLAudioElement" | "HTMLAnchorElement" | "HTMLSpanElement"
                        | "HTMLParagraphElement" | "HTMLHeadingElement"
                        | "HTMLTableElement" | "HTMLTableRowElement" | "SVGElement"
                        | "SVGSVGElement" | "SVGPathElement"
                        | "MouseEvent" | "KeyboardEvent" | "FocusEvent" | "InputEvent"
                        | "ChangeEvent" | "DragEvent" | "TouchEvent" | "WheelEvent"
                        | "PointerEvent" | "ClipboardEvent" | "AnimationEvent"
                        | "TransitionEvent" | "SubmitEvent" | "BeforeUnloadEvent"
                        | "PopStateEvent" | "HashChangeEvent" | "PageTransitionEvent"
                        // React hooks (commonly imported)
                        | "useState" | "useEffect" | "useCallback" | "useMemo"
                        | "useRef" | "useContext" | "useReducer" | "useId"
                        | "useLayoutEffect" | "useImperativeHandle"
                        | "useDeferredValue" | "useTransition" | "useSyncExternalStore"
                        | "useDebugValue" | "useInsertionEffect"
                        // React types
                        | "React" | "ReactNode" | "ReactElement" | "JSX"
                        // Events and notifications
                        | "EventSource" | "Notification" | "ServiceWorker"
                        | "BarcodeDetector" | "SpeechRecognition"
                        // Web APIs - Audio, Clipboard, etc.
                        | "AudioContext" | "OfflineAudioContext" | "OscillatorNode"
                        | "GainNode" | "AudioBuffer" | "AudioBufferSourceNode"
                        | "ClipboardItem" | "DOMException" | "ErrorEvent"
                        | "PaymentRequest" | "Geolocation"
                        // Function/constructor types
                        | "Function" | "GeneratorFunction" | "AsyncFunction"
                        // Next.js/React server
                        | "caches" | "addEventListener" | "removeEventListener"
                        // AMD/module loaders
                        | "define"
                        // TS keywords that can appear as identifiers in expression context
                        | "public" | "private" | "protected" | "static"
                        | "readonly" | "override" | "accessor"
                        | "declare" | "module"
                        | "is" | "asserts" | "infer" | "out" | "satisfies"
                        | "async" | "await" | "of" | "from" | "as" | "get" | "set"
                        // Other well-known globals
                        | "eval" | "importScripts" | "postMessage"
                        | "WScript" | "ActiveXObject" | "Enumerator"
                        | "VBArray" | "ScriptEngine" | "CollectGarbage"
                        | "self" | "top" | "parent" | "frames"
                        | "open" | "close" | "print" | "stop" | "focus" | "blur"
                        | "scroll" | "scrollTo" | "scrollBy"
                        | "moveBy" | "moveTo" | "resizeBy" | "resizeTo"
                        | "getSelection" => {
                            if self.report_value_position_global(name, expr.span)
                                || self.report_future_lib_global(name, expr.span)
                            {
                                Type::Error
                            } else {
                                Type::Any
                            }
                        }
                        _ => {
                            // Don't report errors for parser error recovery tokens
                            // or single-char identifiers that are likely type params
                            if name == "<error>" || name.starts_with('<') {
                                Type::Any
                            } else if self.provisional_param_names.contains(name.as_str()) {
                                // An earlier parameter referenced from a later
                                // parameter's initializer, before binding.
                                Type::Any
                            } else if self.with_depth > 0 {
                                // Inside `with (obj)` an unknown name may be a
                                // property of the object: `any`, no error.
                                Type::Any
                            } else if stdlib::stdlib_member_availability(&self.compiler_options)
                                .is_some_and(|availability| availability.value_is_active(name))
                            {
                                // A global value the configured libs declare but
                                // the builtin model does not carry (`Temporal`,
                                // `Intl`): known, untyped.
                                Type::Any
                            } else if self.is_type_only_name(name) {
                                // TS2693: type used as value
                                self.diagnostics
                                    .push(error_type_used_as_value(name, expr.span));
                                Type::Error
                            } else if let Some(class_name) =
                                self.enclosing_class_with_static_member(name)
                            {
                                // TS2662: the name is a static member of an
                                // enclosing class; a bare reference must be
                                // qualified (`C.foo`, not `foo`).
                                self.diagnostics.push(error_cannot_find_name_static_member(
                                    name,
                                    &class_name,
                                    expr.span,
                                ));
                                Type::Error
                            } else if self.instance_member_class_for_missing_name(name).is_some() {
                                // TS2663: an instance member of the class whose
                                // non-static member is the `this` container.
                                self.diagnostics.push(Diagnostic {
                                    code: 2663,
                                    message: format!(
                                        "Cannot find name '{name}'. Did you mean the instance member 'this.{name}'?"
                                    ),
                                    category: DiagnosticCategory::Error,
                                    file_name: None,
                                    span: Some(expr.span),
                                    related: None,
                                });
                                Type::Error
                            } else if self
                                .ctor_params_in_initializer
                                .as_ref()
                                .is_some_and(|(_, params, _)| params.iter().any(|p| p == name.as_str()))
                            {
                                // TS2301: the name IS a constructor parameter
                                // (it resolves in tsc), referenced from an
                                // instance member initializer.
                                let (member, _, _) = self.ctor_params_in_initializer.clone().unwrap();
                                self.diagnostics.push(
                                    crate::diagnostics::error_initializer_references_ctor_param(
                                        &member, name, expr.span,
                                    ),
                                );
                                Type::Error
                            } else if let Some(sugg) = self.attempt_spelling_suggestion(name) {
                                // TS2552: a close visible binding exists.
                                let related = self.declared_here_note(&sugg);
                                self.diagnostics.push(Diagnostic {
                                    code: 2552,
                                    message: format!(
                                        "Cannot find name '{}'. Did you mean '{}'?",
                                        name, sugg
                                    ),
                                    category: DiagnosticCategory::Error,
                                    file_name: None,
                                    span: Some(expr.span),
                                    related,
                                });
                                Type::Error
                            } else {
                                self.diagnostics.push(
                                    crate::diagnostics::error_cannot_find_name_in_expression(
                                        name, expr.span,
                                    ),
                                );
                                Type::Error
                            }
                        }
                    }
                }
            }
            ExprKind::Binary(bin) => {
                let logical_condition = matches!(
                    bin.op,
                    BinaryOp::LogAnd | BinaryOp::LogOr | BinaryOp::NullCoal
                );
                if logical_condition && self.uncalled_condition_depth == 0 {
                    self.check_uncalled_function_logical_value(expr);
                }
                if logical_condition {
                    self.uncalled_condition_depth += 1;
                }
                // `#x in obj` (ergonomic brand check): the private name is not
                // a binding to resolve; it must sit inside a class body.
                let private_brand_check = bin.op == BinaryOp::In
                    && matches!(&bin.left.kind, ExprKind::Ident(n) if n.starts_with('#'));
                let left_ty = if private_brand_check {
                    if self.enclosing_class_names.is_empty() {
                        self.diagnostics.push(
                            crate::diagnostics::error_private_identifier_outside_class(
                                bin.left.span,
                            ),
                        );
                    }
                    Type::Any
                } else {
                    self.check_expr(&bin.left)
                };
                // `&&` evaluates its right operand only when the left is truthy,
                // so the right operand is checked with the left's truthy
                // control-flow facts in scope. Without this, the pervasive
                // `a?.b && a.b.c` / `o.v && o.v.foo()` guard mis-fired TS2532 on
                // the second access (the ternary `a?.b ? a.b.c : x` already
                // narrows via its own arm — this brings `&&` to parity).
                // `||` is the dual: its right operand is evaluated only when the
                // left is FALSY, so check it with the left's falsy facts in
                // scope. That is what lets `!row || !row.a` check `!row.a`
                // knowing `row` is non-null (otherwise the guard itself
                // mis-fired TS2531 on its own second operand).
                let and_narrowed = match bin.op {
                    BinaryOp::LogAnd => {
                        let (consequent, _alternate) = self.analyze_narrowing(&bin.left);
                        self.push_scope();
                        for (name, ty) in &consequent {
                            self.narrow_var(name, ty.clone());
                        }
                        true
                    }
                    BinaryOp::LogOr => {
                        let (_consequent, alternate) = self.analyze_narrowing(&bin.left);
                        self.push_scope();
                        for (name, ty) in &alternate {
                            self.narrow_var(name, ty.clone());
                        }
                        true
                    }
                    _ => false,
                };
                let right_ty = self.check_expr(&bin.right);
                if and_narrowed {
                    self.pop_scope();
                }
                let result = match bin.op {
                    BinaryOp::Add => {
                        // string + anything = string; anything + string = string
                        let apparent_left =
                            self.binary_operator_type_without_strict_nullish(&left_ty);
                        let apparent_right =
                            self.binary_operator_type_without_strict_nullish(&right_ty);
                        let l_is_string = self.binary_operator_type_is_string_like(&apparent_left);
                        let r_is_string = self.binary_operator_type_is_string_like(&apparent_right);
                        if l_is_string || r_is_string {
                            self.report_addition_operator_incompatibility(
                                "+", &left_ty, &right_ty, &bin.left, &bin.right, expr.span,
                            );
                            Type::String
                        } else if self.strict_null_checks
                            && (matches!(bin.left.kind, ExprKind::NullLit)
                                || matches!(&bin.left.kind, ExprKind::Ident(n) if n == "undefined")
                                || matches!(bin.right.kind, ExprKind::NullLit)
                                || matches!(&bin.right.kind, ExprKind::Ident(n) if n == "undefined"))
                            && (self.report_nullish_literal_operand(&bin.left)
                                | self.report_nullish_literal_operand(&bin.right))
                        {
                            // tsc: neither side string-like → checkNonNullType
                            // on both; a nullish literal is the whole error
                            // (under strictNullChecks only — otherwise null is
                            // string-assignable and the pair check runs).
                            Type::Any
                        } else if {
                            // A nullish-TYPED operand reports and then takes
                            // part with its non-nullable remainder.
                            if self.strict_null_checks {
                                self.report_nullish_operand(&bin.left, &left_ty);
                                self.report_nullish_operand(&bin.right, &right_ty);
                            }
                            matches!(apparent_left, Type::Any)
                        } || matches!(apparent_right, Type::Any)
                        {
                            // tsc: `any + x` (x not string-like) is `any`.
                            Type::Any
                        } else {
                            let incompatible = self.report_addition_operator_incompatibility(
                                "+", &left_ty, &right_ty, &bin.left, &bin.right, expr.span,
                            );
                            if !incompatible
                                && self.binary_operator_type_is_bigint_like(&apparent_left)
                                && self.binary_operator_type_is_bigint_like(&apparent_right)
                            {
                                Type::BigInt
                            } else {
                                Type::Number
                            }
                        }
                    }
                    BinaryOp::Sub
                    | BinaryOp::Mul
                    | BinaryOp::Div
                    | BinaryOp::Mod
                    | BinaryOp::Exp
                    | BinaryOp::BitAnd
                    | BinaryOp::BitOr
                    | BinaryOp::BitXor
                    | BinaryOp::Shl
                    | BinaryOp::Shr
                    | BinaryOp::UShr => {
                        // Both-boolean bitwise ops get TS2447 with a logical
                        // suggestion; otherwise TS2362/TS2363 (operands must
                        // be any/number/bigint/enum — wrapper `Number`,
                        // string, boolean, void, type params are not).
                        let bool_pair = matches!(
                            bin.op,
                            BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor
                        ) && self.is_boolean_operand(&left_ty)
                            && self.is_boolean_operand(&right_ty);
                        // A nullish literal operand is its own error (TS18050,
                        // both strictness modes); the other operand still gets
                        // the arithmetic operand check.
                        let left_nullish_literal = self.report_nullish_literal_operand(&bin.left);
                        let right_nullish_literal = self.report_nullish_literal_operand(&bin.right);
                        // A nullish-TYPED name reports (`'y' is possibly
                        // 'undefined'`) and continues as its non-nullable
                        // remainder for the operand checks below.
                        let left_nullish_name = !left_nullish_literal
                            && self.report_nullish_operand(&bin.left, &left_ty);
                        let right_nullish_name = !right_nullish_literal
                            && self.report_nullish_operand(&bin.right, &right_ty);
                        let left_ty = if left_nullish_name {
                            self.binary_operator_type_without_strict_nullish(&left_ty)
                        } else {
                            left_ty.clone()
                        };
                        let right_ty = if right_nullish_name {
                            self.binary_operator_type_without_strict_nullish(&right_ty)
                        } else {
                            right_ty.clone()
                        };
                        if left_nullish_literal || right_nullish_literal {
                            if !left_nullish_literal && self.arithmetic_operand_invalid(&left_ty) {
                                self.diagnostics.push(error_arithmetic_lhs(bin.left.span));
                            }
                            if !right_nullish_literal && self.arithmetic_operand_invalid(&right_ty)
                            {
                                self.diagnostics.push(error_arithmetic_rhs(bin.right.span));
                            }
                        } else if bool_pair {
                            let (op, sug) = match bin.op {
                                BinaryOp::BitAnd => ("&", "&&"),
                                BinaryOp::BitOr => ("|", "||"),
                                _ => ("^", "!=="),
                            };
                            self.diagnostics
                                .push(error_boolean_operator(op, sug, expr.span));
                        } else if self.report_concrete_numeric_operator_incompatibility(
                            match bin.op {
                                BinaryOp::Sub => "-",
                                BinaryOp::Mul => "*",
                                BinaryOp::Div => "/",
                                BinaryOp::Mod => "%",
                                BinaryOp::Exp => "**",
                                BinaryOp::BitAnd => "&",
                                BinaryOp::BitOr => "|",
                                BinaryOp::BitXor => "^",
                                BinaryOp::Shl => "<<",
                                BinaryOp::Shr => ">>",
                                BinaryOp::UShr => ">>>",
                                _ => unreachable!(),
                            },
                            &left_ty,
                            &right_ty,
                            expr.span,
                        ) {
                        } else {
                            // TS18050: null/undefined operands get their own
                            // code, not the generic TS2362/2363.
                            let nullish = |t: &Type| match t {
                                Type::Null => Some("null"),
                                Type::Undefined => Some("undefined"),
                                _ => None,
                            };
                            match nullish(&left_ty) {
                                Some(v) if self.strict_null_checks => {
                                    self.diagnostics.push(
                                        crate::diagnostics::error_value_cannot_be_used_here(
                                            v,
                                            bin.left.span,
                                        ),
                                    );
                                }
                                _ if self.arithmetic_operand_invalid(&left_ty) => {
                                    self.diagnostics.push(error_arithmetic_lhs(bin.left.span));
                                }
                                _ => {}
                            }
                            match nullish(&right_ty) {
                                Some(v) if self.strict_null_checks => {
                                    self.diagnostics.push(
                                        crate::diagnostics::error_value_cannot_be_used_here(
                                            v,
                                            bin.right.span,
                                        ),
                                    );
                                }
                                _ if self.arithmetic_operand_invalid(&right_ty) => {
                                    self.diagnostics.push(error_arithmetic_rhs(bin.right.span));
                                }
                                _ => {}
                            }
                        }
                        // bigint op bigint stays bigint; `any` pairs with a
                        // bigint operand as bigint too (tsc: both any → number,
                        // one any + one bigint-like → bigint).
                        let left_kind = self.widen_type(&left_ty);
                        let right_kind = self.widen_type(&right_ty);
                        let left_big = matches!(left_kind, Type::BigInt);
                        let right_big = matches!(right_kind, Type::BigInt);
                        let left_any = matches!(left_kind, Type::Any);
                        let right_any = matches!(right_kind, Type::Any);
                        if (left_big && (right_big || right_any)) || (right_big && left_any) {
                            Type::BigInt
                        } else {
                            Type::Number
                        }
                    }
                    BinaryOp::Eq | BinaryOp::Ne | BinaryOp::StrictEq | BinaryOp::StrictNe => {
                        // TS2367: equality between types with NO overlap
                        // (neither side assignable to the other) is flagged as
                        // an unintentional comparison — the classic shape is a
                        // literal-typed const against a different literal
                        // (`const x = 0; if (x == 1)`). Conservatively gated
                        // to primitive/literal shapes: object/reference
                        // comparability is laxer in tsc, and nullish operands
                        // are excluded because our inference can drop
                        // `undefined` from unions where tsc keeps it.
                        //
                        // A bare-ident operand WITHOUT a const marker compares
                        // by its widened DECLARED type: tsc widens a mutable
                        // (`let`) binding's fresh-literal flow type, so
                        // `let x = 1; x == 2` is fine while
                        // `const x = 1; x == 2` has no overlap.
                        let cmp_ty = |chk: &Self, e: &Expr, narrowed: &Type| -> Type {
                            if let ExprKind::Ident(n) = &e.kind {
                                let const_marked = matches!(
                                    chk.nearer_binding_is(&Self::const_marker_name(n), n),
                                    Some(true)
                                );
                                if !const_marked {
                                    if let Some(decl) = chk.lookup_declared_var(n) {
                                        return decl;
                                    }
                                }
                            }
                            narrowed.clone()
                        };
                        let l_cmp = cmp_ty(self, &bin.left, &left_ty);
                        let r_cmp = cmp_ty(self, &bin.right, &right_ty);
                        let (no_overlap, l_shape, r_shape) =
                            self.comparison_no_overlap(&l_cmp, &r_cmp);
                        if self.suppress_comparison_no_overlap_depth == 0 && no_overlap {
                            // tsc getBaseTypesIfUnrelated: when even the
                            // literal-widened bases have no overlap, the
                            // message shows the bases (`'string' and
                            // 'number'`); otherwise the literals stay
                            // (`'1' and '0'`, `'E.a' and 'E.b'`).
                            let (mut l_disp, mut r_disp) = match (&l_shape, &r_shape) {
                                (Some((_, l)), Some((_, r))) => (l.clone(), r.clone()),
                                _ => (l_cmp.clone(), r_cmp.clone()),
                            };
                            if let (Some((l, _)), Some((r, _))) = (&l_shape, &r_shape) {
                                let lb = self.widen_literals_for_display(l);
                                let rb = self.widen_literals_for_display(r);
                                if !self.is_assignable_to(&lb, &rb)
                                    && !self.is_assignable_to(&rb, &lb)
                                {
                                    l_disp = self.widen_literals_for_display(&l_disp);
                                    r_disp = self.widen_literals_for_display(&r_disp);
                                }
                            }
                            self.diagnostics.push(error_comparison_no_overlap(
                                &l_disp.display_string(),
                                &r_disp.display_string(),
                                expr.span,
                            ));
                        }
                        Type::Boolean
                    }
                    BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
                        // Relational operands are non-null-checked in both
                        // strictness modes; a nullish literal replaces the
                        // comparability check.
                        let nullish_literal = self.report_nullish_literal_operand(&bin.left)
                            | self.report_nullish_literal_operand(&bin.right);
                        if !nullish_literal {
                            self.report_nullish_operand(&bin.left, &left_ty);
                            self.report_nullish_operand(&bin.right, &right_ty);
                        }
                        if !nullish_literal {
                            self.report_relational_operator_incompatibility(
                                match bin.op {
                                    BinaryOp::Lt => "<",
                                    BinaryOp::Le => "<=",
                                    BinaryOp::Gt => ">",
                                    BinaryOp::Ge => ">=",
                                    _ => unreachable!(),
                                },
                                &left_ty,
                                &right_ty,
                                expr.span,
                            );
                        }
                        Type::Boolean
                    }
                    BinaryOp::In => {
                        self.report_nullish_operand(&bin.left, &left_ty);
                        self.report_nullish_operand(&bin.right, &right_ty);
                        Type::Boolean
                    }
                    BinaryOp::InstanceOf => {
                        // The JS checker relies heavily on JSDoc recovery. Until
                        // recovered JSDoc-any annotations reliably override
                        // initializer inference, emitting these diagnostics in
                        // JS would turn an `any` operand into a false primitive.
                        if !self.current_file_is_js() {
                            if self.instanceof_lhs_invalid(&left_ty)
                                || self.instanceof_expr_definitely_primitive(&bin.left)
                            {
                                self.diagnostics
                                    .push(error_invalid_instanceof_left(bin.left.span));
                            }
                            if self.instanceof_rhs_invalid(&bin.right, &right_ty)
                                || self.instanceof_expr_definitely_primitive(&bin.right)
                            {
                                self.diagnostics
                                    .push(error_invalid_instanceof_right(bin.right.span));
                            }
                        }
                        Type::Boolean
                    }
                    BinaryOp::LogAnd => {
                        self.check_syntactic_truthiness(&bin.left);
                        // Return right operand type (simplified)
                        right_ty
                    }
                    BinaryOp::LogOr => {
                        self.check_syntactic_truthiness(&bin.left);
                        // `a || b` has type `(truthy part of a) | b` — the falsy
                        // members of `a` can never reach the result. Critically
                        // this removes `undefined`/`null`, so the ubiquitous
                        // `const e = map.get(k) || fallback` (and `x || "def"`)
                        // yields a non-nullable `e` instead of keeping
                        // `undefined` and mis-firing TS2532 on later use.
                        let truthy_left = match &left_ty {
                            Type::Union(members) => {
                                let filtered: Vec<Type> = members
                                    .iter()
                                    .filter(|m| !Self::is_always_falsy(m))
                                    .cloned()
                                    .collect();
                                if filtered.is_empty() {
                                    right_ty.clone()
                                } else {
                                    Type::flatten_union(filtered)
                                }
                            }
                            t if Self::is_always_falsy(t) => right_ty.clone(),
                            _ => left_ty,
                        };
                        Type::flatten_union(vec![truthy_left, right_ty])
                    }
                    BinaryOp::NullCoal => {
                        // x ?? y: strip null/undefined from left, then union with right.
                        // e.g., (string | null) ?? "default" → string. An alias
                        // (`Maybe<T>`) expands first.
                        let left_ty = self
                            .resolve_type_for_assignability(&left_ty)
                            .filter(|resolved| matches!(resolved, Type::Union(_)))
                            .unwrap_or(left_ty);
                        let stripped_left = match &left_ty {
                            Type::Union(members) => {
                                let filtered: Vec<Type> = members
                                    .iter()
                                    .filter(|m| !matches!(m, Type::Null | Type::Undefined))
                                    .cloned()
                                    .collect();
                                if filtered.is_empty() {
                                    right_ty.clone()
                                } else if filtered.len() == 1 {
                                    filtered.into_iter().next().unwrap()
                                } else {
                                    Type::Union(filtered.into())
                                }
                            }
                            Type::Null | Type::Undefined => right_ty.clone(),
                            _ => left_ty,
                        };
                        Type::flatten_union(vec![stripped_left, right_ty])
                    }
                };
                if logical_condition {
                    self.uncalled_condition_depth -= 1;
                }
                result
            }
            ExprKind::Unary(un) => {
                let operand_ty = self.check_expr(&un.argument);
                if matches!(un.op, UnaryOp::Neg | UnaryOp::Pos | UnaryOp::BitNot) {
                    self.report_nullish_operand(&un.argument, &operand_ty);
                }
                if matches!(un.op, UnaryOp::Pos)
                    && matches!(operand_ty, Type::BigInt | Type::BigIntLiteral(_))
                {
                    self.diagnostics
                        .push(error_bigint_unary_plus(un.argument.span));
                }
                match (&un.op, &un.argument.kind) {
                    // `+1` / `-1` on a numeric literal keep a literal type.
                    (UnaryOp::Pos, ExprKind::NumLit(value)) => {
                        Type::NumberLiteral(Self::normalize_numeric_name(value))
                    }
                    (UnaryOp::Neg, ExprKind::NumLit(value)) => {
                        let magnitude = Self::normalize_numeric_name(value);
                        Type::NumberLiteral(match magnitude.strip_prefix('-') {
                            Some(rest) => rest.to_string(),
                            None => format!("-{magnitude}"),
                        })
                    }
                    (UnaryOp::Neg, ExprKind::BigIntLit(value)) => {
                        let magnitude = Self::normalize_bigint_literal(value);
                        Type::BigIntLiteral(if magnitude == "0n" {
                            magnitude
                        } else {
                            format!("-{magnitude}")
                        })
                    }
                    (UnaryOp::Neg | UnaryOp::Pos | UnaryOp::BitNot, _) => Type::Number,
                    (UnaryOp::LogNot, _) => Type::Boolean,
                    (UnaryOp::Typeof, _) => Self::typeof_result_type(),
                    (UnaryOp::Void, _) => Type::Undefined,
                    (UnaryOp::Delete, _) => {
                        self.check_delete_operand(&un.argument);
                        Type::Boolean
                    }
                }
            }
            ExprKind::Update(up) => {
                if self.report_invalid_assignment_target(&up.argument) {
                    return Type::Number;
                }
                let readonly_write = self.report_readonly_property_write(&up.argument);
                let operand = self.check_expr(&up.argument);
                if self.update_operand_has_unknown(&operand) {
                    if let Some(path) = Self::simple_expression_path(&up.argument) {
                        self.diagnostics
                            .push(error_value_unknown(&path, up.argument.span));
                    }
                } else if !matches!(operand, Type::Error)
                    && !readonly_write
                    && !self.update_target_has_specific_assignment_error(&up.argument)
                    && !self.update_operand_is_arithmetic(&operand)
                {
                    self.diagnostics
                        .push(error_arithmetic_operand(up.argument.span));
                }
                if matches!(operand, Type::BigInt | Type::BigIntLiteral(_)) {
                    Type::BigInt
                } else {
                    Type::Number
                }
            }
            ExprKind::Call(call) => {
                for arg in call.type_args.as_deref().unwrap_or_default() {
                    self.check_type_node_generic_arity(arg);
                }
                // Dynamic import: import("module") → Promise<any>
                if matches!(&call.callee.kind, ExprKind::Ident(name) if name == "import") {
                    for arg in &call.args {
                        self.check_expr(arg);
                    }
                    Type::TypeReference("Promise".to_string(), vec![Type::Any].into())
                } else {
                    let saved_callee_pos = self.in_callee_position;
                    self.in_callee_position = true;
                    let callee_ty_raw = self.check_expr(&call.callee);
                    self.in_callee_position = saved_callee_pos;
                    let receiver_for_this: Option<Type> = match &call.callee.kind {
                        ExprKind::Member(mem) => Some(self.check_expr(&mem.object)),
                        _ => None,
                    };
                    let callee_ty = if let Some(ref recv) = receiver_for_this {
                        Self::substitute_this(&callee_ty_raw, recv)
                    } else {
                        callee_ty_raw
                    };

                    // TS2349/TS2348: calling a definitely-non-callable value
                    // (`foo()(...)` where foo returns string, `(a | b)()`,
                    // instances of call-signature-less classes, or the class
                    // VALUE itself — "did you mean new?"). Member callees are
                    // skipped (accessor calls are tsc's TS6234, not TS2349),
                    // and duplicate spans are suppressed (expressions can be
                    // checked more than once).
                    if !matches!(&call.callee.kind, ExprKind::Member(_) | ExprKind::Super) {
                        let class_value = match &call.callee.kind {
                            // Skip fn+class merges (`function Foo(); class Foo`):
                            // the call resolves through the function overloads
                            // and tsc reports the merge error at the decls.
                            ExprKind::Ident(n)
                                if self.class_info.contains_key(n.as_str())
                                    && !self.fn_param_info.contains_key(n.as_str()) =>
                            {
                                Some(n.clone())
                            }
                            _ => None,
                        };
                        let dup = |diags: &[Diagnostic], code: u32| {
                            diags.iter().any(|d| {
                                d.code == code
                                    && d.span.is_some_and(|sp| sp.start == call.callee.span.start)
                            })
                        };
                        if let Some(cls) = class_value {
                            if !dup(&self.diagnostics, 2348) {
                                // tsc marks the whole call expression.
                                self.diagnostics
                                    .push(error_class_not_callable(&cls, expr.span));
                            }
                        } else if self.callee_definitely_not_callable(&callee_ty)
                            && !dup(&self.diagnostics, 2349)
                        {
                            // tsc names the apparent type: `String`, not `string`.
                            let shown = match self.widen_type(&callee_ty) {
                                Type::String => "String".to_string(),
                                Type::Number => "Number".to_string(),
                                Type::Boolean => "Boolean".to_string(),
                                Type::BigInt => "BigInt".to_string(),
                                Type::Symbol => "Symbol".to_string(),
                                other => other.display_string(),
                            };
                            let mut diagnostic = error_not_callable(&shown, call.callee.span);
                            // A single-argument call whose parenthesis starts on a later
                            // line than the callee is usually a missing semicolon; tsc
                            // notes it on the callee.
                            if call.args.len() == 1 {
                                if let Some(source) = self.current_source.as_deref() {
                                    let end = call.callee.span.end as usize;
                                    if let Some(offset) =
                                        source.get(end..).and_then(|rest| rest.find('('))
                                    {
                                        let between = &source[end..end + offset];
                                        if between.contains('\n') && between.trim().is_empty() {
                                            diagnostic.related =
                                                Some(vec![tsc_rs_ast::RelatedDiagnostic {
                                                    code: 2734,
                                                    message: "Are you missing a semicolon?"
                                                        .to_string(),
                                                    file_name: self.current_file_name.clone(),
                                                    span: Some(call.callee.span),
                                                }]);
                                        }
                                    }
                                }
                            }
                            self.diagnostics.push(diagnostic);
                        }
                    }

                    // Determine parameter types for contextual typing of callback args
                    let param_types: Vec<(Type, bool)> = match &callee_ty {
                        Type::Function(ft) => {
                            if !ft.type_params.is_empty() {
                                // Infer generics from non-callback args first
                                let for_inference: Vec<Type> = call
                                    .args
                                    .iter()
                                    .map(|arg| match &arg.kind {
                                        ExprKind::Arrow(_) | ExprKind::FnExpr(_) => Type::Any,
                                        _ => self.infer_expr_type(arg),
                                    })
                                    .collect();
                                let type_param_map = if let Some(ref ta) = call.type_args {
                                    let mut map = HashMap::new();
                                    for (tp_name, tan) in ft.type_params.iter().zip(ta.iter()) {
                                        map.insert(tp_name.clone(), self.resolve_type_node(tan));
                                    }
                                    map
                                } else {
                                    // Widen literals before generic inference
                                    let widened: Vec<Type> = for_inference
                                        .iter()
                                        .map(|t| self.widen_argument_for_inference(t))
                                        .collect();
                                    self.infer_type_arguments_for_call(
                                        &widened,
                                        &ft.params,
                                        &ft.type_params,
                                    )
                                };
                                ft.params
                                    .iter()
                                    .map(|(name, t)| {
                                        (
                                            Self::optional_param_type(
                                                name,
                                                Self::substitute(t, &type_param_map),
                                            ),
                                            name.starts_with("..."),
                                        )
                                    })
                                    .collect()
                            } else {
                                ft.params
                                    .iter()
                                    .map(|(name, t)| {
                                        (
                                            Self::optional_param_type(name, t.clone()),
                                            name.starts_with("..."),
                                        )
                                    })
                                    .collect()
                            }
                        }
                        _ => Vec::new(),
                    };

                    // Check arguments with contextual types for callbacks.
                    // For super(...) args, arrows are exempt from TS17009
                    // (deferred execution) — flag the region.
                    let is_super_call = matches!(call.callee.kind, ExprKind::Super);
                    // Outside a constructor body a super call is TS2337.
                    if is_super_call
                        && (self.computed_name_depth == Some(self.fn_nesting_depth)
                            || self
                                .ctor_own_super_call
                                .is_some_and(|(depth, _)| depth == self.fn_nesting_depth))
                    {
                        self.check_super_in_non_derived_class(call.callee.span);
                    }
                    let base_class_name = is_super_call
                        .then(|| {
                            self.enclosing_class_names
                                .last()
                                .and_then(|class_name| self.class_info.get(class_name))
                                .and_then(|info| info.extends.clone())
                        })
                        .flatten();
                    let base_constructor_signatures = base_class_name
                        .as_deref()
                        .and_then(|base_name| self.class_constructor_signatures(base_name));
                    let super_contextual_params = base_class_name
                        .as_deref()
                        .and_then(|base_name| self.get_contextual_constructor_params(base_name));
                    let contextual_params = if is_super_call {
                        super_contextual_params.as_deref()
                    } else if param_types.is_empty() {
                        None
                    } else {
                        Some(param_types.as_slice())
                    };
                    let saved_isa = self.in_super_call_args;
                    if is_super_call {
                        self.in_super_call_args = true;
                    }
                    let arg_types =
                        self.check_arguments_contextual_once(&call.args, contextual_params);
                    self.in_super_call_args = saved_isa;

                    // TS17009: a `super(...)` call legalizes `this`. At the
                    // scope that OWNS the pending marker (statement-level
                    // super in the ctor body) the marker is REMOVED — the
                    // rest of the ctor is legal. In a branch scope, a local
                    // THIS_OK keeps the effect branch-scoped. Applied after
                    // arg checking — direct `this` inside super's own
                    // arguments still errors.
                    // Super calls use the base's public signatures instantiated
                    // with the class's declared heritage arguments.
                    if is_super_call {
                        if let Some(signatures) = base_constructor_signatures {
                            let base_args = self
                                .enclosing_class_names
                                .last()
                                .and_then(|name| self.class_info.get(name))
                                .map(|info| info.extends_type_args.clone())
                                .unwrap_or_default();
                            let signatures: Vec<_> = signatures
                                .iter()
                                .map(|signature| {
                                    self.instantiate_constructor_signature(
                                        signature, &base_args, &arg_types, None,
                                    )
                                })
                                .collect();
                            if !signatures.iter().any(|signature| {
                                self.constructor_signature_accepts(signature, &arg_types)
                            }) {
                                let old_name = std::mem::replace(
                                    &mut self.pending_new_callee_name,
                                    base_class_name.clone(),
                                );
                                // tsc getDiagnosticSpanForCallNode: a call's
                                // arity/overload errors squiggle the callee
                                // (`super`), not the whole call.
                                if signatures.len() > 1 {
                                    self.report_constructor_overload_mismatch(
                                        &signatures,
                                        &arg_types,
                                        Some(&call.args),
                                        call.callee.span,
                                    );
                                } else if let Some(signature) = signatures.first() {
                                    self.report_constructor_signature_mismatch(
                                        signature,
                                        &arg_types,
                                        Some(&call.args),
                                        call.callee.span,
                                    );
                                }
                                self.pending_new_callee_name = old_name;
                            }
                        }
                    }
                    if is_super_call {
                        if !self.remove_super_pending_current() {
                            self.declare_var(THIS_OK_MARKER, Type::Never);
                        }
                        self.super_call_count += 1;
                        if let Some((depth, seen)) = &mut self.ctor_own_super_call {
                            if *depth == self.fn_nesting_depth {
                                *seen = true;
                            }
                        }
                    }

                    // Extract callee name for overload lookup.
                    // Only consider bare Ident calls — member calls have their
                    // callee_ty already resolved through the receiver
                    // (`check_expr` on the Member node substitutes the
                    // owning interface's type args), so falling back to the
                    // bare property name would pick up an UNSUBSTITUTED
                    // signature from `self.overloads` (keyed by name only).
                    // Real-world: `proc.input(schema)` where `proc:
                    // PB<{tenantId:string}, undefined>` lost its `C` and
                    // returned `PB<C, NewI>` because the overload
                    // resolver re-used the original interface signature.
                    let callee_name = match &call.callee.kind {
                        ExprKind::Ident(name) => Some(name.to_string()),
                        _ => None,
                    };

                    // Check for overloads. The overloads map is keyed by NAME
                    // and built from file-level declarations — a block-scoped
                    // `function foo() {}` shadowing an outer `foo(a)` must NOT
                    // be checked against the outer signatures
                    // (blockScopedSameNameFunctionDeclaration*): when the
                    // nearest binding is NOT root-scoped, skip the map and let
                    // the plain Function-callee path use the scoped type.
                    let overload_sigs = callee_name
                        .as_ref()
                        .filter(|name| self.binding_scope_is_root(name).unwrap_or(true))
                        .and_then(|name| self.overloads.get(name.as_str()))
                        .cloned();

                    let raw_ret = if let Some(ref sigs) = overload_sigs {
                        if sigs.len() > 1 {
                            if let Some((index, ret_ty)) =
                                self.select_overload_resolution(sigs, &arg_types)
                            {
                                self.selected_overload_indices
                                    .insert(call.callee.span.start, index);
                                ret_ty
                            } else {
                                let diagnostic =
                                    self.no_overload_diagnostic(sigs, &arg_types, call, expr);
                                self.diagnostics.push(diagnostic);
                                Type::Error
                            }
                        } else if let Some(sig) = sigs.first() {
                            // A lone bodyless signature is a plain call in
                            // tsc — per-argument TS2345 elaboration, not a
                            // blanket "No overload matches" (TS2769).
                            let sig_ty = Type::Function(sig.clone());
                            self.check_call_against_fn_type(&sig_ty, &arg_types, call, expr)
                        } else {
                            self.check_call_against_fn_type(&callee_ty, &arg_types, call, expr)
                        }
                    } else {
                        self.check_call_against_fn_type(&callee_ty, &arg_types, call, expr)
                    };
                    let with_this = if let Some(ref recv) = receiver_for_this {
                        Self::substitute_this(&raw_ret, recv)
                    } else {
                        raw_ret
                    };
                    // Type-predicate refinement for `<arr>.filter(pred)`. If
                    // the user passed an arrow / function whose return type
                    // is `x is S`, refine the call result from `Array<T>` to
                    // `Array<S>`. Our `Array.filter` builtin signature is a
                    // single non-overloaded `(cb: …) => T[]`, so the standard
                    // resolver returns the unrefined `T[]`. Real TS has a
                    // second overload `<S extends T>(cb: (v:T)=>v is S):S[]`
                    // that selects when the callback's `type_predicate` is
                    // set — we hand-roll the equivalent here. This unlocks
                    // `arr.filter((x): x is File => x instanceof File)`
                    // returning `File[]` (real-world: ~6 errors in apps/app
                    // and rises through downstream `unknown[]` propagation).
                    // `<arr>.find(pred)` narrows to `S | undefined` and
                    // `<arr>.filter(pred)` to `S[]` when `pred` is a type guard
                    // (`x is S`). Our builtin Array signatures are single,
                    // non-overloaded, so the resolver returns the unrefined
                    // `T | undefined` / `T[]`; hand-roll the predicate overloads
                    // here. Works for Array and ReadonlyArray receivers alike.
                    let predicate_target = arg_types
                        .first()
                        .and_then(|a| match a {
                            Type::Function(ft) => ft.type_predicate.as_ref(),
                            _ => None,
                        })
                        .filter(|tp| !tp.is_asserts)
                        .map(|tp| Type::clone(&tp.target_type));
                    // The refined method is on a member callee (`arr.find`), so
                    // key on the member property name, not `callee_name` (which
                    // only captures bare-identifier callees).
                    let method_name = match &call.callee.kind {
                        ExprKind::Member(m) => Some(m.property.to_string()),
                        _ => None,
                    };
                    let with_predicate = match (method_name.as_deref(), predicate_target) {
                        (Some("filter"), Some(target)) => Type::Array(Arc::new(target)),
                        (Some("find"), Some(target)) => {
                            Type::flatten_union(vec![target, Type::Undefined])
                        }
                        // `arr.map(cb)` / `flatMap`: the element type is the
                        // callback's RETURN type (builtins types the generic
                        // U as any; take it from the checked callback arg).
                        (Some("map"), None) | (Some("flatMap"), None) => {
                            let cb_ret = arg_types.first().and_then(|a| match a {
                                Type::Function(ft) => {
                                    let r = Type::clone(&ft.return_type);
                                    if matches!(r, Type::Any | Type::Error | Type::Unknown) {
                                        None
                                    } else {
                                        Some(r)
                                    }
                                }
                                _ => None,
                            });
                            match (cb_ret, &with_this) {
                                (Some(ret), Type::Array(_)) => {
                                    let elem = if method_name.as_deref() == Some("flatMap") {
                                        match ret {
                                            Type::Array(inner) => Type::clone(&inner),
                                            other => other,
                                        }
                                    } else {
                                        ret
                                    };
                                    Type::Array(Arc::new(self.widen_type(&elem)))
                                }
                                _ => with_this,
                            }
                        }
                        _ => with_this,
                    };
                    // Preserve canonical object-merge aliases returned by
                    // calls. Expanding one here discards both its diagnostic
                    // identity and the DAG sharing in chains such as
                    // `merge<merge<...>, P>`. Other aliases retain the normal
                    // simplification path.
                    if self.lazy_object_merge_parts(&with_predicate).is_some() {
                        with_predicate
                    } else {
                        self.simplify_type(&with_predicate)
                    }
                } // end else (non-import call)
            }
            ExprKind::New(new_expr) => {
                let callee_ty = self.check_expr(&new_expr.callee);
                let effective_callee_ty = self.simplify_type(&callee_ty);
                let class_name = match &new_expr.callee.kind {
                    // Follow the resolved value binding, not just the source
                    // spelling. This matters for lexical `typeof` aliases and
                    // for a named class expression's body-local self binding.
                    ExprKind::Ident(name) => match &callee_ty {
                        Type::TypeReference(value_name, args) if args.is_empty() => value_name
                            .strip_prefix("typeof ")
                            .map(str::to_owned)
                            .or_else(|| Some(name.to_string())),
                        _ => Some(name.to_string()),
                    },
                    ExprKind::Member(member) => {
                        // `new A.B.C()`: a dotted path rooted at a namespace
                        // binding names the class under any suffix of the
                        // qualified path (`A.B.C`, `B.C`, `C`).
                        let mut segments: Vec<&str> = vec![member.property.as_str()];
                        let mut cursor = &member.object;
                        let root = loop {
                            match &cursor.kind {
                                ExprKind::Member(inner) => {
                                    segments.push(inner.property.as_str());
                                    cursor = &inner.object;
                                }
                                ExprKind::Ident(root) => break Some(root.as_str()),
                                _ => break None,
                            }
                        };
                        let namespace_rooted = root.is_some_and(|root| {
                            self.lookup_var(root).is_some_and(|binding| {
                                matches!(binding, Type::Namespace(_) | Type::Module(_))
                            })
                        });
                        if !namespace_rooted {
                            None
                        } else {
                            match &callee_ty {
                                Type::TypeReference(value_name, args)
                                    if args.is_empty()
                                        && self.class_info.contains_key(value_name.as_str()) =>
                                {
                                    Some(value_name.clone())
                                }
                                _ => {
                                    // Namespace-owned classes are keyed by
                                    // their full path, root included
                                    // (`A.Point`).
                                    segments.extend(root);
                                    segments.reverse();
                                    (0..segments.len()).find_map(|skip| {
                                        let candidate = segments[skip..].join(".");
                                        self.class_info
                                            .contains_key(candidate.as_str())
                                            .then_some(candidate)
                                    })
                                }
                            }
                        }
                    }
                    _ => None,
                };
                let mut constructability = self.constructability(&effective_callee_ty);
                if matches!(constructability, Constructability::None) {
                    if let Some(signatures) = class_name
                        .as_deref()
                        .and_then(|name| self.class_constructor_signatures(name))
                    {
                        constructability = Constructability::Overloads(signatures);
                    } else if self.is_standard_intl_constructor(&new_expr.callee) {
                        // The lib declaration loader can retain the callable
                        // side of declaration-merged Intl constructor
                        // interfaces while losing their parallel `new`
                        // signature. These are all standard constructable
                        // Intl values; keep argument/result checking
                        // conservative until merged signatures survive
                        // lowering intact.
                        constructability = Constructability::Dynamic;
                    }
                }
                for arg in new_expr.type_args.as_deref().unwrap_or_default() {
                    self.check_type_node_generic_arity(arg);
                }
                let explicit_type_args: Vec<Type> = new_expr
                    .type_args
                    .as_ref()
                    .map(|args| args.iter().map(|arg| self.resolve_type_node(arg)).collect())
                    .unwrap_or_default();
                let type_arg_spans = new_expr
                    .type_args
                    .as_ref()
                    .map(|args| args.iter().map(|arg| arg.span).collect::<Vec<_>>())
                    .unwrap_or_default();
                if !self.validate_constructor_type_args(
                    &constructability,
                    &explicit_type_args,
                    &type_arg_spans,
                    expr.span,
                    &callee_ty.display_string(),
                ) {
                    return Type::Error;
                }

                // Resolve constructor metadata before checking arguments so a
                // locally known, simple constructor can contextually type
                // callbacks. The conservative helper returns `None` for
                // generic, overloaded, bodyless, external, or cyclic bases.
                let named_class_callee = class_name
                    .as_deref()
                    .is_some_and(|name| self.class_info.contains_key(name));
                let preliminary_argument_types: Vec<Type> = new_expr
                    .args
                    .as_deref()
                    .map(|arguments| {
                        arguments
                            .iter()
                            .map(|argument| self.infer_expr_type(argument))
                            .collect()
                    })
                    .unwrap_or_default();
                let signature_context = (|| {
                    if named_class_callee
                        && class_name
                            .as_deref()
                            .is_some_and(|name| !self.constructor_context_is_acyclic(name))
                    {
                        return None;
                    }
                    let signatures = match &constructability {
                        Constructability::All(groups)
                            if groups.len() == 1 && groups[0].len() == 1 =>
                        {
                            &groups[0]
                        }
                        Constructability::Overloads(signatures) => signatures,
                        Constructability::All(_) => return None,
                        Constructability::Dynamic | Constructability::None => return None,
                    };
                    let first = signatures.first()?;
                    if signatures.len() != 1
                        || named_class_callee
                            && !first.type_params.is_empty()
                            && explicit_type_args.is_empty()
                    {
                        return None;
                    }
                    let signature = self.instantiate_constructor_signature(
                        first,
                        &explicit_type_args,
                        &preliminary_argument_types,
                        self.contextual_call_returns.get(&expr.span.start),
                    );
                    (signatures.len() == 1).then(|| {
                        signature
                            .params
                            .iter()
                            .map(|(name, ty)| {
                                (
                                    Self::optional_param_type(name, ty.clone()),
                                    name.starts_with("..."),
                                )
                            })
                            .collect()
                    })
                })();
                let contextual_constructor_params = class_name
                    .as_deref()
                    .and_then(|name| self.get_contextual_constructor_params(name))
                    .or(signature_context);

                // Collect each argument type exactly once and reuse it for
                // generic inference, arity, and assignability below.
                let arg_types: Vec<Type> = new_expr
                    .args
                    .as_deref()
                    .map(|args| {
                        self.check_arguments_contextual_once(
                            args,
                            contextual_constructor_params.as_deref(),
                        )
                    })
                    .unwrap_or_default();
                if matches!(
                    &constructability,
                    Constructability::All(groups)
                        if groups.iter().flatten().any(|signature| signature.is_abstract)
                ) || matches!(
                    &constructability,
                    Constructability::Overloads(signatures)
                        if signatures.iter().any(|signature| signature.is_abstract)
                ) {
                    self.diagnostics.push(error_cannot_create_abstract(
                        &callee_ty.display_string(),
                        expr.span,
                    ));
                    return Type::Error;
                }
                if matches!(constructability, Constructability::None)
                    && match &effective_callee_ty {
                        Type::Unknown => true,
                        Type::Union(members) => {
                            members.iter().any(|member| matches!(member, Type::Unknown))
                        }
                        _ => false,
                    }
                {
                    let name = match &new_expr.callee.kind {
                        ExprKind::Ident(name) => name.as_str(),
                        _ => "value",
                    };
                    self.diagnostics
                        .push(error_value_unknown(name, new_expr.callee.span));
                    return Type::Error;
                }

                // Boxed primitives: `new Boolean()` is the WRAPPER type, not
                // the primitive (and not `any` — the lib constructor
                // signature would otherwise be lost). tsc treats these as
                // distinct object types that primitives can't be assigned to.
                if let Some(ref name) = class_name {
                    if matches!(name.as_str(), "Boolean" | "Number" | "String")
                        && !self.class_info.contains_key(name.as_str())
                    {
                        return Type::TypeReference(name.to_string(), Default::default());
                    }
                }

                if let Some(ref name) = class_name {
                    if self.class_info.contains_key(name.as_str()) {
                        // Reuse the public signatures resolved before argument checking.
                        // An unmodeled base (for example an external DOM class)
                        // retains the class result without inventing a zero-arity constructor.
                        let signatures = match constructability {
                            Constructability::Overloads(signatures) => Some(signatures),
                            _ => self.class_constructor_signatures(name),
                        };
                        let Some(signatures) = signatures else {
                            return Type::TypeReference(
                                name.to_string(),
                                explicit_type_args.clone().into(),
                            );
                        };
                        let instantiated_signatures: Vec<_> = signatures
                            .iter()
                            .map(|signature| {
                                self.instantiate_constructor_signature(
                                    signature,
                                    &explicit_type_args,
                                    &arg_types,
                                    self.contextual_call_returns.get(&expr.span.start),
                                )
                            })
                            .collect();
                        let selected = instantiated_signatures.iter().position(|signature| {
                            self.constructor_signature_accepts(signature, &arg_types)
                        });
                        let Some(instantiated) = instantiated_signatures.get(selected.unwrap_or(0))
                        else {
                            return Type::TypeReference(
                                name.to_string(),
                                explicit_type_args.clone().into(),
                            );
                        };
                        if selected.is_none() {
                            let old_name = std::mem::replace(
                                &mut self.pending_new_callee_name,
                                class_name.clone(),
                            );
                            if instantiated_signatures.len() > 1 {
                                self.report_constructor_overload_mismatch(
                                    &instantiated_signatures,
                                    &arg_types,
                                    new_expr.args.as_deref(),
                                    expr.span,
                                );
                            } else {
                                self.report_constructor_signature_mismatch(
                                    instantiated,
                                    &arg_types,
                                    new_expr.args.as_deref(),
                                    expr.span,
                                );
                            }
                            self.pending_new_callee_name = old_name;
                        }
                        let type_ref_args = match instantiated.return_type.as_ref() {
                            Type::TypeReference(_, args) => args.to_vec(),
                            _ => Vec::new(),
                        };
                        // Always return a TypeReference for class instantiation.
                        // This preserves the class name in hover display and
                        // avoids eagerly expanding recursive instance shapes.
                        Type::TypeReference(name.to_string(), type_ref_args.into())
                    } else {
                        // Not a known class — check well-known constructors
                        match name.as_str() {
                            "Date"
                            | "RegExp"
                            | "Error"
                            | "TypeError"
                            | "RangeError"
                            | "ReferenceError"
                            | "SyntaxError"
                            | "URIError"
                            | "EvalError"
                            | "URL"
                            | "URLSearchParams"
                            | "Headers"
                            | "Request"
                            | "Response"
                            | "FormData"
                            | "Blob"
                            | "File"
                            | "Event"
                            | "CustomEvent"
                            | "AbortController"
                            | "TextEncoder"
                            | "TextDecoder"
                            | "ReadableStream"
                            | "WritableStream"
                            | "TransformStream"
                            | "WeakRef"
                            | "FinalizationRegistry"
                            | "Int8Array"
                            | "Uint8Array"
                            | "Uint8ClampedArray"
                            | "Int16Array"
                            | "Uint16Array"
                            | "Int32Array"
                            | "Uint32Array"
                            | "Float16Array"
                            | "Float32Array"
                            | "Float64Array"
                            | "BigInt64Array"
                            | "BigUint64Array"
                            | "DataView"
                            | "ArrayBuffer"
                            | "SharedArrayBuffer" => {
                                Type::TypeReference(name.to_string(), Arc::from([] as [Type; 0]))
                            }
                            "Array"
                                if !self.file_shadows_global_array
                                    && self.nearest_binding_scope("Array") == Some(0) =>
                            {
                                // Keep the checked-expression representation
                                // identical to type-node and lightweight
                                // inference paths. In particular, `fill():
                                // this` must not turn a checked `new Array<T>`
                                // into a structurally expanded
                                // `TypeReference<Array<T>>` that then fails
                                // assignment to the equivalent `T[]`.
                                let element = if let Some(explicit) = explicit_type_args.first() {
                                    explicit.clone()
                                } else {
                                    match arg_types.as_slice() {
                                        [] => Type::Any,
                                        [single]
                                            if matches!(
                                                single,
                                                Type::Number | Type::NumberLiteral(_)
                                            ) =>
                                        {
                                            Type::Any
                                        }
                                        [single] => Self::widen_nested_literals(single),
                                        many => Type::flatten_union(
                                            many.iter().map(Self::widen_nested_literals).collect(),
                                        ),
                                    }
                                };
                                Type::Array(Arc::new(element))
                            }
                            "Map" | "Set" | "WeakMap" | "WeakSet" | "Promise" => {
                                // Preserve explicit type arguments so that
                                // `new Promise<void>(...)` stays
                                // `Promise<void>` instead of collapsing to
                                // `Promise<>`. Required for the async-return
                                // unwrap path (`return new Promise<void>(…)`
                                // → unwrap to `void` → matches declared
                                // `Promise<void>` return type) and any
                                // downstream member-access / assignment
                                // that depends on the element type.
                                //
                                // When no explicit args are written, infer
                                // from the first constructor argument so
                                // `new Set(stringArr)` is `Set<string>` (not
                                // `Set<>`). Without this, the `Spread` arm
                                // can't unwrap to `string`, and
                                // `[...new Set(strings)]` infers as `Set[]`.
                                let args: Arc<[Type]> = if !explicit_type_args.is_empty() {
                                    explicit_type_args.clone().into()
                                } else {
                                    let first_argument = new_expr
                                        .args
                                        .as_deref()
                                        .and_then(|arguments| arguments.first());
                                    let infer_fresh_map_position = |position: usize| {
                                        let Some(argument) = first_argument else {
                                            return None;
                                        };
                                        let ExprKind::ArrayLit(entries) = &argument.kind else {
                                            return None;
                                        };
                                        let mut position_types = Vec::new();
                                        for entry in entries.iter().flatten() {
                                            let mut entry = entry.as_ref();
                                            while let ExprKind::Paren(inner)
                                            | ExprKind::NonNull(inner) = &entry.kind
                                            {
                                                entry = inner;
                                            }
                                            let ExprKind::ArrayLit(items) = &entry.kind else {
                                                return None;
                                            };
                                            let Some(item) =
                                                items.get(position).and_then(|item| item.as_ref())
                                            else {
                                                return None;
                                            };
                                            let inferred = self.infer_expr_type(item);
                                            let inferred = if Self::is_const_assertion_expr(item)
                                                || !Self::is_fresh_literal_expr(item)
                                            {
                                                inferred
                                            } else {
                                                Self::widen_nested_literals(&inferred)
                                            };
                                            if !position_types
                                                .iter()
                                                .any(|existing: &Type| existing == &inferred)
                                            {
                                                position_types.push(inferred);
                                            }
                                        }
                                        match position_types.len() {
                                            0 => None,
                                            1 => position_types.into_iter().next(),
                                            _ => self.best_common_type(&position_types).or_else(
                                                || Some(Type::flatten_union(position_types)),
                                            ),
                                        }
                                    };
                                    match (name.as_str(), preliminary_argument_types.as_slice()) {
                                        ("Set" | "WeakSet", [first, ..]) => match first {
                                            Type::Array(elem) => Arc::from([Type::clone(elem)]),
                                            Type::Tuple(elems) if !elems.is_empty() => {
                                                Arc::from([Type::flatten_union(
                                                    elems.iter().cloned().collect(),
                                                )])
                                            }
                                            Type::TypeReference(n, inner)
                                                if matches!(
                                                    n.as_str(),
                                                    "Set"
                                                        | "ReadonlySet"
                                                        | "Iterable"
                                                        | "IterableIterator"
                                                        | "Generator"
                                                        | "ReadonlyArray"
                                                        | "Array",
                                                ) && !inner.is_empty() =>
                                            {
                                                Arc::from([inner[0].clone()])
                                            }
                                            _ => Arc::from([] as [Type; 0]),
                                        },
                                        ("Map" | "WeakMap", _) => {
                                            // Map(iterable of [K,V] tuples)
                                            if let (Some(key), Some(value)) = (
                                                infer_fresh_map_position(0),
                                                infer_fresh_map_position(1),
                                            ) {
                                                Arc::from([key, value])
                                            } else {
                                                match arg_types.first() {
                                                    Some(Type::Array(elem)) => {
                                                        match elem.as_ref() {
                                                            Type::Tuple(t) if t.len() >= 2 => {
                                                                Arc::from([
                                                                    t[0].clone(),
                                                                    t[1].clone(),
                                                                ])
                                                            }
                                                            _ => Arc::from([] as [Type; 0]),
                                                        }
                                                    }
                                                    _ => Arc::from([] as [Type; 0]),
                                                }
                                            }
                                        }
                                        _ => Arc::from([] as [Type; 0]),
                                    }
                                };
                                // No explicit type args AND nothing inferable
                                // from the constructor argument: fall back to
                                // the lib's declared zero-arg overload
                                // (`new (): Map<any, any>`, `new <T = any>():
                                // Set<T>`), NOT a bare `Map`/`Set` with no
                                // arguments. A no-arg reference leaves the
                                // declaration's own type PARAMETER un-
                                // substituted, so `new Map().get(k)` typed as
                                // `V | undefined` and every use was flagged
                                // possibly-undefined — real tsc gives `any`.
                                let args: Arc<[Type]> = if args.is_empty() {
                                    match name.as_str() {
                                        "Map" | "WeakMap" => Arc::from([Type::Any, Type::Any]),
                                        "Set" | "WeakSet" | "Promise" => Arc::from([Type::Any]),
                                        _ => args,
                                    }
                                } else {
                                    args
                                };
                                Type::TypeReference(name.to_string(), args)
                            }
                            _ => match &effective_callee_ty {
                                Type::Any | Type::Error => Type::Any,
                                // Functions are constructable in JS (new function())
                                Type::Function(_) => Type::Any,
                                _ if !matches!(&constructability, Constructability::None) => {
                                    self.pending_new_callee_name = class_name.clone();
                                    let constructed = self.check_constructability(
                                        constructability.clone(),
                                        &explicit_type_args,
                                        &arg_types,
                                        new_expr.args.as_deref(),
                                        new_expr.callee.span,
                                        self.contextual_call_returns
                                            .get(&expr.span.start)
                                            .cloned()
                                            .as_ref(),
                                    );
                                    self.pending_new_callee_name = None;
                                    constructed
                                }
                                .unwrap_or(Type::Any),
                                Type::TypeReference(ref_name, _) => Type::TypeReference(
                                    ref_name.clone(),
                                    Arc::from([] as [Type; 0]),
                                ),
                                // ObjectType: if has construct signatures, use them;
                                // otherwise be lenient (may have incomplete type info)
                                Type::ObjectType(info) => {
                                    if !info.construct_signatures.is_empty() {
                                        // Already handled above, but fallback to any
                                        Type::Any
                                    } else {
                                        self.diagnostics.push(error_not_constructable(
                                            &effective_callee_ty.display_string(),
                                            new_expr.callee.span,
                                        ));
                                        Type::Error
                                    }
                                }
                                _ => {
                                    self.diagnostics.push(error_not_constructable(
                                        &effective_callee_ty.display_string(),
                                        new_expr.callee.span,
                                    ));
                                    Type::Error
                                }
                            },
                        }
                    }
                } else {
                    match constructability {
                        Constructability::None => {
                            self.diagnostics.push(error_not_constructable(
                                &effective_callee_ty.display_string(),
                                new_expr.callee.span,
                            ));
                            Type::Error
                        }
                        other => {
                            self.pending_new_callee_name = class_name.clone();
                            let constructed = self.check_constructability(
                                other,
                                &explicit_type_args,
                                &arg_types,
                                new_expr.args.as_deref(),
                                new_expr.callee.span,
                                self.contextual_call_returns
                                    .get(&expr.span.start)
                                    .cloned()
                                    .as_ref(),
                            );
                            self.pending_new_callee_name = None;
                            constructed
                        }
                        .unwrap_or(Type::Any),
                    }
                }
            }
            ExprKind::Member(mem) => {
                // An assignment target (`obj.prop = value`) is checked against
                // the property's DECLARED type, not a narrowed view of the
                // path — read-and-clear the flag so ONLY this outermost target
                // skips the lookup (nested reads below still narrow).
                let skip_path_narrowing = self.assign_lhs_declared;
                self.assign_lhs_declared = false;
                let property_span = Span::new(
                    expr.span.end.saturating_sub(mem.property.len() as u32),
                    expr.span.end,
                );
                let property_name = self.cooked_identifier_name(&mem.property, property_span);
                // `globalThis.x`: only script-level `var`/`function`/enum/
                // namespace declarations and lib values are its properties.
                let global_this_receiver = match &mem.object.kind {
                    ExprKind::Ident(object_name) => {
                        object_name == "globalThis" && self.lookup_var("globalThis").is_none()
                    }
                    // (A script's top-level `this` is `typeof globalThis` too,
                    // but object-literal members are checked on a path that
                    // does not track function nesting; left out for now.)
                    _ => false,
                };
                if global_this_receiver && !self.current_file_is_js() {
                    if let Some(ty) = self.global_this_member_type(&property_name, property_span) {
                        return ty;
                    }
                }
                // Property-path narrowing: `if (typeof field.options === "string")`
                // stores a path-keyed entry `field.options → string`. Honor it
                // for member access at any depth (`a.b`, `a.b.n`, `a?.b?.c`) —
                // without this the same access inside the guard returns the
                // un-narrowed union and trips overload selection / arg-type
                // checks (or a spurious "possibly undefined").
                if !skip_path_narrowing {
                    if let Some(path_key) = Self::member_path(expr) {
                        if let Some(ty) = self.lookup_narrowed(&path_key) {
                            return ty;
                        }
                    }
                }
                // When the object is ITSELF a narrowed member path
                // (`a.b.n` in `a.b.n.foo()`), use the guarded, non-nullable
                // type instead of re-deriving it — this yields the right
                // receiver and suppresses a spurious possibly-undefined on the
                // proven-defined prefix. Falls back to normal checking.
                let mut obj_ty = match &mem.object.kind {
                    ExprKind::Member(_) => Self::member_path(&mem.object)
                        .and_then(|p| self.lookup_narrowed(&p))
                        .unwrap_or_else(|| self.check_expr(&mem.object)),
                    _ => self.check_expr(&mem.object),
                };
                // Member access on a bare constrained type parameter sees
                // the constraint's apparent type (`t.x` with `T extends I`).
                if let Some(apparent) = self.typeparam_constraint_apparent(&obj_ty) {
                    obj_ty = apparent;
                }
                if !mem.optional {
                    self.check_nullable_access(&obj_ty, &mem.object);
                }
                self.check_private_member_access(&obj_ty, &mem.property, property_span);
                // TS2729: `this.x` directly in an instance initializer where
                // `x` is an own instance property not yet initialized.
                if matches!(mem.object.kind, ExprKind::This)
                    && self.class_init_ctx == Some(self.fn_nesting_depth)
                {
                    let declared_at = self
                        .instance_initializer_pending_props
                        .as_ref()
                        .and_then(|pending| pending.get(mem.property.as_str()).copied());
                    if let Some(declared_at) = declared_at {
                        let mut diagnostic = crate::diagnostics::error_property_used_before_init(
                            &mem.property,
                            property_span,
                        );
                        diagnostic.related = Some(vec![tsc_rs_ast::RelatedDiagnostic {
                            code: 2728,
                            message: format!("'{}' is declared here.", mem.property),
                            file_name: self.current_file_name.clone(),
                            span: Some(declared_at),
                        }]);
                        self.diagnostics.push(diagnostic);
                    }
                }
                if matches!(mem.object.kind, ExprKind::Super) {
                    self.check_super_property_container(mem.object.span);
                }
                // TS17011: `super.x` before `super()` in a derived constructor.
                if matches!(mem.object.kind, ExprKind::Super)
                    && self.super_pending_depth == Some(self.fn_nesting_depth)
                    && matches!(
                        self.nearer_binding_is(SUPER_PENDING_MARKER, THIS_OK_MARKER),
                        Some(true)
                    )
                {
                    self.diagnostics.push(Diagnostic {
                        code: 17011,
                        message: "'super' must be called before accessing a property of 'super' in the constructor of a derived class.".to_string(),
                        category: DiagnosticCategory::Error,
                        file_name: None,
                        span: Some(mem.object.span),
                        related: None,
                    });
                }
                // TS2855: class FIELDS live on the instance, not the
                // prototype — `super.field` can never reach them.
                if matches!(mem.object.kind, ExprKind::Super) {
                    if let Some(class_name) = self.enclosing_class_names.last() {
                        let base = self
                            .class_info
                            .get(class_name)
                            .and_then(|info| info.extends.clone());
                        if let Some(base_name) = base {
                            if let Some(binfo) = self.class_info.get(&base_name) {
                                let is_field = binfo
                                    .instance_properties
                                    .iter()
                                    .any(|(n, _)| *n == mem.property)
                                    && !binfo
                                        .instance_methods
                                        .iter()
                                        .any(|(n, _)| *n == mem.property)
                                    && !binfo.accessor_props.contains(mem.property.as_str());
                                if is_field {
                                    self.diagnostics.push(Diagnostic {
                                        code: 2855,
                                        message: format!(
                                            "Class field '{}' defined by the parent class is not accessible in the child class via super.",
                                            mem.property
                                        ),
                                                                                category: DiagnosticCategory::Error,
                                        file_name: None,
                                        span: Some(property_span),
                                        related: None,
                                    });
                                }
                            }
                        }
                    }
                }
                // For optional chaining (obj?.prop), strip null/undefined from
                // the object type before member resolution, then add | undefined
                // to the result.
                let (effective_obj, is_optional_chain) = if mem.optional {
                    let stripped = match &obj_ty {
                        Type::Union(members) => {
                            let filtered: Vec<Type> = members
                                .iter()
                                .filter(|m| !matches!(m, Type::Null | Type::Undefined))
                                .cloned()
                                .collect();
                            if filtered.len() == 1 {
                                filtered.into_iter().next().unwrap()
                            } else if filtered.is_empty() {
                                Type::Never
                            } else {
                                Type::Union(filtered.into())
                            }
                        }
                        Type::Null | Type::Undefined => Type::Never,
                        other => other.clone(),
                    };
                    (stripped, true)
                } else {
                    (obj_ty.clone(), false)
                };
                // Type aliases keep their display identity in diagnostics,
                // but member availability must be checked against the
                // aliased structure. In particular, `type U = A | B` has the
                // same all-union-members property rule as a written `A | B`.
                let member_obj = match &effective_obj {
                    Type::TypeReference(name, _)
                        if self.complete_union_flow_depth > 0
                            && self.type_aliases.contains_key(name.as_str()) =>
                    {
                        self.resolve_type_for_assignability(&effective_obj)
                            .unwrap_or_else(|| effective_obj.clone())
                    }
                    _ => effective_obj.clone(),
                };
                let reported_future_lib_member = self.report_future_lib_member(
                    &mem.object,
                    &member_obj,
                    &mem.property,
                    property_span,
                );
                let lazy_alias_member = match &member_obj {
                    Type::TypeReference(_, _) => self
                        .lazy_object_merge_property(&member_obj, &mem.property)
                        .map(|member| match member {
                            LazyAliasProperty::Present(value) => value,
                            LazyAliasProperty::Missing => {
                                self.diagnostics.push(error_property_not_exist(
                                    &mem.property,
                                    &obj_ty.display_string_single_line(),
                                    property_span,
                                ));
                                Type::Error
                            }
                            LazyAliasProperty::Unknown => Type::Any,
                        }),
                    _ => None,
                };
                let member_ty = lazy_alias_member.unwrap_or_else(|| match &member_obj {
                    Type::ObjectType(info) => {
                        if let Some((_, prop_ty)) =
                            info.properties.iter().find(|(n, _)| n == &mem.property)
                        {
                            // Unwrap Optional(T) → T | undefined when reading a property
                            match prop_ty.as_ref() {
                                Type::Optional(inner) => {
                                    Type::flatten_union(vec![Type::clone(&inner), Type::Undefined])
                                }
                                _ => Type::clone(&prop_ty),
                            }
                        } else if !info.call_signatures.is_empty()
                            && matches!(
                                mem.property.as_str(),
                                "length"
                                    | "name"
                                    | "bind"
                                    | "call"
                                    | "apply"
                                    | "caller"
                                    | "prototype"
                                    | "toString"
                            )
                        {
                            match mem.property.as_str() {
                                "length" => Type::Number,
                                "name" => Type::String,
                                "bind" | "call" | "apply" => Type::Function(FunctionType {
                                    type_param_constraints: Vec::new(),
                                    params: vec![("thisArg".to_string(), Type::Any)],
                                    return_type: Arc::new(Type::Any),
                                    type_params: Vec::new(),
                                    type_param_defaults: Vec::new(),
                                    type_predicate: None,
                                }),
                                _ => Type::Any,
                            }
                        } else if let Some(builtin_ty) = self
                            .builtins
                            .lookup_instance_property("Object", &mem.property)
                        {
                            // Property inherited from Object.prototype (e.g., hasOwnProperty, toString, valueOf)
                            builtin_ty
                        } else if let Some(value) = info.index_signature.as_ref().and_then(|(key, value)| {
                            matches!(key.as_ref(), Type::String).then(|| Type::clone(value))
                        }) {
                            // A string index signature admits every name.
                            value
                        } else if info.properties.is_empty()
                            && info.index_signature.is_none()
                        {
                            // Empty object type — the checker lacks type info,
                            // don't error on property access.
                            Type::Any
                        } else if info.properties.iter().any(|(_, ty)| {
                            matches!(ty.as_ref(), Type::Any)
                                || matches!(ty.as_ref(), Type::Union(members) if members.iter().any(|m| matches!(m, Type::Any)))
                        }) {
                            // Partially resolved type (has any-typed properties) —
                            // may have more properties from cross-file resolution.
                            Type::Any
                        } else if self.in_js_expando_assignment
                            && (self.current_file_is_js() || !info.call_signatures.is_empty())
                        {
                            // A simple property assignment in JavaScript, or
                            // on a callable object in TypeScript, is an expando
                            // declaration rather than a missing-property read.
                            Type::Any
                        } else if reported_future_lib_member {
                            Type::Any
                        } else {
                            self.diagnostics.push(error_property_not_exist(
                                &mem.property,
                                &obj_ty.display_string_single_line(),
                                property_span,
                            ));
                            Type::Error
                        }
                    }
                    Type::Any | Type::Error => Type::Any,
                    // Member access on a namespace import (`z.string`).
                    // The module's exports were resolved during the donor's
                    // `build_module_exports` pass and stored verbatim in the
                    // ModuleType. Each entry is `(name, Type)` — for a function
                    // export, the Type is already a `Type::Function`, so call
                    // resolution downstream works without any extra lookup.
                    Type::Module(info) => {
                        if let Some((_, ty)) = info.exports.iter().find(|(n, _)| n == &mem.property)
                        {
                            ty.clone()
                        } else {
                            Type::Any
                        }
                    }
                    Type::Namespace(info) => {
                        if let Some((_, ty)) = info.exports.iter().find(|(n, _)| n == &mem.property)
                        {
                            ty.clone()
                        } else {
                            // TS2339: the namespace has no such export.
                            // Display matches tsc: 'typeof M'.
                            let is_js_file = self
                                .current_file_name
                                .as_deref()
                                .map(|f| {
                                    f.ends_with(".js")
                                        || f.ends_with(".jsx")
                                        || f.ends_with(".mjs")
                                        || f.ends_with(".cjs")
                                })
                                .unwrap_or(false);
                            if !is_js_file
                                && !reported_future_lib_member
                                && !self.namespace_member_may_be_elsewhere(&info.name)
                            {
                                // tsc underlines the member name, not the
                                // whole access expression.
                                let property_span = Span::new(
                                    expr.span.end.saturating_sub(mem.property.len() as u32),
                                    expr.span.end,
                                );
                                self.diagnostics.push(error_property_not_exist(
                                    &mem.property,
                                    &format!("typeof {}", info.name),
                                    property_span,
                                ));
                                Type::Error
                            } else {
                                Type::Any
                            }
                        }
                    }
                    // Resolve member access on TypeReference (class instances)
                    Type::TypeReference(ref name, ref type_args) => {
                        // Constructor values (`typeof C`): members are the
                        // class STATICS.
                        if let Some(cls) = name.strip_prefix("typeof ") {
                            if let Some(ci) = self.class_info.get(cls) {
                                // `C.prototype` is the instance type with every
                                // type argument `any` (tsc getTypeOfPrototypeProperty).
                                // Classes of the same name in different
                                // namespaces share the bare key; keep `any`
                                // rather than guess which one this is.
                                let qualified_suffix = format!(".{cls}");
                                let ambiguous = self
                                    .class_info
                                    .keys()
                                    .any(|key| key.ends_with(qualified_suffix.as_str()));
                                if mem.property == "prototype"
                                    && !ambiguous
                                    && !self.current_file_is_js()
                                {
                                    let args = vec![Type::Any; ci.type_params.len()];
                                    return Type::TypeReference(cls.to_string(), Arc::from(args));
                                }
                                if let Some((_, t)) = ci
                                    .static_properties
                                    .iter()
                                    .find(|(n, _)| n == &mem.property)
                                {
                                    return t.clone();
                                }
                                if let Some((_, ft)) =
                                    ci.static_methods.iter().find(|(n, _)| n == &mem.property)
                                {
                                    return Type::Function(ft.clone());
                                }
                                // Inherited statics, `prototype`, and Function's
                                // own members (`call`, `bind`, `name`, …) exist;
                                // anything else is TS2339 on `typeof C`.
                                let inherited = self
                                    .direct_constructor_static_members(&obj_ty)
                                    .is_some_and(|members| {
                                        members.iter().any(|(n, _)| n == &mem.property)
                                    });
                                let function_member = matches!(
                                    mem.property.as_str(),
                                    "prototype"
                                        | "call"
                                        | "apply"
                                        | "bind"
                                        | "toString"
                                        | "length"
                                        | "name"
                                        | "arguments"
                                        | "caller"
                                        | "constructor"
                                        | "hasOwnProperty"
                                        | "isPrototypeOf"
                                        | "propertyIsEnumerable"
                                        | "toLocaleString"
                                        | "valueOf"
                                );
                                let narrowed_receiver = match &mem.object.kind {
                                    ExprKind::Ident(object_name) => {
                                        self.lookup_narrowed(object_name).is_some()
                                    }
                                    _ => false,
                                };
                                // A program-declared augmentation of `Function` /
                                // `Object` (`declare global { interface Function
                                // { now(): string } }`) applies to every class value.
                                // (`declare global` blocks register under a
                                // qualified key such as `global.Function`.)
                                let augmented = ["Function", "CallableFunction", "NewableFunction", "Object"]
                                    .iter()
                                    .any(|owner| {
                                        self.interface_info.iter().any(|(key, info)| {
                                            (key == owner
                                                || key
                                                    .rsplit_once('.')
                                                    .is_some_and(|(_, tail)| tail == *owner))
                                                && info
                                                    .object_type
                                                    .properties
                                                    .iter()
                                                    .any(|(n, _)| n == &mem.property)
                                        })
                                    });
                                if inherited
                                    || augmented
                                    || function_member
                                    || ci.extends.is_some()
                                    || self.in_js_expando_assignment
                                    || reported_future_lib_member
                                    || narrowed_receiver
                                    || self.current_file_is_js()
                                {
                                    return Type::Any;
                                }
                                self.diagnostics.push(error_property_not_exist(
                                    &mem.property,
                                    &format!("typeof {cls}"),
                                    property_span,
                                ));
                                return Type::Error;
                            }
                            return Type::Any;
                        }
                        // Check enum members first (Status.Active)
                        // Return the enum type reference rather than the literal value,
                        // matching TypeScript's behavior where enum member accesses have
                        // the enum type (e.g., `Colors`, not `number`).
                        // Namespace enums are keyed by their qualified path
                        // (`m.Color`) while the receiver reads `Color`. The
                        // qualified entry holds a single declaration's members
                        // (merged declarations aren't combined), so it is only
                        // used when it has the accessed member.
                        // A qualified receiver (`ns.Foo.X`) names that
                        // namespace's enum even when an unrelated `Foo` is
                        // registered under the bare name (another module's).
                        let qualified_enum_key = Self::simple_expression_path(&mem.object)
                            .filter(|path| {
                                path.as_str() != name.as_str()
                                    && path.rsplit('.').next() == Some(name.as_str())
                                    && self.enum_info.get(path.as_str()).is_some_and(|members| {
                                        members.iter().any(|(n, _)| n == mem.property.as_str())
                                    })
                                    && self.enum_info.get(name.as_str()).is_none_or(|members| {
                                        !members.iter().any(|(n, _)| n == mem.property.as_str())
                                    })
                            });
                        let enum_key = qualified_enum_key.as_deref().unwrap_or(name.as_str());
                        if let Some(members) = self.enum_info.get(enum_key) {
                            let primitive_member = self
                                .builtins
                                .lookup_instance_property("Number", &mem.property)
                                .or_else(|| self.builtins.lookup_instance_property("String", &mem.property))
                                .or_else(|| self.builtins.lookup_instance_property("Object", &mem.property));
                            let merged_with_namespace = self.namespace_paths.contains(name.as_str())
                                || self
                                    .namespace_paths
                                    .iter()
                                    .any(|path| path.rsplit_once('.').is_some_and(|(_, tail)| tail == name.as_str()))
                                || matches!(self.lookup_var(name), Some(Type::Namespace(_)) | Some(Type::Module(_)));
                            if let Some((_, value)) =
                                members.iter().find(|(n, _)| n == &mem.property.as_str())
                            {
                                // The member's own literal type (`E.A`), a
                                // subtype of the enum (tsc); mutable bindings
                                // widen it back to the enum.
                                Type::EnumVariant {
                                    enum_name: enum_key.into(),
                                    variant_name: mem.property.to_string(),
                                    value: Some(Arc::new(value.clone())),
                                }
                            } else if let Some(member) = primitive_member {
                                // An enum value is a number/string at runtime.
                                member
                            } else if self.in_js_expando_assignment
                                || reported_future_lib_member
                                || merged_with_namespace
                                || self.current_file_is_js()
                            {
                                Type::Any
                            } else {
                                // An enum value has no such member (the enum's
                                // own members are not properties of a value).
                                self.diagnostics.push(error_property_not_exist(
                                    &mem.property,
                                    &obj_ty.display_string_single_line(),
                                    property_span,
                                ));
                                Type::Error
                            }
                        } else {
                            // Build substitution map from class/interface/alias type params → type args
                            let subst_map: Option<HashMap<std::string::String, Type>> = {
                                let type_params = self
                                    .class_info
                                    .get(name.as_str())
                                    .map(|info| info.type_params.clone())
                                    .or_else(|| {
                                        self.interface_info
                                            .get(name.as_str())
                                            .map(|info| info.type_params.clone())
                                    })
                                    .or_else(|| {
                                        self.type_aliases
                                            .get(name.as_str())
                                            .map(|(params, _, _)| params.clone())
                                    });
                                type_params.and_then(|tps| {
                                    if tps.is_empty() || type_args.is_empty() {
                                        None
                                    } else {
                                        let mut map = HashMap::new();
                                        for (tp, ta) in tps.iter().zip(type_args.iter()) {
                                            map.insert(tp.clone(), ta.clone());
                                        }
                                        Some(map)
                                    }
                                })
                            };
                            // Prefer `resolve_type_reference_to_object` when type
                            // args are present (it does substitution + alias
                            // expansion in one pass). `get_class_instance_type`
                            // returns the unsubstituted shape and only handles
                            // class/interface — type aliases need the alias path.
                            // A resolved reference is ALREADY substituted; a
                            // second pass would re-substitute inside the type
                            // arguments (`Vec2_T<(a: A) => B>` → `(a: (a: A) =>
                            // B) => B`).
                            let resolved = if !type_args.is_empty() {
                                self.resolve_type_reference_to_object(name, type_args)
                            } else {
                                None
                            };
                            let already_substituted = resolved.is_some();
                            let instance_ty =
                                resolved.unwrap_or_else(|| self.get_class_instance_type(name));
                            let raw_ty = if let Type::ObjectType(ref info) = instance_ty {
                                if let Some((_, prop_ty)) =
                                    info.properties.iter().find(|(n, _)| n == &mem.property)
                                {
                                    match prop_ty.as_ref() {
                                        Type::Optional(inner) => Type::flatten_union(vec![
                                            Type::clone(&inner),
                                            Type::Undefined,
                                        ]),
                                        _ => Type::clone(&prop_ty),
                                    }
                                } else if let Some(builtin) =
                                    self.builtins.lookup_instance_property(name, &mem.property)
                                {
                                    builtin
                                } else {
                                    // Static member of the class. If the
                                    // receiver is the class NAME itself (the
                                    // constructor value — e.g. `C.a` inside
                                    // `class C { static c = C.a }`), resolve
                                    // to the static's type. On an INSTANCE
                                    // receiver it's TS2576 ("did you mean the
                                    // static member?").
                                    // Statics inherit: probe the whole
                                    // extends chain (`c.bar()` where bar is
                                    // a BASE static is TS2576, not TS2339).
                                    let static_ty = {
                                        let mut cur = Some(name.to_string());
                                        let mut found = None;
                                        for _ in 0..8 {
                                            let Some(cn) = cur.take() else { break };
                                            let Some(ci) = self.class_info.get(cn.as_str()) else {
                                                break;
                                            };
                                            found = ci
                                                .static_properties
                                                .iter()
                                                .find(|(n, _)| n == &mem.property)
                                                .map(|(_, t)| t.clone())
                                                .or_else(|| {
                                                    ci.static_methods
                                                        .iter()
                                                        .find(|(n, _)| n == &mem.property)
                                                        .map(|(_, ft)| Type::Function(ft.clone()))
                                                });
                                            if found.is_some() {
                                                break;
                                            }
                                            cur = ci.extends.clone();
                                        }
                                        found
                                    };
                                    match static_ty {
                                        Some(t)
                                            if matches!(
                                                &mem.object.kind,
                                                ExprKind::Ident(recv) if recv == name
                                            ) =>
                                        {
                                            t
                                        }
                                        Some(_) => {
                                            self.diagnostics.push(
                                                error_static_member_via_instance(
                                                    &mem.property,
                                                    name,
                                                    expr.span,
                                                ),
                                            );
                                            Type::Error
                                        }
                                        None => {
                                            // TS2339 on class-instance receivers:
                                            // the member exists neither on the
                                            // instance nor as a static. Same
                                            // blind-spot gates as `this.member`.
                                            let is_js_file = self
                                                .current_file_name
                                                .as_deref()
                                                .map(|f| {
                                                    f.ends_with(".js")
                                                        || f.ends_with(".jsx")
                                                        || f.ends_with(".mjs")
                                                        || f.ends_with(".cjs")
                                                })
                                                .unwrap_or(false);
                                            let mut reported = false;
                                            // A program-declared interface (or an alias of an
                                            // object shape) reports like a class instance:
                                            // the member exists on no declaration of it
                                            // (`declare global` merges use qualified keys).
                                            let augmented = self.interface_info.iter().any(|(key, info)| {
                                                (key == name
                                                    || key
                                                        .rsplit_once('.')
                                                        .is_some_and(|(_, tail)| tail == name.as_str()))
                                                    && info
                                                        .object_type
                                                        .properties
                                                        .iter()
                                                        .any(|(n, _)| n.trim_start_matches('?') == mem.property.as_str())
                                            });
                                            let interface_receiver = !self.class_info.contains_key(name.as_str())
                                                && self
                                                    .interface_info
                                                    .get(name.as_str())
                                                    .is_some_and(|info| !info.decl_file.is_empty())
                                                && !stdlib::any_lib_declares_global(name)
                                                && self.lookup_var(name).is_none()
                                                && self.interface_shape_fully_declared(name);
                                            let callable_receiver = matches!(&instance_ty, Type::ObjectType(ref info)
                                                if !info.call_signatures.is_empty() || !info.construct_signatures.is_empty());
                                            let function_member = callable_receiver
                                                && matches!(
                                                    mem.property.as_str(),
                                                    "call" | "apply" | "bind" | "toString" | "length" | "name"
                                                        | "arguments" | "caller" | "prototype"
                                                );
                                            if !is_js_file
                                                && !mem.property.starts_with('#')
                                                && interface_receiver
                                                && !augmented
                                                && !function_member
                                                && !self.contains_unresolved_type_param(&instance_ty)
                                                && !reported_future_lib_member
                                            {
                                                if let Type::ObjectType(ref info) = instance_ty {
                                                    let object_member = self
                                                        .builtins
                                                        .lookup_instance_property("Object", &mem.property)
                                                        .is_some();
                                                    let prop_missing = info.index_signature.is_none()
                                                        && !object_member
                                                        && !info.properties.iter().any(|(n, _)| {
                                                            n.trim_start_matches('?') == mem.property.as_str()
                                                        });
                                                    if prop_missing {
                                                        let property_span = Span::new(
                                                            expr.span.end.saturating_sub(mem.property.len() as u32),
                                                            expr.span.end,
                                                        );
                                                        self.diagnostics.push(error_property_not_exist(
                                                            &mem.property,
                                                            &obj_ty.display_string_single_line(),
                                                            property_span,
                                                        ));
                                                        reported = true;
                                                    }
                                                }
                                            }
                                            if !reported
                                                && !is_js_file
                                                && !mem.property.starts_with('#')
                                                && !self.interface_info.contains_key(name.as_str())
                                                && self.class_chain_fully_resolved(name)
                                            {
                                                if let Type::ObjectType(ref info) =
                                                    self.get_class_instance_type(name)
                                                {
                                                    let prop_missing = info
                                                        .index_signature
                                                        .is_none()
                                                        && !info.properties.iter().any(|(n, _)| {
                                                            let clean = n
                                                                .strip_prefix('?')
                                                                .or_else(|| n.strip_prefix("..."))
                                                                .unwrap_or(n);
                                                            clean == mem.property.as_str()
                                                        });
                                                                                                        if prop_missing {
                                                        let property_span = Span::new(
                                                            expr.span.end.saturating_sub(
                                                                mem.property.len() as u32,
                                                            ),
                                                            expr.span.end,
                                                        );
                                                        self.diagnostics.push(
                                                            error_property_not_exist(
                                                                &mem.property,
                                                                &obj_ty.display_string_single_line(),
                                                                property_span,
                                                            ),
                                                        );
                                                        reported = true;
                                                    }
                                                }
                                            }
                                            if reported {
                                                Type::Error
                                            } else {
                                                Type::Any
                                            }
                                        }
                                    }
                                }
                            } else {
                                // No class info – try built-in instance members
                                self.builtins
                                    .lookup_instance_property(name, &mem.property)
                                    .unwrap_or_else(|| {
                                        // A lib interface instance (`Promise<number>`) reports a
                                        // member that no active lib declaration provides.
                                        let js = self.current_file_is_js();
                                        self.lib_instance_member_fallback(
                                            name,
                                            &obj_ty,
                                            &mem.property,
                                            property_span,
                                            js,
                                            reported_future_lib_member,
                                        )
                                    })
                            };
                            // Substitute class type parameters if available
                            if let (Some(map), false) = (&subst_map, already_substituted) {
                                Self::substitute(&raw_ty, map)
                            } else {
                                raw_ty
                            }
                        }
                    }
                    // Resolve member access on primitive and well-known types via builtins
                    Type::String | Type::StringLiteral(_) => self
                        .builtins
                        .lookup_instance_property("String", &mem.property)
                        .unwrap_or_else(|| {
                            self.primitive_member_fallback(
                                "String",
                                &mem.object,
                                &obj_ty,
                                &mem.property,
                                property_span,
                                reported_future_lib_member,
                            )
                        }),
                    Type::Number | Type::NumberLiteral(_) => self
                        .builtins
                        .lookup_instance_property("Number", &mem.property)
                        .unwrap_or_else(|| {
                            self.primitive_member_fallback(
                                "Number",
                                &mem.object,
                                &obj_ty,
                                &mem.property,
                                property_span,
                                reported_future_lib_member,
                            )
                        }),
                    Type::Boolean | Type::BooleanLiteral(_) => self
                        .builtins
                        .lookup_instance_property("Boolean", &mem.property)
                        .unwrap_or_else(|| {
                            self.primitive_member_fallback(
                                "Boolean",
                                &mem.object,
                                &obj_ty,
                                &mem.property,
                                property_span,
                                reported_future_lib_member,
                            )
                        }),
                    Type::Array(ref elem) => self
                        .builtins
                        .lookup_array_method_typed(elem, &mem.property)
                        .or_else(|| {
                            self.builtins
                                .lookup_instance_property("Array", &mem.property)
                        })
                        .unwrap_or_else(|| {
                            if self.array_member_definitely_missing(&mem.property) {
                                let property_span = Span::new(
                                    expr.span.end.saturating_sub(mem.property.len() as u32),
                                    expr.span.end,
                                );
                                self.diagnostics.push(error_property_not_exist(
                                    &mem.property,
                                    &obj_ty.display_string(),
                                    property_span,
                                ));
                                Type::Error
                            } else {
                                Type::Any
                            }
                        }),
                    // Union: resolve property from each member
                    Type::Union(members) => {
                        let presences: Vec<_> = if self.complete_union_flow_depth > 0 {
                            members
                                .iter()
                                .map(|member| self.type_property_presence(member, &mem.property))
                                .collect()
                        } else {
                            vec![None; members.len()]
                        };
                        let all_known = presences.iter().all(Option::is_some);
                        let has_missing = presences.iter().any(|presence| *presence == Some(false));
                        let has_present = presences.iter().any(|presence| *presence == Some(true));
                        if self.complete_union_flow_depth > 0 && all_known && has_missing {
                            let property_span = Span::new(
                                expr.span.end.saturating_sub(mem.property.len() as u32),
                                expr.span.end,
                            );
                            let mut diagnostic = error_property_not_exist(
                                &mem.property,
                                &obj_ty.display_string(),
                                property_span,
                            );
                            if has_present {
                                if let Some((missing, _)) = members
                                    .iter()
                                    .zip(presences.iter())
                                    .find(|(_, presence)| **presence == Some(false))
                                {
                                    diagnostic.message.push_str(&format!(
                                        "\n  Property '{}' does not exist on type '{}'.",
                                        mem.property,
                                        missing.display_string()
                                    ));
                                }
                            }
                            self.diagnostics.push(diagnostic);
                        }
                        let prop_types: Vec<Type> = members
                            .iter()
                            .zip(presences.iter())
                            .filter(|(_, presence)| **presence != Some(false))
                            .map(|(member, _)| self.resolve_member_on_type(member, &mem.property))
                            .collect();
                        Type::flatten_union(prop_types)
                    }
                    // Intersection: property must exist in at least one member
                    Type::Intersection(members) => {
                        // A property declared by several constituents has the
                        // intersection of their declared types (`(X & Y).x` for
                        // `X { x: A }` and `Y { x: B }` is `A & B`).
                        let mut found: Vec<Type> = Vec::new();
                        for member in members.iter() {
                            let resolved = self.resolve_member_on_type(member, &mem.property);
                            if !matches!(resolved, Type::Any) && !found.contains(&resolved) {
                                found.push(resolved);
                            }
                        }
                        match found.len() {
                            0 => Type::Any,
                            1 => found.pop().unwrap(),
                            _ => Type::Intersection(Arc::from(found)),
                        }
                    }
                    // Tuple: access .length or numeric index properties
                    Type::Tuple(elems) => {
                        if mem.property.as_str() == "length" {
                            Type::NumberLiteral(elems.len().to_string())
                        } else {
                            // Tuple member access for array methods
                            let elem_union = Type::flatten_union(elems.iter().cloned().collect());
                            self.builtins
                                .lookup_array_method_typed(&elem_union, &mem.property)
                                .or_else(|| {
                                    self.builtins
                                        .lookup_instance_property("Array", &mem.property)
                                })
                                .unwrap_or(Type::Any)
                        }
                    }
                    // Function: .length, .name, .bind, .call, .apply
                    Type::Function(_) => match mem.property.as_str() {
                        "length" => Type::Number,
                        "name" => Type::String,
                        "bind" | "call" | "apply" => Type::Function(FunctionType {
                            type_param_constraints: Vec::new(),
                            params: vec![("thisArg".to_string(), Type::Any)],
                            return_type: Arc::new(Type::Any),
                            type_params: Vec::new(),
                            type_param_defaults: Vec::new(),
                            type_predicate: None,
                        }),
                        "prototype" => Type::Any,
                        _ => Type::Any,
                    },
                    // `this` in a class — resolve to the enclosing class instance type
                    Type::This => {
                        if let Some(class_name) = self.enclosing_class_names.last().cloned() {
                            if self.nearer_binding_is(STATIC_THIS_MARKER, THIS_OK_MARKER)
                                == Some(true)
                            {
                                if let Some(info) = self.class_info.get(class_name.as_str()) {
                                    if let Some((_, ty)) = info
                                        .static_properties
                                        .iter()
                                        .find(|(name, _)| name == property_name.as_str())
                                    {
                                        return ty.clone();
                                    }
                                    if let Some((_, method)) = info
                                        .static_methods
                                        .iter()
                                        .find(|(name, _)| name == property_name.as_str())
                                    {
                                        return Type::Function(method.clone());
                                    }
                                }
                                self.diagnostics.push(error_property_not_exist(
                                    &property_name,
                                    &format!("typeof {class_name}"),
                                    property_span,
                                ));
                                return Type::Any;
                            }
                            // TS2715: abstract property accessed through `this`
                            // during class initialization (ctor body / instance
                            // property initializer, not inside a nested
                            // function-like — the depth pairing exempts those).
                            self.push_abstract_property_in_constructor(
                                &class_name,
                                &property_name,
                                &property_name,
                                property_span,
                            );
                            // TS2339: `this.member` that exists NOWHERE on the
                            // class (reads and writes alike; TS classes have no
                            // expando). Gated hard against our own blind spots:
                            // JS files (expando is legal), interface-merged
                            // classes, index signatures, private `#` names, and
                            // any extends chain we can't fully resolve.
                            let is_js_file = self
                                .current_file_name
                                .as_deref()
                                .map(|f| {
                                    f.ends_with(".js")
                                        || f.ends_with(".jsx")
                                        || f.ends_with(".mjs")
                                        || f.ends_with(".cjs")
                                })
                                .unwrap_or(false);
                            if !is_js_file
                                && !property_name.starts_with('#')
                                && !self.interface_info.contains_key(class_name.as_str())
                                && self.class_chain_fully_resolved(&class_name)
                            {
                                if let Type::ObjectType(ref info) =
                                    self.get_class_instance_type(&class_name)
                                {
                                    let prop_missing = info.index_signature.is_none()
                                        && !info.properties.iter().any(|(n, _)| {
                                            let clean = n
                                                .strip_prefix('?')
                                                .or_else(|| n.strip_prefix("..."))
                                                .unwrap_or(n);
                                            clean == property_name.as_str()
                                        });
                                    if prop_missing {
                                        self.diagnostics.push(error_property_not_exist(
                                            &property_name,
                                            &class_name,
                                            property_span,
                                        ));
                                    }
                                }
                            }
                            self.resolve_member_on_type(
                                &Type::TypeReference(class_name, Arc::from([] as [Type; 0])),
                                &property_name,
                            )
                        } else {
                            Type::Any
                        }
                    }
                    // Null/Undefined without optional chain — already errored above
                    Type::Null | Type::Undefined => Type::Any,
                    Type::Never => {
                        let flow_proven = match &mem.object.kind {
                            ExprKind::Ident(name) => {
                                self.nearer_binding_is(
                                    &Self::switch_true_exit_marker_name(name),
                                    name,
                                ) == Some(true)
                            }
                            _ => false,
                        };
                        if !flow_proven {
                            return Type::Any;
                        }
                        let property_span = Span::new(
                            expr.span.end.saturating_sub(mem.property.len() as u32),
                            expr.span.end,
                        );
                        self.diagnostics.push(error_property_not_exist(
                            &mem.property,
                            "never",
                            property_span,
                        ));
                        Type::Error
                    }
                    _ => Type::Any,
                });
                // For optional chaining, the result is T | undefined
                if is_optional_chain && !matches!(member_ty, Type::Any | Type::Error | Type::Never)
                {
                    match &member_ty {
                        Type::Union(members)
                            if members.iter().any(|m| matches!(m, Type::Undefined)) =>
                        {
                            member_ty // already has undefined
                        }
                        _ => Type::flatten_union(vec![member_ty, Type::Undefined]),
                    }
                } else {
                    member_ty
                }
            }
            ExprKind::ElemAccess(ea) => {
                if let (ExprKind::Ident(object_name), ExprKind::StrLit(key)) =
                    (&ea.object.kind, &ea.index.kind)
                {
                    if object_name == "globalThis"
                        && self.lookup_var("globalThis").is_none()
                        && !self.current_file_is_js()
                    {
                        if let Some(ty) = self.global_this_member_type(key, expr.span) {
                            return ty;
                        }
                    }
                }
                let obj_ty = self.check_expr(&ea.object);
                let idx_ty = self.check_expr(&ea.index);
                if let Some(operand) = self.enum_element_access_type(expr, &idx_ty) {
                    return operand;
                }
                let is_symbol_iterator = matches!(
                    &ea.index.kind,
                    ExprKind::Member(member)
                        if member.property.as_str() == "iterator"
                            && matches!(&member.object.kind, ExprKind::Ident(name) if name.as_str() == "Symbol")
                );
                // TS2538: concretely-invalid index types. Conservatively
                // limited to shapes our inference gets right (bigint, arrays,
                // tuples) — string/number/symbol/enums/any stay valid, and
                // nullish/unknown/objects are excluded until inference
                // precision warrants them.
                let invalid_index_constituent = |ty: &Type| {
                    matches!(
                        ty,
                        Type::BigInt | Type::BigIntLiteral(_) | Type::Array(_) | Type::Tuple(_)
                    )
                };
                let syntactic_nullish = matches!(&ea.index.kind, ExprKind::NullLit)
                    || matches!(&ea.index.kind, ExprKind::Ident(name) if name == "undefined");
                let offending_index = if invalid_index_constituent(&idx_ty) {
                    Some(idx_ty.clone())
                } else if let Type::Union(members) = &idx_ty {
                    // tsc checks each constituent and names the offender.
                    members
                        .iter()
                        .find(|member| invalid_index_constituent(member))
                        .cloned()
                } else if syntactic_nullish && matches!(idx_ty, Type::Undefined | Type::Null) {
                    Some(idx_ty.clone())
                } else {
                    None
                };
                if let Some(offending) = offending_index {
                    // A bigint literal index reports its base type.
                    let shown = match offending {
                        Type::BigIntLiteral(_) => Type::BigInt,
                        other => other,
                    };
                    self.diagnostics.push(error_invalid_index_type(
                        &shown.display_string(),
                        ea.index.span,
                    ));
                }
                // `noUncheckedIndexedAccess`: any indexed access via a
                // dynamic index (number on Array / string on index-signed
                // object) returns `T | undefined` because the index may
                // be out of bounds / missing. Skipped for tuple element
                // access via a NumLit (the element is known to exist)
                // and for ObjectType.property lookups that hit a known
                // declared property (those aren't going through the
                // index signature).
                let unchecked = self
                    .compiler_options
                    .no_unchecked_indexed_access
                    .unwrap_or(false);
                let wrap_undef = |ty: Type| {
                    if unchecked {
                        Type::flatten_union(vec![ty, Type::Undefined])
                    } else {
                        ty
                    }
                };
                match &obj_ty {
                    Type::Array(elem) if is_symbol_iterator => Type::Function(FunctionType {
                        type_param_constraints: Vec::new(),
                        params: Vec::new(),
                        return_type: Arc::new(Type::TypeReference(
                            "ArrayIterator".to_string(),
                            vec![Type::clone(elem)].into(),
                        )),
                        type_params: Vec::new(),
                        type_param_defaults: Vec::new(),
                        type_predicate: None,
                    }),
                    Type::Array(elem) => wrap_undef(Type::clone(&elem)),
                    Type::Tuple(elems) => {
                        // A literal index or a union made entirely of valid
                        // tuple indices is provably in bounds. This includes
                        // expressions such as `i as 0 | 1 | 2`, not only a
                        // syntactic numeric literal.
                        let exact = match &idx_ty {
                            Type::NumberLiteral(value) => value
                                .parse::<usize>()
                                .ok()
                                .and_then(|index| elems.get(index).cloned()),
                            Type::Union(indices) => {
                                let mut selected = Vec::new();
                                let all_in_bounds = indices.iter().all(|index| {
                                    let Type::NumberLiteral(value) = index else {
                                        return false;
                                    };
                                    let Some(element) = value
                                        .parse::<usize>()
                                        .ok()
                                        .and_then(|index| elems.get(index))
                                        .cloned()
                                    else {
                                        return false;
                                    };
                                    if !selected.contains(&element) {
                                        selected.push(element);
                                    }
                                    true
                                });
                                all_in_bounds.then(|| Type::flatten_union(selected))
                            }
                            _ => None,
                        };
                        exact.unwrap_or_else(|| {
                            wrap_undef(Type::flatten_union(elems.iter().cloned().collect()))
                        })
                    }
                    // ObjectType: check string literal property access or index signature
                    Type::ObjectType(info) => {
                        let mut found = None;
                        if let ExprKind::StrLit(ref key) = ea.index.kind {
                            if let Some((_, prop_ty)) =
                                info.properties.iter().find(|(n, _)| n == key)
                            {
                                found = Some(Type::clone(&prop_ty));
                            }
                        }
                        found.unwrap_or_else(|| {
                            if let Some((_, ref val_ty)) = info.index_signature {
                                wrap_undef(Type::clone(&val_ty))
                            } else {
                                Type::Any
                            }
                        })
                    }
                    // TypeReference: resolve to ObjectType and use index signature
                    Type::TypeReference(ref name, ref type_args) => {
                        let instance = if !type_args.is_empty() {
                            self.resolve_type_reference_to_object(name, type_args)
                                .unwrap_or_else(|| self.get_class_instance_type(name))
                        } else {
                            self.get_class_instance_type(name)
                        };
                        if let Type::ObjectType(ref info) = instance {
                            let mut found = None;
                            if let ExprKind::StrLit(ref key) = ea.index.kind {
                                if let Some((_, prop_ty)) =
                                    info.properties.iter().find(|(n, _)| n == key)
                                {
                                    found = Some(Type::clone(&prop_ty));
                                }
                            }
                            found.unwrap_or_else(|| {
                                if let Some((_, ref val_ty)) = info.index_signature {
                                    wrap_undef(Type::clone(&val_ty))
                                } else {
                                    Type::Any
                                }
                            })
                        } else {
                            Type::Any
                        }
                    }
                    // String element access returns string
                    Type::String | Type::StringLiteral(_) => {
                        if matches!(idx_ty, Type::Number | Type::NumberLiteral(_)) {
                            wrap_undef(Type::String)
                        } else {
                            Type::Any
                        }
                    }
                    _ => Type::Any,
                }
            }
            ExprKind::Cond(cond) => {
                // Apply narrowing to both branches of the ternary the same
                // way `StmtKind::If` does it. Without this, the helper
                //   function pickString(value: unknown, fallback: string): string {
                //     return typeof value === "string" ? value : fallback;
                //   }
                // was evaluating the true-branch `value` without the
                // typeof narrowing in scope and producing `unknown | string`
                // for the conditional result, then tripping TS2322 against
                // the `string` return type. The narrowing scope must wrap
                // each branch's `check_expr` so `lookup_var(value)` in the
                // true branch returns `string` and `lookup_var(value)` in
                // the false branch returns the negated complement.
                self.check_syntactic_truthiness(&cond.test);
                self.check_uncalled_function_condition(&cond.test, Some(&cond.consequent), None);
                self.uncalled_condition_depth += 1;
                self.check_expr(&cond.test);
                self.uncalled_condition_depth -= 1;
                let (consequent_narrows, alternate_narrows) = self.analyze_narrowing(&cond.test);
                self.push_scope();
                for (name, ty) in &consequent_narrows {
                    self.narrow_var(name, ty.clone());
                }
                let supers_before_cons = self.super_call_count;
                // A literal `false` test makes the consequent unreachable
                // (tsc reports no TS2454 there); likewise `true` for the
                // alternate.
                let consequent_dead = matches!(cond.test.kind, ExprKind::BoolLit(false));
                let alternate_dead = matches!(cond.test.kind, ExprKind::BoolLit(true));
                if consequent_dead {
                    self.unreachable_read_depth += 1;
                }
                let t = self.check_expr(&cond.consequent);
                if consequent_dead {
                    self.unreachable_read_depth -= 1;
                }
                let supers_in_cons = self.super_call_count - supers_before_cons;
                self.pop_scope();
                self.push_scope();
                for (name, ty) in &alternate_narrows {
                    self.narrow_var(name, ty.clone());
                }
                let supers_before_alt = self.super_call_count;
                if alternate_dead {
                    self.unreachable_read_depth += 1;
                }
                let f = self.check_expr(&cond.alternate);
                if alternate_dead {
                    self.unreachable_read_depth -= 1;
                }
                self.pop_scope();
                // TS17009 join rule (`cond ? super() : super()`).
                if supers_in_cons > 0 && self.super_call_count > supers_before_alt {
                    self.declare_var(THIS_OK_MARKER, Type::Never);
                }
                Self::reduce_empty_array_union(Type::flatten_union(vec![t, f]))
            }
            ExprKind::Assign(assign) => {
                let readonly_write = self.report_readonly_property_write(&assign.left);
                if assign.op == AssignOp::Assign && matches!(&assign.right.kind, ExprKind::This) {
                    self.check_abstract_this_assignment_pattern(&assign.left);
                }
                if let ExprKind::Ident(name) = &assign.left.kind {
                    self.push_strict_mode_name_error(name, assign.left.span);
                }
                // TS2629/TS2628: a compound assignment (`f += 1`, `E *= 2`)
                // whose target is a class/enum BINDING is invalid — tsc's
                // checkReferenceExpression emits exactly one error and
                // suppresses operand-type checks. The class binding is
                // recognized by its self-referential hoist type
                // `TypeReference(name, [])` (a shadowing let/param binds a
                // different type first in lookup_var, avoiding cross-file
                // name-collision false positives; scoped to compound ops so
                // the plain-assignment path keeps its existing diagnostics).
                if self.report_invalid_assignment_target(&assign.left) {
                    self.check_expr(&assign.right);
                    return Type::Error;
                }
                // TS2540: the assignment target is a `readonly` property or a
                // getter-only accessor. The receiver's declared class supplies
                // the member set; `this.x = …` inside the declaring class's
                // CONSTRUCTOR is legal, so that case is exempt.
                if let ExprKind::Member(m) = &assign.left.kind {
                    // Keep this branch limited to the same direct Member
                    // targets it has always handled. The shared helper gives
                    // the diagnostic the property-token span without
                    // broadening readonly detection to its other supported
                    // target shapes.
                    let property_span = Self::constant_property_write_target(&assign.left)
                        .map(|(_, _, span)| span)
                        .unwrap_or(assign.left.span);
                    // Type the receiver WITHOUT re-emitting its diagnostics —
                    // the LHS is checked again below, and duplicating the
                    // errors here inflates the count.
                    let dstart = self.diagnostics.len();
                    let recv_ty = self.check_expr(&m.object);
                    self.diagnostics.truncate(dstart);
                    let cls = match &recv_ty {
                        Type::TypeReference(n, _) => n
                            .strip_prefix("typeof ")
                            .map(|c| c.to_string())
                            .or_else(|| Some(n.to_string())),
                        // `this.x = …` inside a method: the receiver is the
                        // enclosing class.
                        Type::This => self.enclosing_class_names.last().cloned(),
                        _ => None,
                    };
                    let cls = cls.or_else(|| match &m.object.kind {
                        ExprKind::This => self.enclosing_class_names.last().cloned(),
                        _ => None,
                    });
                    let in_own_ctor = matches!(&m.object.kind, ExprKind::This)
                        && self.in_constructor
                        && cls
                            .as_deref()
                            .map(|c| {
                                self.enclosing_class_names.last().map(|e| e.as_str()) == Some(c)
                            })
                            .unwrap_or(false);
                    if let Some(cls) = cls {
                        if !in_own_ctor {
                            let ro = self
                                .class_info
                                .get(cls.as_str())
                                .map(|i| i.readonly_members.contains(m.property.as_str()))
                                .unwrap_or(false)
                                || self
                                    .interface_info
                                    .get(cls.as_str())
                                    .map(|i| i.readonly_members.contains(m.property.as_str()))
                                    .unwrap_or(false)
                                // Enum members are read-only (`E.A = 1`).
                                || self.enum_info.contains_key(cls.as_str());
                            if ro {
                                self.diagnostics.push(
                                    crate::diagnostics::error_cannot_assign_readonly(
                                        &m.property,
                                        property_span,
                                    ),
                                );
                            }
                        }
                    }
                }
                // TS2454: the assignment target itself is not a read, but the
                // right-hand side still is (`x = x.concat([])`), so the
                // binding leaves the unassigned set only after the RHS check.
                let saved_assignment_target = self.assignment_target_span;
                if assign.op == AssignOp::Assign && matches!(assign.left.kind, ExprKind::Ident(_)) {
                    self.assignment_target_span = Some(assign.left.span);
                }
                // For an Ident LHS, use the DECLARED type (vars), not the
                // narrowed view. TS-correct semantics: `let x: A = ...;
                // if (!cond) return; x = newValue` — the assignment of
                // `newValue` is checked against `A`, not against the
                // narrowing of x inside the post-early-return scope.
                // Without this, early-return narrowing trips genuine
                // re-assignments (real case: `redis-cache.ts` reassigns
                // `_redis = false` after the `_redis !== null` early
                // return — narrowed `_redis: null`, but the declared type
                // accepts `false`).
                let left_ty = if assign.op == AssignOp::Assign {
                    match &assign.left.kind {
                        ExprKind::Ident(name) => self
                            .lookup_declared_var(name)
                            .unwrap_or_else(|| self.check_expr(&assign.left)),
                        ExprKind::Member(_) | ExprKind::ElemAccess(_)
                            if self.current_file_is_js() =>
                        {
                            // The binder treats JavaScript property assignments
                            // as declarations. Seed an otherwise-undeclared root
                            // before checking the target so `ns.next = ns.next ||
                            // {}` establishes `ns` instead of producing TS2304.
                            if let Some(root) = Self::js_expando_assignment_root(&assign.left) {
                                if self.lookup_var(root).is_none() {
                                    self.declare_var(root, Type::Any);
                                }
                            }

                            let was_in_expando = self.in_js_expando_assignment;
                            self.in_js_expando_assignment = true;
                            let ty = self.check_expr(&assign.left);
                            self.in_js_expando_assignment = was_in_expando;
                            ty
                        }
                        ExprKind::Member(member)
                            if match &member.object.kind {
                                ExprKind::Ident(name) => match self.lookup_var(name) {
                                    Some(Type::Function(_)) => true,
                                    Some(Type::ObjectType(info)) => {
                                        !info.call_signatures.is_empty()
                                    }
                                    _ => false,
                                },
                                _ => false,
                            } =>
                        {
                            let was_in_expando = self.in_js_expando_assignment;
                            self.in_js_expando_assignment = true;
                            let ty = self.check_expr(&assign.left);
                            self.in_js_expando_assignment = was_in_expando;
                            ty
                        }
                        ExprKind::Member(_) => {
                            // Assignment target: check against the property's
                            // DECLARED type, not a narrowed path view (mirrors
                            // the Ident `lookup_declared_var` case above). The
                            // flag suppresses the whole-path narrowing lookup
                            // for this outermost target only — it self-clears
                            // in check_expr's Member arm.
                            self.assign_lhs_declared = true;
                            let ty = self.check_expr(&assign.left);
                            self.assign_lhs_declared = false;
                            ty
                        }
                        ExprKind::ObjectLit(_) | ExprKind::ArrayLit(_) => {
                            // Destructuring assignment pattern.
                            self.assignment_pattern_depth += 1;
                            let ty = self.check_expr(&assign.left);
                            self.assignment_pattern_depth -= 1;
                            ty
                        }
                        _ => self.check_expr(&assign.left),
                    }
                } else {
                    self.check_expr(&assign.left)
                };
                // Contextual-type the RHS by the LHS so literal-narrowing
                // survives across `x = [{ createdAt: "asc" }]` (Prisma
                // `orderBy` shape — `{createdAt?: "asc"|"desc"; ...}[]`).
                // Without this, the ArrayLit branch widens `"asc"` to
                // `string` and the assign fails TS2322. Only fires on
                // simple `=`; compound assignments keep the original
                // unwidened path so e.g. `s += 1` doesn't try to use
                // `string` as a contextual type for `1`.
                self.assignment_target_span = saved_assignment_target;
                let right_ty = if assign.op == AssignOp::Assign
                    && !matches!(left_ty, Type::Any | Type::Error)
                {
                    self.check_expr_contextual(&assign.right, Some(&left_ty))
                } else {
                    self.check_expr(&assign.right)
                };
                if assign.op == AssignOp::Assign {
                    if let ExprKind::Ident(ref name) = assign.left.kind {
                        self.uninitialized_vars.remove(name.as_str());
                    }
                }
                // A proven readonly target is the assignment error. TypeScript
                // still checks the RHS (including contextual typing and
                // diagnostics within it), but suppresses secondary target
                // compatibility, operator, and excess-property diagnostics.
                if readonly_write {
                    return right_ty;
                }
                // Property assignments can declare expando members on a
                // function value. Preserve its call signature while evolving
                // the variable into a callable object with the new property,
                // so subsequent structural assignments see both halves of
                // the value (`fn.extra = value; const x: HasExtra = fn`).
                if assign.op == AssignOp::Assign && !self.current_file_is_js() {
                    if let ExprKind::Member(member) = &assign.left.kind {
                        if let ExprKind::Ident(name) = &member.object.kind {
                            let expanded = self.lookup_var(name).cloned().and_then(|base| {
                                let mut info = match base {
                                    Type::Function(signature) => ObjectTypeInfo {
                                        properties: Vec::new(),
                                        call_signatures: vec![signature],
                                        construct_signatures: Vec::new(),
                                        index_signature: None,
                                        index_signature_name: None,
                                        method_names: Vec::new(),
                                    },
                                    Type::ObjectType(info) if !info.call_signatures.is_empty() => {
                                        info
                                    }
                                    _ => return None,
                                };
                                if !info
                                    .properties
                                    .iter()
                                    .any(|(property, _)| property == &member.property)
                                    && !matches!(
                                        member.property.as_str(),
                                        "length"
                                            | "name"
                                            | "bind"
                                            | "call"
                                            | "apply"
                                            | "caller"
                                            | "prototype"
                                            | "toString"
                                    )
                                {
                                    info.properties.push((
                                        member.property.to_string(),
                                        Arc::new(self.widen_type(&right_ty)),
                                    ));
                                }
                                Some(Type::ObjectType(info))
                            });
                            if let Some(expanded) = expanded {
                                self.narrow_var(name, expanded);
                            }
                        }
                    }
                }
                if matches!(
                    assign.op,
                    AssignOp::AddAssign
                        | AssignOp::SubAssign
                        | AssignOp::MulAssign
                        | AssignOp::DivAssign
                        | AssignOp::ModAssign
                        | AssignOp::ExpAssign
                        | AssignOp::BitAndAssign
                        | AssignOp::BitOrAssign
                        | AssignOp::BitXorAssign
                        | AssignOp::ShlAssign
                        | AssignOp::ShrAssign
                        | AssignOp::UShrAssign
                ) {
                    let bool_pair = matches!(
                        assign.op,
                        AssignOp::BitAndAssign | AssignOp::BitOrAssign | AssignOp::BitXorAssign
                    ) && self.is_boolean_operand(&left_ty)
                        && self.is_boolean_operand(&right_ty);
                    if bool_pair {
                        let (op, sug) = match assign.op {
                            AssignOp::BitAndAssign => ("&=", "&&"),
                            AssignOp::BitOrAssign => ("|=", "||"),
                            _ => ("^=", "!=="),
                        };
                        self.diagnostics
                            .push(error_boolean_operator(op, sug, expr.span));
                    } else if assign.op == AssignOp::AddAssign {
                        if !(self.strict_null_checks
                            && !self.binary_operator_type_is_string_like(&left_ty)
                            && self.report_nullish_literal_operand(&assign.right))
                        {
                            self.report_addition_operator_incompatibility(
                                "+=",
                                &left_ty,
                                &right_ty,
                                &assign.left,
                                &assign.right,
                                expr.span,
                            );
                        }
                    } else if {
                        let left_reported = self.report_nullish_operand(&assign.left, &left_ty);
                        let right_reported = self.report_nullish_operand(&assign.right, &right_ty);
                        let _ = left_reported;
                        right_reported && matches!(assign.right.kind, ExprKind::NullLit)
                            || matches!(&assign.right.kind, ExprKind::Ident(n) if n == "undefined")
                    } {
                    } else if self.report_concrete_numeric_operator_incompatibility(
                        match assign.op {
                            AssignOp::AddAssign => "+=",
                            AssignOp::SubAssign => "-=",
                            AssignOp::MulAssign => "*=",
                            AssignOp::DivAssign => "/=",
                            AssignOp::ModAssign => "%=",
                            AssignOp::ExpAssign => "**=",
                            AssignOp::BitAndAssign => "&=",
                            AssignOp::BitOrAssign => "|=",
                            AssignOp::BitXorAssign => "^=",
                            AssignOp::ShlAssign => "<<=",
                            AssignOp::ShrAssign => ">>=",
                            AssignOp::UShrAssign => ">>>=",
                            _ => unreachable!(),
                        },
                        &left_ty,
                        &right_ty,
                        expr.span,
                    ) {
                    } else {
                        if self.arithmetic_operand_invalid(&left_ty) {
                            self.diagnostics
                                .push(error_arithmetic_lhs(assign.left.span));
                        }
                        if self.arithmetic_operand_invalid(&right_ty) {
                            self.diagnostics
                                .push(error_arithmetic_rhs(assign.right.span));
                        }
                    }
                }
                // An object destructuring target is checked property by
                // property (tsc's checkObjectLiteralAssignment), not as a whole.
                let object_target = matches!(assign.left.kind, ExprKind::ObjectLit(_));
                if assign.op == AssignOp::Assign && object_target {
                    if let ExprKind::ObjectLit(properties) = &assign.left.kind {
                        self.check_object_destructuring_properties(properties, &right_ty);
                    }
                }
                if assign.op == AssignOp::Assign
                    && !object_target
                    && !matches!(left_ty, Type::Any | Type::Error)
                {
                    let assignable = self.is_assignable_to(&right_ty, &left_ty);
                    // tsc: elaborate into a fresh literal / arrow body first,
                    // then excess properties, then the plain error.
                    let mut reported = false;
                    if !assignable {
                        reported = self.elaborate_error(&assign.right, &right_ty, &left_ty, None);
                    }
                    if !reported
                        && (Self::is_fresh_object_literal(&assign.right)
                            || matches!(assign.right.kind, ExprKind::ArrayLit(_)))
                    {
                        let before = self.diagnostics.len();
                        self.check_excess_properties(
                            &right_ty,
                            &left_ty,
                            assign.right.span,
                            Some(&assign.right),
                        );
                        reported = self.diagnostics.len() > before;
                    }
                    if !reported && !assignable {
                        // Widen literal types in source for error message (tsc behavior)
                        let widened_right =
                            self.widen_for_message(Some(&assign.right), &right_ty, &left_ty);
                        // An optional property target prints as `T | undefined`
                        // under strictNullChecks (its declared type).
                        let mut display_target = left_ty.clone();
                        if self.strict_null_checks {
                            if let ExprKind::Member(member) = &assign.left.kind {
                                let object_ty = self.infer_expr_type(&member.object);
                                if let Type::Optional(inner) =
                                    self.resolve_member_on_type(&object_ty, &member.property)
                                {
                                    display_target = Type::flatten_union(vec![
                                        Type::clone(&inner),
                                        Type::Undefined,
                                    ]);
                                }
                            }
                        }
                        // Use LHS span for squiggle, matching tsc behavior
                        self.message_source_is_fresh_object_literal =
                            Self::is_fresh_object_literal(&assign.right);
                        self.push_not_assignable(&widened_right, &display_target, assign.left.span);
                        self.message_source_is_fresh_object_literal = false;
                    }
                }
                // After an assignment to an Ident, clear that name's
                // narrowing entry. Subsequent reads should see the
                // declared type (or fall to its new narrowing) — without
                // this, an early-return-narrowed variable that later
                // gets reassigned still appears as the old narrow type,
                // tripping bogus null/undefined diagnostics on the
                // post-assignment reads.
                if assign.op == AssignOp::Assign {
                    if let ExprKind::Ident(ref name) = assign.left.kind {
                        self.clear_narrowed_var(name);
                        // ASSIGNMENT NARROWING: after `x = <non-nullish>` the
                        // reference's flow type drops the nullish members of
                        // its declared type — `let d: Date | undefined; … d =
                        // new Date(); d.setDate(…)` must not report
                        // possibly-undefined. Deliberately limited to removing
                        // `null`/`undefined` (rather than adopting the RHS type
                        // wholesale) so literal widening for `let` is
                        // unaffected and the declared type still bounds every
                        // later read.
                        if !matches!(right_ty, Type::Any | Type::Error)
                            && !Self::type_is_nullish(&right_ty)
                        {
                            if let Some(declared) = self.lookup_declared_var(name) {
                                // tsc getAssignmentReducedType: keep the declared
                                // constituents the assigned value fits; an invalid
                                // assignment leaves the declared type in place.
                                if let Type::Union(members) = &declared {
                                    let kept: Vec<Type> = members
                                        .iter()
                                        .filter(|member| self.is_assignable_to(&right_ty, member))
                                        .cloned()
                                        .collect();
                                    if !kept.is_empty() && kept.len() < members.len() {
                                        self.narrow_var(name, Type::flatten_union(kept));
                                    }
                                }
                            }
                        }
                    }
                }
                right_ty
            }
            ExprKind::ArrayLit(elems) => {
                let mut elem_types = Vec::new();
                for elem in elems.iter().flatten() {
                    let ty = if let Some((name, default_expr)) = self.pattern_default_target(elem) {
                        // `[x = ""] = [1]`: the element target is the VARIABLE
                        // `x`; its declared type — not the default's literal
                        // type — is what the source element must satisfy.
                        self.check_expr(default_expr);
                        self.uninitialized_vars.remove(name.as_str());
                        self.lookup_declared_var(&name)
                            .or_else(|| self.lookup_var(&name).cloned())
                            .map(|t| self.widen_type(&t))
                            .unwrap_or(Type::Any)
                    } else if self.assignment_pattern_depth > 0
                        && matches!(elem.kind, ExprKind::Ident(_))
                    {
                        let saved = self.assignment_target_span;
                        self.assignment_target_span = Some(elem.span);
                        let ty = self.check_expr(elem);
                        self.assignment_target_span = saved;
                        // An assignment target is checked against its
                        // DECLARED type, not the narrowed one (`[x] = [true]`
                        // after `x = ""`).
                        match &elem.kind {
                            ExprKind::Ident(name) => self.lookup_declared_var(name).unwrap_or(ty),
                            _ => ty,
                        }
                    } else {
                        self.check_expr(elem)
                    };
                    let preserve_narrow =
                        Self::is_const_assertion_expr(elem) || !Self::is_fresh_literal_expr(elem);
                    let widened = if preserve_narrow {
                        ty
                    } else {
                        self.widen_array_elem(&ty)
                    };
                    if !elem_types.iter().any(|t: &Type| t == &widened) {
                        elem_types.push(widened);
                    }
                }
                // tsc orders nullish members last in inferred element unions
                // (`[undefined, "q"]` → `(string | undefined)[]`).
                elem_types.sort_by_key(|t| matches!(t, Type::Null | Type::Undefined));
                let elem_ty = if elem_types.is_empty() {
                    Type::Any
                } else if elem_types.len() == 1 {
                    elem_types.into_iter().next().unwrap()
                } else if let Some(bct) = self.best_common_type(&elem_types) {
                    // A candidate that all others are assignable to is the best
                    // common type (tsc collapses `[i, {..}]` to `I[]`).
                    bct
                } else {
                    Type::Union(elem_types.into())
                };
                Type::Array(Arc::new(elem_ty))
            }
            ExprKind::ObjectLit(props) => {
                if self.check_expression_grammar && self.assignment_pattern_depth == 0 {
                    self.check_object_literal_name_conflicts(props);
                }
                let mut properties = Vec::new();
                let mut any_spread_plain = false;
                for prop in props {
                    match prop {
                        ObjLitProp::Property(p) => {
                            // TS1539: bigint literals cannot be property names
                            // (both `{1n: x}` and computed `{[1n]: x}`).
                            if let PropName::Number(n, sp) = &p.key {
                                if n.ends_with('n') {
                                    self.diagnostics.push(error_bigint_property_name(*sp));
                                }
                            }
                            // Check computed property key expressions
                            // (TS2464 for an invalid key type).
                            if let PropName::Computed(..) = p.key {
                                self.check_property_name_expression(&p.key);
                            }
                            let key = self.prop_name_to_string(&p.key);
                            // In a destructuring assignment pattern a bare
                            // identifier value is the assignment target.
                            let val_ty = if let Some((name, default_expr)) =
                                self.pattern_default_target(&p.value)
                            {
                                // `({ y: x = /a/ } = src)`: the target is the
                                // VARIABLE `x` at its declared type.
                                self.check_expr(default_expr);
                                self.uninitialized_vars.remove(name.as_str());
                                self.lookup_declared_var(&name)
                                    .or_else(|| self.lookup_var(&name).cloned())
                                    .map(|t| self.widen_type(&t))
                                    .unwrap_or(Type::Any)
                            } else if self.assignment_pattern_depth > 0
                                && matches!(p.value.kind, ExprKind::Ident(_))
                            {
                                let saved = self.assignment_target_span;
                                self.assignment_target_span = Some(p.value.span);
                                let ty = self.check_expr(&p.value);
                                self.assignment_target_span = saved;
                                // A target is checked against its DECLARED type.
                                match &p.value.kind {
                                    ExprKind::Ident(name) => {
                                        self.lookup_declared_var(name).unwrap_or(ty)
                                    }
                                    _ => ty,
                                }
                            } else {
                                self.check_expr(&p.value)
                            };
                            properties.push((key, Arc::new(val_ty)));
                        }
                        ObjLitProp::Shorthand(name, span) => {
                            self.check_shorthand_name_exists(name, *span);
                            // `{ x }` reads `x` exactly like a bare identifier,
                            // unless the literal is a destructuring target.
                            if self.assignment_pattern_depth > 0 {
                                self.uninitialized_vars.remove(name.as_str());
                            } else {
                                self.check_read_before_assignment(name, *span);
                            }
                            let ty = if self.report_value_position_global(name, *span)
                                || (stdlib::is_future_lib_global_diagnostic_name(name)
                                    && self.report_future_lib_global(name, *span))
                            {
                                Type::Error
                            } else if self.assignment_pattern_depth > 0 {
                                // A target is checked against its DECLARED type.
                                self.lookup_declared_var(name)
                                    .or_else(|| self.lookup_var(name).cloned())
                                    .unwrap_or(Type::Any)
                            } else {
                                self.lookup_var(name).cloned().unwrap_or(Type::Any)
                            };
                            properties.push((name.to_string(), Arc::new(ty)));
                        }
                        ObjLitProp::ShorthandDefault(name, default_expr, span) => {
                            // Outside a destructuring target `{ a = 1 }` is
                            // only the grammar error TS1312.
                            if self.assignment_pattern_depth > 0 {
                                self.check_shorthand_name_exists(
                                    name,
                                    Span::new(span.start, span.start + name.len() as u32),
                                );
                            }
                            // `({ s = 5 } = source)`: the target property is
                            // the VARIABLE `s` (assigned, not read); its
                            // declared type — not the default's literal type
                            // — is what the source property must satisfy.
                            self.check_expr(default_expr);
                            if self.assignment_pattern_depth > 0 {
                                self.uninitialized_vars.remove(name.as_str());
                                let ty = self
                                    .lookup_declared_var(name)
                                    .or_else(|| self.lookup_var(name).cloned())
                                    .map(|t| self.widen_type(&t))
                                    .unwrap_or(Type::Any);
                                properties.push((name.to_string(), Arc::new(ty)));
                            } else {
                                let ty = self.check_expr(default_expr);
                                properties.push((name.to_string(), Arc::new(ty)));
                            }
                        }
                        ObjLitProp::Spread(e, _) => {
                            // `({ ...rest } = value)`: the spread target is
                            // assigned, not read.
                            let spread_ty = if self.assignment_pattern_depth > 0
                                && matches!(e.kind, ExprKind::Ident(_))
                            {
                                if let ExprKind::Ident(name) = &e.kind {
                                    self.uninitialized_vars.remove(name.as_str());
                                }
                                let saved = self.assignment_target_span;
                                self.assignment_target_span = Some(e.span);
                                let ty = self.check_expr(e);
                                self.assignment_target_span = saved;
                                ty
                            } else {
                                self.check_expr(e)
                            };
                            // Spreading `any` makes the whole literal `any`
                            // (tsc's getSpreadType) — flag it and collapse
                            // after the loop.
                            if matches!(spread_ty, Type::Any) {
                                any_spread_plain = true;
                            }
                            let before = properties.len();
                            let _ = self
                                .merge_spread_into_object_properties(&spread_ty, &mut properties);
                            // If the spread couldn't surface ANY structural
                            // properties (typical for `z.infer<typeof Schema>`
                            // and other complex aliases that don't reduce to
                            // a plain ObjectType, OR a spread of `any`), leave a
                            // tombstone Any property so the produced literal
                            // stays lenient for downstream member access.
                            // Without this, a sibling `description:
                            // <narrowed-string>` would make the whole object
                            // `{description: string}` (only that prop), tripping
                            // TS2339 on every other `obj.X` access that
                            // previously fell through the "has-any-property →
                            // lenient" path. `{ …anyValue }` similarly makes
                            // every property `any` in TS, so it must stay
                            // lenient too.
                            if properties.len() == before
                                && !matches!(spread_ty, Type::ObjectType(_))
                            {
                                properties.push(("__spread__".to_string(), Arc::new(Type::Any)));
                            }
                        }
                        ObjLitProp::Method(m) => {
                            let name = self.prop_name_to_string(&m.name);
                            self.push_scope();
                            self.fn_nesting_depth += 1;
                            self.jump_function_depth += 1;
                            let pushed_type_params =
                                self.push_active_type_param_names(m.type_params.as_deref());
                            self.check_active_type_param_declarations(m.type_params.as_deref());
                            self.check_function_like_future_lib_globals(
                                &m.params,
                                m.return_type.as_ref(),
                            );
                            let saved_var_first_types = std::mem::take(&mut self.var_first_types);
                            self.check_parameter_decorators(&m.params, false);
                            self.check_parameter_runtime_expressions(&m.params, true, false);
                            self.declare_var("arguments", Type::Any);
                            self.declare_var(SUPER_PROPERTY_OK_MARKER, Type::Never);
                            self.declare_var(THIS_OK_MARKER, Type::Never);
                            self.bind_function_this(&m.params);
                            // Register method parameters in scope
                            let params: Vec<(std::string::String, Type)> = m
                                .params
                                .iter()
                                .map(|p| {
                                    let base_name = match &p.name.kind {
                                        PatKind::Ident(n) => n.to_string(),
                                        _ => "_".to_string(),
                                    };
                                    // Encode optional/rest in the name for display
                                    // and arity (`?name`, `...name`).
                                    let pname = if p.dotdotdot {
                                        format!("...{}", base_name)
                                    } else if p.optional || p.initializer.is_some() {
                                        format!("?{}", base_name)
                                    } else {
                                        base_name
                                    };
                                    let pty = p
                                        .type_ann
                                        .as_ref()
                                        .map(|t| self.resolve_type_node(t))
                                        .unwrap_or(Type::Any);
                                    let pty = self.declared_optional_param_type(p, pty);
                                    self.declare_pattern_vars(&p.name, pty.clone());
                                    self.completion_mark_parameter(p);
                                    self.mark_optional_parameter(p);
                                    (pname, pty)
                                })
                                .collect();
                            let declared_ret =
                                m.return_type.as_ref().map(|t| self.resolve_type_node(t));
                            let expected_ret =
                                self.unwrap_async_return(declared_ret.clone(), m.is_async);
                            self.return_type_stack.push(expected_ret);
                            self.return_is_async_stack.push(m.is_async);
                            self.generator_stack.push(m.is_generator);
                            let ret = declared_ret.unwrap_or(Type::Void);
                            let tp = m
                                .type_params
                                .as_ref()
                                .map(|tps| tps.iter().map(|tp| tp.name.clone()).collect())
                                .unwrap_or_default();
                            self.hoist_block_declarations(&m.body);
                            for s in &m.body {
                                self.check_stmt(s);
                            }
                            self.check_function_completion(
                                &m.body,
                                m.return_type.as_ref(),
                                m.name.span(),
                                m.is_async,
                                m.is_generator,
                            );
                            self.return_is_async_stack.pop();
                            self.generator_stack.pop();
                            self.return_type_stack.pop();
                            self.fn_nesting_depth -= 1;
                            self.jump_function_depth -= 1;
                            self.var_first_types = saved_var_first_types;
                            self.pop_active_type_param_names(pushed_type_params);
                            self.pop_scope();
                            properties.push((
                                name,
                                Arc::new(Type::Function(FunctionType {
                                    type_param_constraints: Vec::new(),
                                    params,
                                    return_type: Arc::new(ret),
                                    type_params: tp,
                                    type_param_defaults: Vec::new(),
                                    type_predicate: None,
                                })),
                            ));
                        }
                        ObjLitProp::Get(acc) => {
                            self.check_getter_missing_return(&acc.name, &acc.body);
                            self.check_accessor_signature_grammar(
                                true,
                                &acc.name,
                                &acc.params,
                                acc.return_type.as_ref(),
                                false,
                            );
                            let name = self.prop_name_to_string(&acc.name);
                            self.push_scope();
                            self.check_function_like_future_lib_globals(
                                &acc.params,
                                acc.return_type.as_ref(),
                            );
                            let saved_var_first_types = std::mem::take(&mut self.var_first_types);
                            self.declare_var("arguments", Type::Any);
                            self.declare_var(SUPER_PROPERTY_OK_MARKER, Type::Never);
                            self.declare_var(THIS_OK_MARKER, Type::Never);
                            self.bind_function_this(&[]);
                            for param in &acc.params {
                                self.declare_pattern_vars(&param.name, Type::Any);
                                self.completion_mark_parameter(param);
                            }
                            self.hoist_block_declarations(&acc.body);
                            self.jump_function_depth += 1;
                            for s in &acc.body {
                                self.check_stmt(s);
                            }
                            self.jump_function_depth -= 1;
                            self.check_function_completion(
                                &acc.body,
                                acc.return_type.as_ref(),
                                acc.name.span(),
                                false,
                                false,
                            );
                            self.var_first_types = saved_var_first_types;
                            self.pop_scope();
                            let getter_ty = acc
                                .return_type
                                .as_ref()
                                .map(|t| self.resolve_type_node(t))
                                .unwrap_or_else(|| self.infer_return_type_from_stmts(&acc.body));
                            properties.push((name, Arc::new(getter_ty)));
                        }
                        ObjLitProp::Set(acc) => {
                            self.check_accessor_signature_grammar(
                                false,
                                &acc.name,
                                &acc.params,
                                acc.return_type.as_ref(),
                                false,
                            );
                            self.check_setter_value_returns(&acc.body);
                            let name = self.prop_name_to_string(&acc.name);
                            self.push_scope();
                            self.check_function_like_future_lib_globals(
                                &acc.params,
                                acc.return_type.as_ref(),
                            );
                            let saved_var_first_types = std::mem::take(&mut self.var_first_types);
                            self.declare_var("arguments", Type::Any);
                            self.declare_var(SUPER_PROPERTY_OK_MARKER, Type::Never);
                            self.declare_var(THIS_OK_MARKER, Type::Never);
                            self.bind_function_this(&[]);
                            for param in &acc.params {
                                let pty = param
                                    .type_ann
                                    .as_ref()
                                    .map(|t| self.resolve_type_node(t))
                                    .unwrap_or(Type::Any);
                                self.declare_pattern_vars(&param.name, pty);
                                self.completion_mark_parameter(param);
                            }
                            self.hoist_block_declarations(&acc.body);
                            self.jump_function_depth += 1;
                            for s in &acc.body {
                                self.check_stmt(s);
                            }
                            self.jump_function_depth -= 1;
                            self.var_first_types = saved_var_first_types;
                            self.pop_scope();
                            properties.push((name, Arc::new(Type::Any)));
                        }
                    }
                }
                // Spreading `any` makes the whole literal `any` (tsc's
                // getSpreadType) — better than the `__spread__` tombstone,
                // which leaked a synthetic property name into diagnostics.
                if any_spread_plain {
                    return Type::Any;
                }
                Type::ObjectType(ObjectTypeInfo {
                    properties: Self::dedup_object_props_last_wins(properties),
                    call_signatures: Vec::new(),
                    construct_signatures: Vec::new(),
                    index_signature: None,
                    index_signature_name: None,
                    method_names: Vec::new(),
                })
            }
            ExprKind::Arrow(arrow) => {
                self.push_scope();
                self.fn_nesting_depth += 1;
                self.jump_function_depth += 1;
                self.arrow_nesting_depth += 1;
                let pushed_type_params =
                    self.push_active_type_param_names(arrow.type_params.as_deref());
                self.check_active_type_param_declarations(arrow.type_params.as_deref());
                self.check_function_like_future_lib_globals(
                    &arrow.params,
                    arrow.return_type.as_ref(),
                );
                let saved_var_first_types2 = std::mem::take(&mut self.var_first_types);
                self.check_arrow_parameter_decorators(&arrow.params);
                self.check_parameter_runtime_expressions(&arrow.params, true, false);
                // TS2369: check for parameter properties in arrow functions
                for p in &arrow.params {
                    self.check_parameter_property(p);
                }
                let params: Vec<(std::string::String, Type)> = arrow
                    .params
                    .iter()
                    .map(|p| {
                        let base_name = match &p.name.kind {
                            PatKind::Ident(n) => n.to_string(),
                            _ => "_".to_string(),
                        };
                        // Match the encoding used by every other arrow /
                        // function type producer (`resolve_fn_type`,
                        // `infer_expr_type::Arrow`, `check_arrow_or_fn_contextual`):
                        // `...name` for rest, `?name` for optional.
                        // Required so `has_rest` detection at call sites
                        // recognizes `(...args: any[]) => …` and treats
                        // each variadic argument against the element type.
                        let pname = if p.dotdotdot {
                            format!("...{}", base_name)
                        } else if p.optional || p.initializer.is_some() {
                            format!("?{}", base_name)
                        } else {
                            base_name
                        };
                        let pty = p
                            .type_ann
                            .as_ref()
                            .map(|t| self.resolve_type_node(t))
                            .unwrap_or(Type::Any);
                        self.record_pattern_types(&p.name, &pty);
                        self.declare_pattern_vars(&p.name, pty.clone());
                        self.completion_mark_parameter(p);
                        self.mark_optional_parameter(p);
                        (pname, pty)
                    })
                    .collect();
                // Push declared return type for TS2322 checking in return statements
                let declared_ret_main = arrow
                    .return_type
                    .as_ref()
                    .map(|rt| self.resolve_type_node(rt));
                let declared_ret_main = self.unwrap_async_return(declared_ret_main, arrow.is_async);
                self.return_type_stack.push(declared_ret_main);
                self.return_is_async_stack.push(arrow.is_async);
                self.generator_stack.push(false);
                let ret_type = match &arrow.body {
                    ArrowBody::Expr(e) => {
                        let inferred = self.check_expr(e);
                        let inferred = self.widen_fresh_return_expr_type(e, inferred);
                        // Without strictNullChecks a nullish return widens to
                        // `any` (tsc getWidenedType), whatever the contextual
                        // return type.
                        let inferred = if !self.strict_null_checks
                            && matches!(inferred, Type::Null | Type::Undefined)
                        {
                            Type::Any
                        } else {
                            inferred
                        };
                        // TS2322: check arrow expression body against declared return type
                        if let Some(Some(expected_ret)) = self.return_type_stack.last().cloned() {
                            if !self.is_assignable_to(&inferred, &expected_ret)
                                && !matches!(expected_ret, Type::Any | Type::Error)
                                && !matches!(inferred, Type::Any | Type::Error)
                            {
                                let widened =
                                    self.widen_for_message(Some(e), &inferred, &expected_ret);
                                let return_span = self.error_span_for_expr(e);
                                self.push_not_assignable(&widened, &expected_ret, return_span);
                            }
                        }
                        match arrow.return_type.as_ref() {
                            Some(t) => self.resolve_type_node(t),
                            // Async concise-body arrow (`async () => 42`)
                            // returns `Promise<42>`, not `42`.
                            None if arrow.is_async => Self::wrap_async_inferred_return(inferred),
                            None => inferred,
                        }
                    }
                    ArrowBody::Block(stmts) => {
                        self.hoist_block_declarations(stmts);
                        for s in stmts {
                            self.check_stmt(s);
                        }
                        self.check_function_completion(
                            stmts,
                            arrow.return_type.as_ref(),
                            self.error_span_for_expr(expr),
                            arrow.is_async,
                            false,
                        );
                        match arrow.return_type.as_ref() {
                            Some(t) => self.resolve_type_node(t),
                            None => {
                                let inferred = self.infer_return_type_from_block(stmts);
                                // An async arrow always returns a Promise, so
                                // an INFERRED body-completion type must be
                                // wrapped — otherwise `async () => { await x; }`
                                // types as `() => void` and fails assignment to
                                // `() => Promise<void>`.
                                if arrow.is_async {
                                    Self::wrap_async_inferred_return(inferred)
                                } else {
                                    inferred
                                }
                            }
                        }
                    }
                };
                self.return_type_stack.pop();
                self.return_is_async_stack.pop();
                self.generator_stack.pop();
                let arrow_tp = arrow
                    .return_type
                    .as_ref()
                    .and_then(|rt| self.extract_type_predicate(rt));
                self.fn_nesting_depth -= 1;
                self.jump_function_depth -= 1;
                self.arrow_nesting_depth -= 1;
                self.var_first_types = saved_var_first_types2;
                self.pop_active_type_param_names(pushed_type_params);
                self.pop_scope();
                Type::Function(FunctionType {
                    type_param_constraints: self
                        .resolve_type_param_constraints(arrow.type_params.as_deref()),
                    params,
                    return_type: Arc::new(ret_type),
                    type_params: arrow
                        .type_params
                        .as_ref()
                        .map(|tps| tps.iter().map(|tp| tp.name.clone()).collect())
                        .unwrap_or_default(),
                    type_param_defaults: self
                        .resolve_type_param_defaults(arrow.type_params.as_deref()),
                    type_predicate: arrow_tp,
                })
            }
            ExprKind::FnExpr(fn_decl) => {
                self.push_scope();
                self.fn_nesting_depth += 1;
                self.jump_function_depth += 1;
                let pushed_type_params =
                    self.push_active_type_param_names(fn_decl.type_params.as_deref());
                self.check_active_type_param_declarations(fn_decl.type_params.as_deref());
                self.check_function_like_future_lib_globals(
                    &fn_decl.params,
                    fn_decl.return_type.as_ref(),
                );
                let saved_var_first_types2 = std::mem::take(&mut self.var_first_types);
                self.check_decorators_with_validity(&fn_decl.decorators, false);
                self.check_parameter_decorators(&fn_decl.params, false);
                self.check_parameter_runtime_expressions(
                    &fn_decl.params,
                    fn_decl.body.is_some(),
                    false,
                );
                // `arguments` is implicitly available inside all non-arrow functions
                self.declare_var("arguments", Type::Any);
                self.declare_var(SUPER_PROPERTY_BARRIER_MARKER, Type::Never);
                self.declare_var(THIS_OK_MARKER, Type::Never);
                self.bind_function_this(&fn_decl.params);
                // Named function expression: name is available inside the body
                if let Some(ref name) = fn_decl.name {
                    self.declare_var(name, Type::Any);
                }
                // TS2369: check for parameter properties in function expressions
                for p in &fn_decl.params {
                    self.check_parameter_property(p);
                }
                let params: Vec<(std::string::String, Type)> = fn_decl
                    .params
                    .iter()
                    .map(|p| {
                        let base_name = match &p.name.kind {
                            PatKind::Ident(n) => n.to_string(),
                            _ => "_".to_string(),
                        };
                        // See the Arrow case above for the rationale.
                        let pname = if p.dotdotdot {
                            format!("...{}", base_name)
                        } else if p.optional || p.initializer.is_some() {
                            format!("?{}", base_name)
                        } else {
                            base_name
                        };
                        let pty = p
                            .type_ann
                            .as_ref()
                            .map(|t| self.resolve_type_node(t))
                            .unwrap_or(Type::Any);
                        self.record_pattern_types(&p.name, &pty);
                        self.declare_pattern_vars(&p.name, pty.clone());
                        self.completion_mark_parameter(p);
                        self.mark_optional_parameter(p);
                        (pname, pty)
                    })
                    .collect();
                self.generator_stack.push(fn_decl.is_generator);
                if let Some(ref body) = fn_decl.body {
                    self.hoist_block_declarations(body);
                    for s in body {
                        self.check_stmt(s);
                    }
                    self.check_function_completion(
                        body,
                        fn_decl.return_type.as_ref(),
                        self.error_span_for_expr(expr),
                        fn_decl.is_async,
                        fn_decl.is_generator,
                    );
                }
                self.generator_stack.pop();
                let ret = if let Some(ref rt) = fn_decl.return_type {
                    self.resolve_type_node(rt)
                } else if let Some(ref body) = fn_decl.body {
                    self.infer_return_type_from_block(body)
                } else {
                    Type::Any
                };
                let fn_tp = fn_decl
                    .return_type
                    .as_ref()
                    .and_then(|rt| self.extract_type_predicate(rt));
                self.fn_nesting_depth -= 1;
                self.jump_function_depth -= 1;
                self.var_first_types = saved_var_first_types2;
                self.pop_active_type_param_names(pushed_type_params);
                self.pop_scope();
                Type::Function(FunctionType {
                    type_param_constraints: self
                        .resolve_type_param_constraints(fn_decl.type_params.as_deref()),
                    params,
                    return_type: Arc::new(ret),
                    type_params: fn_decl
                        .type_params
                        .as_ref()
                        .map(|tps| tps.iter().map(|tp| tp.name.clone()).collect())
                        .unwrap_or_default(),
                    type_param_defaults: self
                        .resolve_type_param_defaults(fn_decl.type_params.as_deref()),
                    type_predicate: fn_tp,
                })
            }
            ExprKind::ClassExpr(class_decl) => {
                if let Some(name) = &class_decl.name {
                    self.check_reserved_type_name(name, class_decl.name_span, 2414, "Class");
                }
                let pushed_type_params =
                    self.push_active_type_param_names(class_decl.type_params.as_deref());
                self.check_active_type_param_declarations(class_decl.type_params.as_deref());
                if let Some(arguments) = &class_decl.extends_type_args {
                    for argument in arguments {
                        self.check_type_node_future_lib_globals(argument);
                    }
                }
                if let Some(extends) = &class_decl.extends {
                    self.check_base_constructor_expression(extends);
                }
                let legacy_decorators = self.compiler_options.experimental_decorators == Some(true);
                self.check_decorators_with_validity(&class_decl.decorators, !legacy_decorators);
                // Give every class expression a stable internal class identity,
                // including anonymous expressions. This preserves private
                // instance/static nominal origins across construction and
                // assignment; a structural ObjectType would erase them.
                let identity = self
                    .pending_class_expression_name
                    .take()
                    .unwrap_or_else(|| format!("__class_expression_{}", expr.span.start));
                let class_name = class_decl.name.clone();
                if let Some(ref name) = class_name {
                    // A named class expression's source name is local to its body.
                    // Bind it to the synthetic identity before resolving member
                    // annotations so an outer same-named class cannot capture
                    // self-references such as `make(): C`.
                    self.push_scope();
                    self.declare_var(
                        name,
                        Type::TypeReference(
                            format!("typeof {identity}"),
                            Arc::from([] as [Type; 0]),
                        ),
                    );
                }
                self.enclosing_class_names.push(identity.clone());
                self.enclosing_class_derived
                    .push(class_decl.extends.is_some());
                let info = self.build_class_info(class_decl);
                self.insert_class_info(identity.clone(), info);
                let class_type_param_frame = pushed_type_params
                    .then(|| self.active_type_param_names.len().saturating_sub(1));
                for member in &class_decl.members {
                    let pushed_static_context =
                        self.push_static_member_context(member, class_type_param_frame);
                    let member_type_params = match &member.kind {
                        ClassMemberKind::Method(method) => method.type_params.as_deref(),
                        _ => None,
                    };
                    let pushed_member_type_params =
                        self.push_active_type_param_names(member_type_params);
                    self.check_active_type_param_declarations(member_type_params);
                    self.check_class_member_with_siblings(member, &class_decl.members, true);
                    self.check_class_member_future_lib_globals(member);
                    self.pop_active_type_param_names(pushed_member_type_params);
                    if pushed_static_context {
                        self.static_member_contexts.pop();
                    }
                }
                self.check_class_overload_compatibility(&class_decl.members);
                self.check_static_extends(class_decl, &identity);
                self.check_index_signature_members(class_decl);
                if self.ambient_depth == 0 && !self.current_file_is_declaration() {
                    self.check_strict_property_initialization(class_decl);
                }
                self.enclosing_class_names.pop();
                self.enclosing_class_derived.pop();
                if class_name.is_some() {
                    self.pop_scope();
                }
                self.pop_active_type_param_names(pushed_type_params);
                Type::TypeReference(format!("typeof {identity}"), Arc::from([] as [Type; 0]))
            }
            ExprKind::Paren(inner) => self.check_expr(inner),

            ExprKind::As(a) => {
                // Detect `as const` assertion: the parser encodes it as a
                // TypeNodeKind::Reference with name "const".
                let is_const_assertion = matches!(&a.type_node.kind,
                    TypeNodeKind::Reference(type_ref) if matches!(&type_ref.name.kind,
                        ExprKind::Ident(n) if n == "const"
                    )
                );
                if is_const_assertion {
                    // `as const` freezes literal types: string/number/boolean
                    // literals stay narrow, arrays become readonly tuples.
                    self.check_expr(&a.expr);
                    self.infer_const_asserted_expr(&a.expr)
                } else {
                    if !self.static_member_contexts.is_empty() {
                        self.check_type_node_generic_arity(&a.type_node);
                    }
                    let target = self.resolve_type_node(&a.type_node);
                    let source_ty = if matches!(
                        a.expr.kind,
                        ExprKind::Arrow(_) | ExprKind::FnExpr(_) | ExprKind::Paren(_)
                    ) {
                        // A parenthesized function operand (`x as ((a: number) => void)`
                        // or `<T>(fn)`) must still receive the asserted type as its
                        // contextual type; check_expr_contextual recurses through Paren.
                        self.check_expr_contextual(&a.expr, Some(&target))
                    } else {
                        self.check_expr(&a.expr)
                    };
                    self.check_assertion_comparability(&source_ty, &target, expr.span);
                    target
                }
            }
            ExprKind::Satisfies(s) => {
                if !self.static_member_contexts.is_empty() {
                    self.check_type_node_generic_arity(&s.type_node);
                }
                let target_type = self.resolve_type_node(&s.type_node);
                let expr_type = self.check_expr_contextual(&s.expr, Some(&target_type));
                if !self.is_assignable_to(&expr_type, &target_type) {
                    self.diagnostics.push(Diagnostic {
                        code: 1360,
                        message: format!(
                            "Type '{}' does not satisfy the expected type '{}'.",
                            expr_type.display_string(),
                            target_type.display_string()
                        ),
                        category: DiagnosticCategory::Error,
                        file_name: None,
                        span: Some(expr.span),
                        related: None,
                    });
                }
                if Self::is_fresh_object_literal(&s.expr) {
                    self.check_excess_properties(
                        &expr_type,
                        &target_type,
                        expr.span,
                        Some(&s.expr),
                    );
                }
                expr_type
            }
            ExprKind::NonNull(inner) => {
                let ty = self.check_expr(inner);
                self.remove_null_undefined(&ty)
            }
            ExprKind::TypeAssertion(ta) => {
                if Self::is_const_assertion_expr(expr) {
                    self.check_expr(&ta.expr);
                    return self.infer_const_asserted_expr(&ta.expr);
                }
                let diagnostic_count = self.diagnostics.len();
                self.check_type_node_generic_arity(&ta.type_node);
                let target_has_type_error = self.diagnostics.len() != diagnostic_count;
                let target = self.resolve_type_node(&ta.type_node);
                // Pass the asserted type as contextual type for function
                // expressions, including parenthesized ones (`<T>(fn)`);
                // check_expr_contextual recurses through Paren.
                let source_ty = if matches!(
                    ta.expr.kind,
                    ExprKind::Arrow(_) | ExprKind::FnExpr(_) | ExprKind::Paren(_)
                ) {
                    self.check_expr_contextual(&ta.expr, Some(&target))
                } else {
                    self.check_expr(&ta.expr)
                };
                if !target_has_type_error {
                    self.check_assertion_comparability(&source_ty, &target, expr.span);
                }
                target
            }
            ExprKind::Instantiation(inst) => self.check_expr(&inst.expr),
            ExprKind::Spread(inner) => {
                let ty = self.check_expr(inner);
                // TS2802: below ES2015 without --downlevelIteration, spreading
                // a non-array iterable (Iterable/Set/Map/Generator, or a
                // string) cannot be downleveled.
                let target_lt_es2015 = !matches!(
                    self.compiler_options.target,
                    Some(t) if t >= tsc_rs_ast::ScriptTarget::ES2015
                );
                if target_lt_es2015 && self.compiler_options.down_level_iteration != Some(true) {
                    let iterable_only = match &ty {
                        Type::String | Type::StringLiteral(_) => true,
                        Type::TypeReference(name, _) => matches!(
                            name.as_str(),
                            "Iterable"
                                | "IterableIterator"
                                | "Generator"
                                | "Set"
                                | "ReadonlySet"
                                | "Map"
                                | "ReadonlyMap"
                        ),
                        _ => false,
                    };
                    if iterable_only {
                        self.diagnostics.push(Diagnostic {
                            code: 2802,
                            message: format!(
                                "Type '{}' can only be iterated through when using the '--downlevelIteration' flag or with a '--target' of 'es2015' or higher.",
                                ty.display_string()
                            ),
                            category: DiagnosticCategory::Error,
                            file_name: None,
                            span: Some(inner.span),
                            related: None,
                        });
                    }
                }
                // Unwrap iterable element type for spread. `...arr` produces
                // the array element; `...new Set(arr)` produces the set's
                // element; `...mapInstance` produces `[K, V]` tuples. Without
                // this, `[...new Set(strings)]` infers as `Set<string>[]`
                // instead of `string[]`, which propagates as bogus "Set[]"
                // diagnostics far from the spread site (e.g. `validEmails:
                // Set[]` in apps/app/src/utils/emailSuppressionFilter.ts).
                match &ty {
                    Type::Array(elem) => Type::clone(elem),
                    Type::Tuple(elems) if !elems.is_empty() => {
                        Type::flatten_union(elems.iter().cloned().collect())
                    }
                    Type::TypeReference(name, args) => match (name.as_str(), args.as_ref()) {
                        (
                            "Set"
                            | "ReadonlySet"
                            | "Iterable"
                            | "IterableIterator"
                            | "Generator"
                            | "AsyncIterable"
                            | "AsyncIterableIterator"
                            | "AsyncGenerator"
                            | "ReadonlyArray"
                            | "Array",
                            [elem, ..],
                        ) => elem.clone(),
                        ("Map" | "ReadonlyMap" | "WeakMap", [k, v, ..]) => {
                            Type::Tuple(Arc::from([k.clone(), v.clone()]))
                        }
                        // Same iterable with NO resolved type argument — e.g.
                        // `new Set(xs.map(f).filter(Boolean))`, where the
                        // element type could not be inferred from the
                        // constructor argument. The element is unknown, but it
                        // is definitely NOT the collection itself: yielding the
                        // collection made `[...new Set(xs)] as string[]` compare
                        // element `Set` against `string` and report a spurious
                        // TS2352. `any` keeps the spread lenient instead.
                        (
                            "Set"
                            | "ReadonlySet"
                            | "Iterable"
                            | "IterableIterator"
                            | "Generator"
                            | "AsyncIterable"
                            | "AsyncIterableIterator"
                            | "AsyncGenerator"
                            | "ReadonlyArray"
                            | "Array"
                            | "Map"
                            | "ReadonlyMap"
                            | "WeakMap",
                            [],
                        ) => Type::Any,
                        _ => ty.clone(),
                    },
                    Type::String | Type::StringLiteral(_) => Type::String,
                    _ => ty,
                }
            }
            ExprKind::Await(inner) => {
                // TS1308: `await` inside a non-async function-like. The
                // innermost entry of return_is_async_stack is the enclosing
                // function's async-ness (constructors/accessors/initializers
                // push `false`); an empty stack is top level — allowed.
                if self.return_is_async_stack.last() == Some(&false) {
                    // tsc marks only the `await` keyword.
                    self.diagnostics.push(error_await_in_non_async(Span::new(
                        expr.span.start,
                        expr.span.start + 5,
                    )));
                }
                let ty = self.check_expr(inner);
                // Unwrap Promise<T> -> T
                match ty {
                    Type::TypeReference(ref name, ref args)
                        if name == "Promise" && args.len() == 1 =>
                    {
                        args[0].clone()
                    }
                    _ => ty,
                }
            }
            ExprKind::Yield(delegate, inner) => {
                // tsc checkYieldExpression: outside a generator body the
                // expression is `any` and its operand is not checked.
                let in_generator = self.generator_stack.last().copied().unwrap_or(false);
                if !in_generator {
                    // TS1163 (checkYieldExpressionGrammar).
                    self.diagnostics.push(Diagnostic {
                        code: 1163,
                        message: "A 'yield' expression is only allowed in a generator body."
                            .to_string(),
                        category: DiagnosticCategory::Error,
                        file_name: None,
                        // grammarErrorOnFirstToken: only the `yield` keyword.
                        span: Some(Span {
                            start: expr.span.start,
                            end: expr.span.start + 5,
                        }),
                        related: None,
                    });
                }
                if let Some(ref e) = inner {
                    if in_generator {
                        let operand = self.check_expr(e);
                        let in_async = self.return_is_async_stack.last().copied().unwrap_or(false);
                        if *delegate && !in_async {
                            if self.iteration_uses_protocol() {
                                self.report_not_iterable(&operand, e.span, false);
                            }
                        }
                    }
                }
                Type::Any
            }
            ExprKind::Typeof(inner) => {
                self.check_expr(inner);
                Self::typeof_result_type()
            }
            ExprKind::Void(inner) => {
                self.check_expr(inner);
                Type::Undefined
            }
            ExprKind::Delete(inner) => {
                self.check_delete_operand(inner);
                self.check_expr(inner);
                Type::Boolean
            }
            ExprKind::Comma(exprs) => {
                // TS2695: every operand but the last has its value discarded,
                // so one with no side effects is dead code. tsc skips the
                // check entirely under `allowUnreachableCode: true`.
                let unreachable_allowed = self.allow_unreachable_code == Some(true);
                // tsc's isSideEffectFree.
                fn has_side_effects(e: &Expr) -> bool {
                    match &e.kind {
                        ExprKind::Ident(_)
                        | ExprKind::StrLit(_)
                        | ExprKind::RegexpLit(_)
                        | ExprKind::TaggedTemplate(_)
                        | ExprKind::Template(_)
                        | ExprKind::NoSubstTemplate(_)
                        | ExprKind::NumLit(_)
                        | ExprKind::BigIntLit(_)
                        | ExprKind::BoolLit(_)
                        | ExprKind::NullLit
                        | ExprKind::FnExpr(_)
                        | ExprKind::ClassExpr(_)
                        | ExprKind::Arrow(_)
                        | ExprKind::ArrayLit(_)
                        | ExprKind::ObjectLit(_)
                        | ExprKind::Typeof(_)
                        | ExprKind::NonNull(_)
                        | ExprKind::JsxSelfClosing(_)
                        | ExprKind::JsxElement(_) => false,
                        ExprKind::Paren(inner) => has_side_effects(inner),
                        ExprKind::Cond(c) => {
                            has_side_effects(&c.consequent) || has_side_effects(&c.alternate)
                        }
                        ExprKind::Binary(b) => {
                            has_side_effects(&b.left) || has_side_effects(&b.right)
                        }
                        ExprKind::Unary(u) => !matches!(
                            u.op,
                            UnaryOp::LogNot
                                | UnaryOp::Pos
                                | UnaryOp::Neg
                                | UnaryOp::BitNot
                                | UnaryOp::Typeof
                        ),
                        _ => true,
                    }
                }
                // Parser-recovery ASTs are not tsc's; skip files with
                // syntax errors.
                // Sibling JSX roots are recovered as a comma (tsc: TS2657).
                let jsx_siblings = exprs.iter().any(|e| {
                    matches!(
                        e.kind,
                        ExprKind::JsxElement(_)
                            | ExprKind::JsxSelfClosing(_)
                            | ExprKind::JsxFragment(_)
                    )
                });
                // tsc reports TS2695 even in files with parse errors; only
                // skip a comma our parser recovery built — one with a parse
                // error inside it.
                let recovered = exprs
                    .first()
                    .zip(exprs.last())
                    .is_some_and(|(first, last)| {
                        let idx = self
                            .syntax_error_starts
                            .partition_point(|&d| d < first.span.start);
                        self.syntax_error_starts
                            .get(idx)
                            .is_some_and(|&d| d <= last.span.end)
                    });
                let exempt =
                    self.in_callee_position || unreachable_allowed || jsx_siblings || recovered;
                let mut last = Type::Any;
                let mut chain_free = true;
                for (i, e) in exprs.iter().enumerate() {
                    // `a, b, c` is `((a, b), c)`: the left of each comma is
                    // the whole chain so far, side-effect free only if every
                    // operand in it is.
                    chain_free &= !has_side_effects(e);
                    if !exempt && i + 1 < exprs.len() && chain_free {
                        self.diagnostics
                            .push(crate::diagnostics::error_comma_left_unused(Span::new(
                                exprs[0].span.start,
                                e.span.end,
                            )));
                    }
                    last = self.check_expr(e);
                }
                last
            }
            ExprKind::RegexpLit(regex) => {
                if self.check_expression_grammar {
                    self.check_regexp_escape_diagnostics(regex.pattern.as_str(), expr.span);
                }
                Type::TypeReference("RegExp".to_string(), Arc::from([] as [Type; 0]))
            }
            ExprKind::MetaProp(mp) => {
                match (mp.meta.as_str(), mp.property.as_str()) {
                    // import.meta returns an object type with standard + runtime properties
                    ("import", "meta") => {
                        let mut meta = self.import_meta_builtin_shape();
                        // Program `ImportMeta` declarations (global
                        // augmentations) add members to `import.meta`.
                        if let (Type::ObjectType(shape), Some(info)) =
                            (&mut meta, self.interface_info.get("ImportMeta"))
                        {
                            for (name, ty) in &info.object_type.properties {
                                if !shape.properties.iter().any(|(n, _)| n == name) {
                                    shape.properties.push((name.clone(), ty.clone()));
                                }
                            }
                        }
                        meta
                    }
                    // new.target returns a function or undefined
                    ("new", "target") => Type::Any,
                    _ => Type::Any,
                }
            }
            ExprKind::Omitted => Type::Undefined,
            // JSX expressions produce JSX.Element (React.ReactElement or equivalent).
            //
            // We don't yet do full prop-type binding (component-fn signature
            // vs attrs object), but we DO walk the sub-expressions:
            //   - the opening / closing tag identifier (resolves it as a
            //     value reference so TS2304 fires on undefined components)
            //   - every attribute value expression
            //   - every child expression `{ ... }` and nested JSX child
            // Without this, an `undefinedVar` inside `<Foo prop={undefinedVar} />`
            // silently passed type checking even though real `tsc` flags it.
            ExprKind::JsxElement(el) => {
                self.check_jsx_factory_in_scope(&el.name);
                Self::check_jsx_node(self, &el.name, &el.attributes, el.opening_span);
                if let Some(closing) = &el.closing {
                    Self::check_jsx_node(self, &closing.name, &[], closing.span);
                }
                for child in &el.children {
                    Self::check_jsx_child(self, child);
                }
                Type::TypeReference("JSX.Element".to_string(), Arc::from([] as [Type; 0]))
            }
            ExprKind::JsxSelfClosing(el) => {
                self.check_jsx_factory_in_scope(&el.name);
                Self::check_jsx_node(self, &el.name, &el.attributes, expr.span);
                Type::TypeReference("JSX.Element".to_string(), Arc::from([] as [Type; 0]))
            }
            ExprKind::JsxFragment(frag) => {
                for child in &frag.children {
                    Self::check_jsx_child(self, child);
                }
                Type::TypeReference("JSX.Element".to_string(), Arc::from([] as [Type; 0]))
            }
        };

        // Record type at this expression's position.
        // For arrow/function expressions, use or_insert to avoid overwriting
        // parameter types recorded at the same position (e.g., `a => 10`).
        if matches!(expr.kind, ExprKind::Arrow(_) | ExprKind::FnExpr(_)) {
            self.record_expr_type_if_absent(expr.span.start, &ty);
        } else {
            self.record_expr_type(expr.span.start, &ty);
        }

        // For member expressions, also record the type at the property name position
        // so hover on `.property` resolves correctly.
        if let ExprKind::Member(mem) = &expr.kind {
            let prop_len = mem.property.len() as u32;
            if expr.span.end > prop_len {
                let prop_start = expr.span.end - prop_len;
                self.record_expr_type(prop_start, &ty);
            }
        }

        ty
    }

    /// Check a function call against a single FunctionType (no overloads).
    /// Walk a JSX tag name and attributes so contained expressions are
    /// type-checked. The element dispatcher visits closing tags and children.
    /// We don't bind attrs to
    /// the component's prop type yet — that's a separate, larger fix —
    /// but at minimum we type-check what's INSIDE the JSX (undefined
    /// identifiers, wrong arguments inside attribute callbacks, etc.).
    pub(crate) fn check_jsx_node(
        &mut self,
        tag: &Expr,
        attrs: &[tsc_rs_ast::JsxAttribute],
        intrinsic_span: Span,
    ) {
        if self.check_expression_grammar {
            // TypeScript reports the first duplicate/empty attribute grammar
            // error per opening element, then still checks all expressions.
            let mut seen = rustc_hash::FxHashSet::default();
            for attr in attrs {
                let tsc_rs_ast::JsxAttribute::Normal { name, value, span } = attr else {
                    continue;
                };
                let error =
                    if !seen.insert(name.as_str()) {
                        let mut name_span = Span::new(span.start, span.start + name.len() as u32);
                        // A namespaced name can contain trivia around its colon.
                        // Only rescan the duplicate being diagnosed, so its range
                        // includes that trivia and excludes the initializer.
                        if name.contains(':') {
                            if let Some(text) = self.current_source.as_deref().and_then(|source| {
                                source.get(span.start as usize..span.end as usize)
                            }) {
                                let mut scanner = tsc_rs_scanner::TsScanner::new(text);
                                scanner.scan();
                                scanner.scan_jsx_identifier();
                                if scanner.scan() == tsc_rs_scanner::TokenKind::Colon {
                                    scanner.scan();
                                    scanner.scan_jsx_identifier();
                                    name_span.end = span.start + scanner.text_pos() as u32;
                                }
                            }
                        }
                        Some((
                            17001,
                            "JSX elements cannot have multiple attributes with the same name.",
                            name_span,
                        ))
                    } else if let Some(value) = value
                        .as_ref()
                        .filter(|value| matches!(value.kind, ExprKind::Omitted))
                    {
                        Some((
                            17000,
                            "JSX attributes must only be assigned a non-empty 'expression'.",
                            value.span,
                        ))
                    } else {
                        None
                    };
                if let Some((code, message, span)) = error {
                    self.diagnostics.push(Diagnostic {
                        code,
                        message: message.into(),
                        category: DiagnosticCategory::Error,
                        file_name: None,
                        span: Some(span),
                        related: None,
                    });
                    break;
                }
            }
        }
        // Keep source spellings in the AST for emission, but resolve escaped
        // names using their semantic identifier value. JSX reports TS17021 for
        // the escape independently of normal name/expression checking.
        fn decoded_tag(tag: &Expr) -> Option<Expr> {
            fn decoded_name(name: &str) -> Option<String> {
                if !name.contains('\\') {
                    return None;
                }
                let mut decoded = String::new();
                for (index, part) in name.split(':').enumerate() {
                    if index != 0 {
                        decoded.push(':');
                    }
                    let mut scanner = tsc_rs_scanner::TsScanner::new(part);
                    scanner.scan();
                    scanner.scan_jsx_identifier();
                    decoded.push_str(scanner.token_value());
                }
                Some(decoded)
            }
            let kind = match &tag.kind {
                ExprKind::Ident(name) => ExprKind::Ident(decoded_name(name)?.into()),
                ExprKind::Member(member) => {
                    let object = decoded_tag(&member.object);
                    let property = decoded_name(&member.property);
                    if object.is_none() && property.is_none() {
                        return None;
                    }
                    let mut member = member.clone();
                    if let Some(object) = object {
                        member.object = Box::new(object);
                    }
                    if let Some(property) = property {
                        member.property = property.into();
                    }
                    ExprKind::Member(member)
                }
                _ => return None,
            };
            Some(Expr {
                kind,
                span: tag.span,
            })
        }
        fn restore_tag_spelling(
            checker: &mut TypeChecker,
            original: &Expr,
            decoded: &Expr,
            diagnostic_start: usize,
        ) {
            let (raw, cooked, raw_span, lookup_span, property) =
                match (&original.kind, &decoded.kind) {
                    (ExprKind::Ident(raw), ExprKind::Ident(cooked)) => {
                        (raw, cooked, original.span, decoded.span, false)
                    }
                    (ExprKind::Member(raw), ExprKind::Member(cooked)) => {
                        restore_tag_spelling(
                            checker,
                            &raw.object,
                            &cooked.object,
                            diagnostic_start,
                        );
                        let raw_span = Span::new(
                            original.span.end.saturating_sub(raw.property.len() as u32),
                            original.span.end,
                        );
                        let lookup_span = Span::new(
                            decoded
                                .span
                                .end
                                .saturating_sub(cooked.property.len() as u32),
                            decoded.span.end,
                        );
                        (&raw.property, &cooked.property, raw_span, lookup_span, true)
                    }
                    _ => return,
                };
            if raw == cooked && !property {
                return;
            }
            let needle = format!("'{cooked}'");
            let spelling = format!("'{raw}'");
            for diagnostic in checker.diagnostics.iter_mut().skip(diagnostic_start) {
                let name_error = if property {
                    matches!(diagnostic.code, 2339 | 2551 | 2341 | 2445)
                } else {
                    matches!(diagnostic.code, 2304 | 2552 | 2693 | 2708)
                };
                if name_error
                    && matches!(diagnostic.span, Some(span) if span == original.span || span == lookup_span)
                    && diagnostic.message.contains(&needle)
                {
                    diagnostic.message = diagnostic.message.replacen(&needle, &spelling, 1);
                    diagnostic.span = Some(raw_span);
                }
            }
            // Member checking also records a hover type at the property's
            // start. Decoding changes its byte length; keep that entry at the
            // source token's actual start rather than inside its escape.
            if property && lookup_span.start != raw_span.start {
                if let Some(ty) = checker.expression_types.remove(&lookup_span.start) {
                    checker.expression_types.insert(raw_span.start, ty);
                }
            }
        }
        let original_tag = tag;
        let decoded = decoded_tag(tag);
        let tag = decoded.as_ref().unwrap_or(tag);
        // Lowercase, hyphenated, and namespaced names are intrinsic elements.
        // Member-access tags (`<Foo.Bar>`) always resolve as component values.
        let is_intrinsic = matches!(&tag.kind, ExprKind::Ident(name) if name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_lowercase())
            || name.contains('-') || name.contains(':'));

        // TS7026: an intrinsic element needs a `JSX.IntrinsicElements`
        // interface to type its tag. With no JSX typings in the program, the
        // element is implicitly `any` — an error under noImplicitAny.
        // A `/// <reference path=".../react.d.ts" />` (or any lib reference)
        // brings JSX typings we do not load — their IntrinsicElements is
        // present as far as tsc is concerned, so stay silent.
        let has_lib_reference = self.current_file_has_lib_reference;
        if is_intrinsic
            && self.no_implicit_any
            && !has_lib_reference
            && !self.has_jsx_intrinsic_elements
            && !self.interface_info.contains_key("IntrinsicElements")
            && !self.interface_info.contains_key("JSX.IntrinsicElements")
        {
            self.diagnostics
                .push(crate::diagnostics::error_jsx_no_intrinsic_elements(
                    intrinsic_span,
                ));
        }
        if !is_intrinsic {
            // Run through check_expr to surface TS2304 / TS2552 on missing
            // component names. Result is discarded — we don't bind it yet.
            let diagnostic_start = self.diagnostics.len();
            let _ = self.check_expr(tag);
            restore_tag_spelling(self, original_tag, tag, diagnostic_start);
        }
        for attr in attrs {
            match attr {
                tsc_rs_ast::JsxAttribute::Normal { value: Some(v), .. } => {
                    self.check_jsx_expression(v);
                }
                tsc_rs_ast::JsxAttribute::Normal { value: None, .. } => {}
                tsc_rs_ast::JsxAttribute::Spread(e, _) => {
                    let _ = self.check_expr(e);
                }
            }
        }
    }

    fn check_jsx_expression(&mut self, expr: &Expr) {
        if self.check_expression_grammar && matches!(expr.kind, ExprKind::Comma(_)) {
            self.diagnostics.push(Diagnostic {
                code: 18007,
                message: "JSX expressions may not use the comma operator. Did you mean to write an array?".into(),
                category: DiagnosticCategory::Error,
                file_name: None,
                span: Some(expr.span),
                related: None,
            });
        }
        let _ = self.check_expr(expr);
    }

    pub(crate) fn check_jsx_child(&mut self, child: &tsc_rs_ast::JsxChild) {
        match child {
            tsc_rs_ast::JsxChild::Expression(Some(e), _) => {
                self.check_jsx_expression(e);
            }
            tsc_rs_ast::JsxChild::Element(e) => {
                let _ = self.check_expr(e);
            }
            tsc_rs_ast::JsxChild::Fragment(frag) => {
                for c in &frag.children {
                    Self::check_jsx_child(self, c);
                }
            }
            _ => {}
        }
    }

    pub(crate) fn check_call_against_fn_type(
        &mut self,
        callee_ty: &Type,
        arg_types: &[Type],
        call: &CallExpr,
        expr: &Expr,
    ) -> Type {
        // Intersection-of-Functions is the encoding used for overloaded
        // callables: `function f(a: number): void; function f(a: string): void`
        // becomes `Function(num_sig) & Function(str_sig)`. Real zod ships
        // this shape via cross-file method declarations (`default(value)` +
        // `default(thunk)`). Run overload selection on the constituent
        // sigs and fall through to the chosen one if it picks the right
        // arity / arg-type pair; if no sig matches we surface "no overload
        // matches" with the count for the diagnostic.
        if let Type::Intersection(members) = callee_ty {
            let sigs: Vec<FunctionType> = members
                .iter()
                .filter_map(|m| match m {
                    Type::Function(ft) => Some(ft.clone()),
                    _ => None,
                })
                .collect();
            if !sigs.is_empty() {
                if let Some((_idx, ret_ty)) = self.select_overload_resolution(&sigs, arg_types) {
                    return ret_ty;
                }
                let diagnostic = self.no_overload_diagnostic(&sigs, arg_types, call, expr);
                self.diagnostics.push(diagnostic);
                return Type::Error;
            }
        }
        match callee_ty {
            Type::Function(ft) => {
                // If all params are `any`, treat as variadic (e.g., console.log)
                let all_params_any =
                    !ft.params.is_empty() && ft.params.iter().all(|(_, t)| matches!(t, Type::Any));

                // Resolve generic type parameters
                let (effective_params, effective_return) = if !ft.type_params.is_empty() {
                    // Build type param map from explicit args or inference
                    let mut type_param_map = if let Some(ref ta) = call.type_args {
                        // Explicit type arguments provided: identity<number>(42)
                        let mut map = HashMap::new();
                        for (tp_name, type_arg_node) in ft.type_params.iter().zip(ta.iter()) {
                            map.insert(tp_name.clone(), self.resolve_type_node(type_arg_node));
                        }
                        map
                    } else {
                        // Infer type arguments from call argument types.
                        // Widen literals: TypeScript infers `number` from arg `1`, not literal `1`.
                        let widened: Vec<Type> = arg_types
                            .iter()
                            .map(|t| self.widen_argument_for_inference(t))
                            .collect();
                        let mut map = self.infer_type_arguments_for_call(
                            &widened,
                            &ft.params,
                            &ft.type_params,
                        );
                        // tsc keeps literal candidates for a type parameter that
                        // appears at top level in the return type
                        // (`invoke(() => 1)` with `invoke<T>(f: () => T): T` → `1`).
                        // Likewise for one whose declared constraint is a
                        // primitive or literal type (tsc hasPrimitiveConstraint:
                        // `<T extends "foo">` keeps `"foo" | "bar"`).
                        let top_level: Vec<&std::string::String> = ft
                            .type_params
                            .iter()
                            .enumerate()
                            .filter(|(index, name)| {
                                Self::type_param_at_top_level(&ft.return_type, name.as_str())
                                    || ft
                                        .type_param_constraints
                                        .get(*index)
                                        .and_then(Option::as_ref)
                                        .is_some_and(Self::is_primitive_constraint)
                            })
                            .map(|(_, name)| name)
                            .collect();
                        if !top_level.is_empty() {
                            let raw = self.infer_type_arguments_for_call(
                                arg_types,
                                &ft.params,
                                &ft.type_params,
                            );
                            for name in top_level {
                                if let Some(candidate) = raw.get(name) {
                                    map.insert(name.clone(), candidate.clone());
                                }
                            }
                        }
                        // The call's contextual type infers what the
                        // arguments left unsolved (tsc: return-type priority).
                        if let Some(contextual) = self
                            .contextual_call_returns
                            .get(&expr.span.start)
                            .map(|contextual| match contextual {
                                Type::Optional(inner) => Type::clone(inner),
                                other => other.clone(),
                            })
                        {
                            let unsolved: Vec<std::string::String> = ft
                                .type_params
                                .iter()
                                .filter(|name| {
                                    !matches!(map.get(*name), Some(ty) if !matches!(ty, Type::Unknown))
                                })
                                .cloned()
                                .collect();
                            if !unsolved.is_empty() {
                                let from_return = self.infer_type_arguments_for_call(
                                    std::slice::from_ref(&contextual),
                                    &[("__return".to_string(), Type::clone(&ft.return_type))],
                                    &ft.type_params,
                                );
                                for name in unsolved {
                                    if let Some(candidate) = from_return.get(&name) {
                                        if !matches!(candidate, Type::Unknown) {
                                            map.insert(name, candidate.clone());
                                        }
                                    }
                                }
                            }
                        }
                        map
                    };
                    // Inference precedes defaults. Apply each declared default
                    // only when its parameter remains unsolved, substituting
                    // earlier solutions/defaults so `<D, R = D>` makes R use
                    // the inferred D.
                    Self::apply_type_param_defaults(ft, &mut type_param_map);

                    // Substitute type params in parameter and return types.
                    // A rest parameter typed by a BARE type parameter
                    // (`<T extends unknown[]>(...args: T)`) infers T as a
                    // tuple of ALL arguments — substituting the first
                    // argument's type and then checking later arguments
                    // against it fabricates TS2345s. Such slots are
                    // inference targets, not per-argument constraints.
                    let params: Vec<_> = ft
                        .params
                        .iter()
                        .map(|(n, t)| {
                            let bare_tp = matches!(t, Type::TypeParameter(_))
                                || matches!(t, Type::TypeReference(p, a)
                                    if a.is_empty() && ft.type_params.iter().any(|tp| tp == p.as_str()));
                            if n.starts_with("...") && bare_tp {
                                (n.clone(), Type::Any)
                            } else {
                                (n.clone(), Self::substitute(t, &type_param_map))
                            }
                        })
                        .collect();
                    let ret = Self::substitute(&ft.return_type, &type_param_map);
                    // Apply a display-only sweep: any leftover `TypeParameter`
                    // that's still in `ft.type_params` (e.g. `TResult2` in
                    // `Promise.then<TResult1 = T, TResult2 = never>` when only
                    // `.then(onfulfilled)` is called) collapses to `never`
                    // INSIDE THE RETURN TYPE so the canonical hover form is
                    // `Promise<T>` instead of `Promise<T | TResult2>`.
                    // Don't touch the params side — that would treat callback
                    // parameter positions as `never` and break overload
                    // selection / assignability for un-inferred contravariant
                    // slots (e.g. `pg.query<V>(QueryConfig<V>, params)`).
                    let ret = Self::default_unsolved_to_never_in_return(
                        &ret,
                        &ft.type_params,
                        &type_param_map,
                    );
                    (params, ret)
                } else {
                    (ft.params.clone(), Type::clone(&ft.return_type))
                };
                // A leading `this` pseudo-parameter takes no argument slot
                // (object-literal methods and function expressions keep it
                // in their signature).
                let effective_params: Vec<(std::string::String, Type)> = effective_params
                    .into_iter()
                    .enumerate()
                    .filter(|(index, (name, _))| !(*index == 0 && name == "this"))
                    .map(|(_, param)| param)
                    .collect();
                let arity_params: Vec<(std::string::String, Type)> = ft
                    .params
                    .iter()
                    .enumerate()
                    .filter(|(index, (name, _))| !(*index == 0 && name == "this"))
                    .map(|(_, param)| param.clone())
                    .collect();

                // tsc: a non-tuple spread that breaks arity (TS2556) makes the
                // signature inapplicable; its arguments are not type-checked.
                let spread_arity_error = self.check_call_arity(&arity_params, call, expr);
                // Check argument types against (possibly substituted) param types.
                //
                // For rest parameters (`...data: any[]`) we compare each
                // argument against the ELEMENT type (`any`), not the array
                // type (`any[]`). Walking the parameter list lock-step
                // with the argument list and "sticking" on the last
                // parameter when it's a rest gives `console.log("hello")`
                // the same shape as `(...data: any[]) => …`'s rest slot.
                //
                // This was the cause of ~5 400 "Argument of type 'string'
                // is not assignable to parameter of type 'any[]'" false
                // positives on apps/app: every `console.log(...)` /
                // `Array.from(...)` / similar variadic call.
                let has_rest = effective_params
                    .iter()
                    .any(|(name, _)| name.starts_with("..."));
                let last_param_idx = effective_params.len().saturating_sub(1);
                for (i, arg_ty) in arg_types.iter().enumerate() {
                    if spread_arity_error {
                        break;
                    }
                    let param_slot = if i < effective_params.len() {
                        Some(&effective_params[i])
                    } else if has_rest && !effective_params.is_empty() {
                        Some(&effective_params[last_param_idx])
                    } else {
                        None
                    };
                    let Some((pname, pty)) = param_slot else {
                        break;
                    };
                    // A spread of a TUPLE (`f(...t)` with `t: [number,
                    // string]`) supplies one argument per element, checked
                    // against the parameters from this position on (tsc
                    // getEffectiveCallArguments); it consumes the rest of the
                    // parameter list.
                    let spread_tuple = match call.args.get(i).map(|arg| &arg.kind) {
                        Some(ExprKind::Spread(inner)) => match self.infer_expr_type(inner) {
                            Type::Tuple(elements) => Some(elements),
                            _ => None,
                        },
                        _ => None,
                    };
                    if let (Some(arg), Some(elements)) = (call.args.get(i), spread_tuple) {
                        {
                            for (offset, element) in elements.iter().enumerate() {
                                let position = i + offset;
                                let slot = if position < effective_params.len() {
                                    Some(&effective_params[position])
                                } else if has_rest && !effective_params.is_empty() {
                                    Some(&effective_params[last_param_idx])
                                } else {
                                    None
                                };
                                let Some((slot_name, slot_ty)) = slot else {
                                    break;
                                };
                                let element_ty = match element {
                                    Type::Optional(inner) | Type::Rest(inner) => Type::clone(inner),
                                    other => other.clone(),
                                };
                                let target = if slot_name.starts_with("...") {
                                    self.rest_tuple_position_type(
                                        slot_ty,
                                        position.saturating_sub(last_param_idx),
                                    )
                                    .unwrap_or_else(|| self.rest_param_element_type(slot_ty))
                                } else {
                                    Self::optional_param_type(slot_name, slot_ty.clone())
                                };
                                if !self.is_assignable_to(&element_ty, &target)
                                    && !matches!(element_ty, Type::Any | Type::Error)
                                    && !matches!(target, Type::Any | Type::Error)
                                {
                                    let param_display = if slot_name.starts_with('?') {
                                        Self::strip_optionality_undefined(&target).display_string()
                                    } else {
                                        target.display_string()
                                    };
                                    let arg_span = self.error_span_for_expr(arg);
                                    self.diagnostics.push(error_arg_not_assignable(
                                        &self.widen_type(&element_ty).display_string(),
                                        &param_display,
                                        arg_span,
                                    ));
                                    break;
                                }
                            }
                            break;
                        }
                    }
                    let effective_param_ty = if pname.starts_with("...") {
                        // A TUPLE-typed rest parameter (`...args: [string,
                        // string]`) types each remaining argument by its
                        // position in the tuple.
                        let rest_position = i.saturating_sub(last_param_idx);
                        self.rest_tuple_position_type(pty, rest_position)
                            .unwrap_or_else(|| self.rest_param_element_type(pty))
                    } else {
                        // Optional param (`p?: T`) accepts `T | undefined`.
                        Self::optional_param_type(pname, pty.clone())
                    };
                    let arg_span = if i < call.args.len() {
                        self.error_span_for_expr(&call.args[i])
                    } else {
                        expr.span
                    };
                    // Optional parameters (`c?: T`) have effective type
                    // `T | undefined` — an explicit `undefined` argument is fine.
                    let optional_param = pname.starts_with('?');
                    let assignable = self.is_assignable_to(arg_ty, &effective_param_ty)
                        || (optional_param && matches!(arg_ty, Type::Undefined))
                        || matches!(arg_ty, Type::Any | Type::Error)
                        || matches!(effective_param_ty, Type::Any | Type::Error);
                    // tsc elaborates into a fresh literal / arrow argument
                    // first (leaf TS2322s) and stops at that argument.
                    if !assignable && i < call.args.len() {
                        self.elaborating_argument = true;
                        let elaborated =
                            self.elaborate_error(&call.args[i], arg_ty, &effective_param_ty, None);
                        self.elaborating_argument = false;
                        if elaborated {
                            break;
                        }
                    }
                    // Excess property checking for fresh object literal
                    // arguments runs next — when it reports, tsc suppresses
                    // the general TS2345 for the same argument.
                    let mut excess_reported = false;
                    if i < call.args.len() && Self::is_fresh_object_literal(&call.args[i]) {
                        let before = self.diagnostics.len();
                        self.check_excess_properties(
                            arg_ty,
                            &effective_param_ty,
                            arg_span,
                            Some(&call.args[i]),
                        );
                        // A literal argument missing members the parameter
                        // requires: TS2345 via the missing-property refinement
                        // (the literal adopts its contextual type, so plain
                        // assignability can't see the gap).
                        if self.diagnostics.len() == before {
                            self.check_missing_properties_for_literal(
                                &call.args[i],
                                &effective_param_ty,
                                arg_span,
                                true,
                            );
                        }
                        excess_reported = self.diagnostics.len() > before;
                    }
                    if !excess_reported && !assignable {
                        {
                            // tsc prints a fresh literal argument widened unless the
                            // parameter itself is literal-typed; a narrowed variable keeps
                            // its literal type; literals nested in fresh array/object
                            // literals widen.
                            let param_is_literal_typed = {
                                fn has_literal(ty: &Type) -> bool {
                                    match ty {
                                        Type::StringLiteral(_)
                                        | Type::NumberLiteral(_)
                                        | Type::BooleanLiteral(_)
                                        | Type::BigIntLiteral(_) => true,
                                        Type::Union(members) => members.iter().any(has_literal),
                                        _ => false,
                                    }
                                }
                                has_literal(&effective_param_ty)
                            };
                            let widened_arg = match call.args.get(i).map(|a| &a.kind) {
                                Some(
                                    ExprKind::StrLit(_)
                                    | ExprKind::NumLit(_)
                                    | ExprKind::BoolLit(_)
                                    | ExprKind::BigIntLit(_),
                                ) if param_is_literal_typed => arg_ty.clone(),
                                Some(
                                    ExprKind::StrLit(_)
                                    | ExprKind::NumLit(_)
                                    | ExprKind::BoolLit(_)
                                    | ExprKind::BigIntLit(_),
                                ) => self.widen_for_message(
                                    call.args.get(i).map(|a| a.as_ref()),
                                    arg_ty,
                                    &effective_param_ty,
                                ),
                                Some(ExprKind::ArrayLit(_) | ExprKind::ObjectLit(_)) => {
                                    Self::widen_nested_literal_positions(arg_ty)
                                }
                                _ => arg_ty.clone(),
                            };
                            // An optional parameter prints its declared type, not
                            // the `T | undefined` it accepts. Type parameters left
                            // unsolved by inference print as `unknown` (tsc).
                            let unknown_map: HashMap<std::string::String, Type> = ft
                                .type_params
                                .iter()
                                .map(|name| (name.clone(), Type::Unknown))
                                .collect();
                            // An optional parameter prints its declared type only
                            // for a definitely non-nullable argument; a union
                            // argument (a spread's element type) sees the
                            // `| undefined` it must satisfy.
                            let param_display = if optional_param
                                && Self::is_definitely_non_nullable(&widened_arg)
                            {
                                Self::strip_optionality_undefined(&Self::substitute(
                                    pty,
                                    &unknown_map,
                                ))
                                .display_string()
                            } else {
                                // A definitely non-nullable argument reports
                                // against the parameter without its nullish
                                // members (`string | undefined` → `string`).
                                let substituted =
                                    Self::substitute(&effective_param_ty, &unknown_map);
                                Self::strip_nullable_union_target(&widened_arg, &substituted)
                                    .unwrap_or(substituted)
                                    .display_string()
                            };
                            let mut diagnostic = error_arg_not_assignable(
                                &widened_arg.display_string(),
                                &param_display,
                                arg_span,
                            );
                            if let Some(line) =
                                self.type_parameter_elaboration(&widened_arg, &effective_param_ty)
                            {
                                diagnostic.message.push_str("\n  ");
                                diagnostic.message.push_str(&line);
                            } else if let Some((line, related)) =
                                self.assignability_elaboration(arg_ty, &effective_param_ty)
                            {
                                diagnostic.message.push_str("\n  ");
                                diagnostic.message.push_str(&line.replace('\n', "\n  "));
                                diagnostic.related = related.map(|note| vec![note]);
                            } else if let Type::Union(members) = arg_ty {
                                // A union argument names its first failing
                                // member against the nullable-stripped
                                // parameter (nullish members first).
                                let ordered =
                                    members
                                        .iter()
                                        .filter(|m| matches!(m, Type::Undefined))
                                        .chain(members.iter().filter(|m| matches!(m, Type::Null)))
                                        .chain(members.iter().filter(|m| {
                                            !matches!(m, Type::Undefined | Type::Null)
                                        }));
                                if let Some(member) = ordered
                                    .into_iter()
                                    .find(|m| !self.is_assignable_to(m, &effective_param_ty))
                                {
                                    let leaf_target = Self::strip_nullable_union_target(
                                        member,
                                        &effective_param_ty,
                                    )
                                    .unwrap_or_else(|| effective_param_ty.clone());
                                    diagnostic.message.push_str(&format!(
                                        "\n  Type '{}' is not assignable to type '{}'.",
                                        member.display_string_single_line(),
                                        leaf_target.display_string_single_line()
                                    ));
                                }
                            }
                            self.diagnostics.push(diagnostic);
                            // tsc stops at the first argument that fails.
                            break;
                        }
                    }
                }
                // The Call handler above also runs `simplify_type` on the
                // final return, which catches conditional/indexed-access
                // patterns. Returning the raw effective_return here keeps
                // the post-call pipeline working uniformly (overload path
                // returns the raw sig.return_type too).
                effective_return
            }
            Type::TypeReference(ref name, ref type_args) => {
                // Resolve call signatures from interface, using type args
                // for proper generic instantiation (e.g. I2<Date> extends I1<Date[]>)
                // The structural resolution carries inherited call signatures
                // (`interface Bar extends Foo`); the class path is the fallback.
                let instance = self
                    .resolve_type_reference_to_object(name, type_args)
                    .filter(|resolved| matches!(resolved, Type::ObjectType(_)))
                    .unwrap_or_else(|| self.get_class_instance_type(name));
                if let Type::ObjectType(ref info) = instance {
                    if !info.call_signatures.is_empty() {
                        // Several call signatures form an overload set: pick
                        // the first that accepts the arguments (own signatures
                        // precede inherited ones), else tsc's TS2769.
                        if info.call_signatures.len() > 1 {
                            if let Some((_, ret)) =
                                self.select_overload_resolution(&info.call_signatures, arg_types)
                            {
                                return ret;
                            }
                            let diagnostic = self.no_overload_diagnostic(
                                &info.call_signatures,
                                arg_types,
                                call,
                                expr,
                            );
                            self.diagnostics.push(diagnostic);
                            return Type::Error;
                        }
                        // A lone signature is a plain call: per-argument checks.
                        if let Some(sig) = info.call_signatures.first() {
                            let sig_ty = Type::Function(sig.clone());
                            return self.check_call_against_fn_type(&sig_ty, arg_types, call, expr);
                        }
                    }
                }
                Type::Any
            }
            // A possibly-nullish callee (`f?.()`, `t()?.(x)`) is called through
            // its callable constituent; the result is `T | undefined`.
            Type::Union(members)
                if members
                    .iter()
                    .any(|m| matches!(m, Type::Null | Type::Undefined))
                    // A parenthesized optional chain `(o?.b)(x)` ends the chain
                    // and needs TS2722, which is not modeled yet — leave it.
                    && !matches!(call.callee.kind, ExprKind::Paren(_)) =>
            {
                let callable: Vec<Type> = members
                    .iter()
                    .filter(|m| !matches!(m, Type::Null | Type::Undefined))
                    .cloned()
                    .collect();
                if let [single] = callable.as_slice() {
                    let ret = self.check_call_against_fn_type(single, arg_types, call, expr);
                    if matches!(ret, Type::Error)
                        || call.optional
                        || Self::expr_in_optional_chain(&call.callee)
                    {
                        // Inside an optional chain (`o?.b()`, `f?.()`) the chain
                        // machinery owns the `undefined`.
                        return ret;
                    }
                    return Type::flatten_union(vec![ret, Type::Undefined]);
                }
                Type::Any
            }
            // A variable annotated with an inline object type literal that
            // carries call signatures (`const f: { (x: "a"): string; (x): number }`)
            // is itself callable. Run overload selection so const/literal-typed
            // args pick the matching signature; fall back to the first signature.
            Type::ObjectType(info) if !info.call_signatures.is_empty() => {
                if let Some(ret) = self.try_overload_resolution(&info.call_signatures, arg_types) {
                    return ret;
                }
                let widened: Vec<Type> = arg_types
                    .iter()
                    .map(|t| self.widen_argument_for_inference(t))
                    .collect();
                if let Some(sig) = info.call_signatures.first() {
                    if !sig.type_params.is_empty() {
                        let mut inferred = self.infer_type_arguments_for_call(
                            &widened,
                            &sig.params,
                            &sig.type_params,
                        );
                        Self::apply_type_param_defaults(sig, &mut inferred);
                        let ret = Self::substitute(&sig.return_type, &inferred);
                        return Self::default_unsolved_to_never_in_return(
                            &ret,
                            &sig.type_params,
                            &inferred,
                        );
                    }
                    return Type::clone(&sig.return_type);
                }
                Type::Any
            }
            _ => Type::Any,
        }
    }

    /// Try each overload signature in order. Return the return type of the
    /// first signature whose parameters match the provided argument types.
    /// Returns `None` if no overload matches.
    /// tsc widens a fresh literal return expression (`() => 0` is
    /// `() => number`, `{ name: "joe" }` becomes `{ name: string; }`) unless
    /// the contextual return type keeps literal types.
    /// The type of `globalThis.<name>` (tsc: `typeof globalThis` holds the
    /// script-level `var`/`function`/enum/namespace values and the lib's
    /// values). A block-scoped, class or type-only global is TS2339; an
    /// unknown name is TS7017 under noImplicitAny.
    fn global_this_member_type(&mut self, name: &str, span: Span) -> Option<Type> {
        match self.script_global_kinds.get(name).copied() {
            Some(0) => Some(self.lookup_var(name).cloned().unwrap_or(Type::Any)),
            // A type-only name is no property at all (TS7017 below).
            Some(2) => self.global_this_unknown_member(span),
            Some(_) => {
                self.diagnostics
                    .push(crate::diagnostics::error_property_not_exist(
                        name,
                        "typeof globalThis",
                        span,
                    ));
                Some(Type::Error)
            }
            None => {
                let lib_value = crate::stdlib::stdlib_member_availability(&self.compiler_options)
                    .is_some_and(|availability| availability.declares_global_value(name));
                if lib_value {
                    return Some(self.lookup_var(name).cloned().unwrap_or(Type::Any));
                }
                self.global_this_unknown_member(span)
            }
        }
    }

    fn global_this_unknown_member(&mut self, span: Span) -> Option<Type> {
        if self.no_implicit_any {
            self.diagnostics.push(Diagnostic {
                code: 7017,
                message: "Element implicitly has an 'any' type because type 'typeof globalThis' has no index signature.".to_string(),
                category: DiagnosticCategory::Error,
                file_name: None,
                span: Some(span),
                related: None,
            });
            return Some(Type::Error);
        }
        Some(Type::Any)
    }

    /// tsc getTemplateLiteralType for a template EXPRESSION: literal
    /// substitutions fold into the fixed text, others widen to their base
    /// type; a template without remaining substitutions is a string literal.
    fn template_literal_type_of_expression(
        &self,
        quasis: Vec<std::string::String>,
        types: Vec<Type>,
    ) -> Type {
        let mut folded_quasis: Vec<std::string::String> = vec![quasis[0].clone()];
        let mut folded_types: Vec<Type> = Vec::new();
        for (index, ty) in types.into_iter().enumerate() {
            let suffix = quasis.get(index + 1).cloned().unwrap_or_default();
            match Self::type_to_literal_string(&ty) {
                Some(text) => {
                    let last = folded_quasis.last_mut().unwrap();
                    last.push_str(&text);
                    last.push_str(&suffix);
                }
                None => {
                    folded_types.push(match ty {
                        Type::Any | Type::Error => Type::Any,
                        other => self.widen_type(&other),
                    });
                    folded_quasis.push(suffix);
                }
            }
        }
        if folded_types.is_empty() {
            return Type::StringLiteral(folded_quasis.concat());
        }
        if folded_types.iter().any(|t| matches!(t, Type::Any)) {
            return Type::String;
        }
        // Unions of literals cross-multiply into a union of string literals
        // (`\`abc${"foo" | "bar"}\`` is `"abcfoo" | "abcbar"`).
        let literal_union = |t: &Type| match t {
            Type::Union(members) => members
                .iter()
                .all(|m| Self::type_to_literal_string(m).is_some()),
            _ => false,
        };
        if folded_types.iter().all(literal_union) {
            return Self::evaluate_template_literal_type(folded_quasis, folded_types);
        }
        Type::TemplateLiteral {
            quasis: folded_quasis.into(),
            types: folded_types.into(),
        }
    }

    /// Inside an assignment pattern, `x = default` names the target variable
    /// `x` with its default expression.
    fn pattern_default_target<'a>(
        &self,
        elem: &'a Expr,
    ) -> Option<(std::string::String, &'a Expr)> {
        if self.assignment_pattern_depth == 0 {
            return None;
        }
        match &elem.kind {
            ExprKind::Assign(assign) if assign.op == AssignOp::Assign => match &assign.left.kind {
                ExprKind::Ident(name) => Some((name.to_string(), assign.right.as_ref())),
                _ => None,
            },
            _ => None,
        }
    }

    /// tsc hasPrimitiveConstraint: a constraint made of primitive/literal
    /// types keeps literal inference candidates unwidened.
    pub(crate) fn is_primitive_constraint(constraint: &Type) -> bool {
        match constraint {
            Type::String
            | Type::Number
            | Type::Boolean
            | Type::BigInt
            | Type::Symbol
            | Type::StringLiteral(_)
            | Type::NumberLiteral(_)
            | Type::BooleanLiteral(_)
            | Type::BigIntLiteral(_)
            | Type::TemplateLiteral { .. } => true,
            Type::Union(members) => members.iter().all(Self::is_primitive_constraint),
            _ => false,
        }
    }

    /// The contextual parameter at position `index` of a contextual signature:
    /// positions at or after a rest parameter (`...args: T[]`) take its
    /// element type (tsc getTypeAtPosition), not the whole array.
    pub(crate) fn contextual_param_at(
        params: &[(String, Type)],
        index: usize,
    ) -> Option<(String, Type)> {
        let rest = params
            .iter()
            .enumerate()
            .find(|(position, (name, _))| *position <= index && name.starts_with("..."));
        if let Some((_, (name, ty))) = rest {
            let element = match ty {
                Type::Array(element) => Some(Type::clone(element)),
                Type::TypeReference(owner, args)
                    if matches!(owner.as_str(), "Array" | "ReadonlyArray") && args.len() == 1 =>
                {
                    Some(args[0].clone())
                }
                Type::Any => Some(Type::Any),
                _ => None,
            };
            if let Some(element) = element {
                return Some((name.trim_start_matches("...").to_string(), element));
            }
        }
        params.get(index).cloned()
    }

    pub(crate) fn widen_fresh_return_expr_type(&self, expr: &Expr, ty: Type) -> Type {
        // Without strictNullChecks a nullish return widens to `any` (tsc
        // getWidenedType), whatever the contextual return type.
        if !self.strict_null_checks && matches!(ty, Type::Null | Type::Undefined) {
            return Type::Any;
        }
        if let Some(Some(contextual)) = self.return_type_stack.last() {
            if self.contextual_type_keeps_literals(contextual, 3) {
                return ty;
            }
        }
        let mut inner = expr;
        while let ExprKind::Paren(e) = &inner.kind {
            inner = e;
        }
        if Self::is_const_assertion_expr(inner) {
            return ty;
        }
        match &inner.kind {
            ExprKind::StrLit(_)
            | ExprKind::NumLit(_)
            | ExprKind::BoolLit(_)
            | ExprKind::BigIntLit(_)
            | ExprKind::Template(_) => self.widen_type(&ty),
            ExprKind::Unary(unary)
                if matches!(unary.op, UnaryOp::Neg | UnaryOp::Pos)
                    && matches!(
                        unary.argument.kind,
                        ExprKind::NumLit(_) | ExprKind::BigIntLit(_)
                    ) =>
            {
                self.widen_type(&ty)
            }
            ExprKind::ObjectLit(_) | ExprKind::ArrayLit(_) => {
                Self::widen_nested_literal_positions(&ty)
            }
            _ => ty,
        }
    }

    /// Whether a contextual type keeps fresh literal types somewhere in the
    /// positions a literal expression could fill (literal types, type
    /// parameters, enums; recursing through unions, arrays and properties).
    pub(crate) fn contextual_type_keeps_literals(&self, ty: &Type, depth: u8) -> bool {
        if depth == 0 {
            return true;
        }
        match ty {
            Type::NumberLiteral(_)
            | Type::StringLiteral(_)
            | Type::BooleanLiteral(_)
            | Type::BigIntLiteral(_)
            | Type::TypeParameter(_)
            | Type::EnumType(_)
            | Type::EnumVariant { .. }
            | Type::Conditional { .. }
            | Type::Mapped { .. }
            | Type::IndexedAccess(..)
            | Type::Keyof(_)
            | Type::Infer(_) => true,
            Type::Union(members) | Type::Intersection(members) => members
                .iter()
                .any(|m| self.contextual_type_keeps_literals(m, depth - 1)),
            Type::Array(inner)
            | Type::Optional(inner)
            | Type::Readonly(inner)
            | Type::Rest(inner) => self.contextual_type_keeps_literals(inner, depth - 1),
            Type::Tuple(elems) => elems
                .iter()
                .any(|e| self.contextual_type_keeps_literals(e, depth - 1)),
            Type::ObjectType(info) => info
                .properties
                .iter()
                .any(|(_, p)| self.contextual_type_keeps_literals(p, depth - 1)),
            Type::TypeReference(_, args) if args.is_empty() => {
                match self.resolve_type_for_assignability(ty) {
                    Some(resolved) if resolved != *ty => {
                        self.contextual_type_keeps_literals(&resolved, depth - 1)
                    }
                    // An unresolved bare name may be a type parameter.
                    _ => {
                        !(self.class_info.contains_key(match ty {
                            Type::TypeReference(name, _) => name.as_str(),
                            _ => "",
                        }) || self.interface_info.contains_key(match ty {
                            Type::TypeReference(name, _) => name.as_str(),
                            _ => "",
                        }))
                    }
                }
            }
            Type::TypeReference(_, args) => args
                .iter()
                .any(|a| self.contextual_type_keeps_literals(a, depth - 1)),
            _ => false,
        }
    }

    /// TS2554 for a call whose parameter metadata is faithful (local
    /// function declarations, methods on program-declared receivers).
    pub(crate) fn check_call_arity(
        &mut self,
        ft_params: &[(std::string::String, Type)],
        call: &CallExpr,
        expr: &Expr,
    ) -> bool {
        // TS2554: Check argument count against parameter metadata.
        // Only check when we have stored param info (from local function decls).
        // Skip for: builtins, imports, methods, constructors (too many false positives).
        // Bare local functions, and methods on program-declared
        // receivers (class / interface / type literal), have faithful
        // parameter metadata; library receivers are skipped.
        let arity_checkable = match &call.callee.kind {
            ExprKind::Ident(name) => self.fn_param_info.contains_key(name.as_str()),
            ExprKind::Member(member) => self.receiver_is_user_declared(&member.object),
            _ => false,
        };
        if arity_checkable {
            // TS2556: a spread argument of non-tuple type must land on a rest
            // parameter; otherwise tsc reports it instead of an arity error.
            let positional_params: Vec<&(std::string::String, Type)> =
                ft_params.iter().filter(|(n, _)| n != "this").collect();
            let mut spread_reported = false;
            for (index, arg) in call.args.iter().enumerate() {
                let ExprKind::Spread(inner) = &arg.kind else {
                    continue;
                };
                let spread_ty = self.infer_expr_type(inner);
                let is_tuple = matches!(spread_ty, Type::Tuple(_))
                    || matches!(&spread_ty, Type::Union(members)
                        if members.iter().all(|m| matches!(m, Type::Tuple(_))));
                // tsc: a non-tuple spread is fine while it lands on a
                // parameter (its element type is then checked against that
                // parameter); beyond the last positional parameter it needs
                // a rest parameter.
                let has_rest_param = positional_params
                    .last()
                    .is_some_and(|(n, _)| n.starts_with("..."));
                let beyond_params = index >= positional_params.len() && !has_rest_param;
                // tsc (hasCorrectArity): a non-tuple spread cannot satisfy
                // required parameters either, since its length is unknown.
                let min_required = positional_params
                    .iter()
                    .filter(|(n, _)| !n.starts_with('?') && !n.starts_with("..."))
                    .count();
                let before_required = index < min_required;
                if !is_tuple
                    && (beyond_params || before_required)
                    && !matches!(spread_ty, Type::Any | Type::Error)
                {
                    self.diagnostics.push(Diagnostic {
                        code: 2556,
                        message: "A spread argument must either have a tuple type or be passed to a rest parameter.".to_string(),
                        category: DiagnosticCategory::Error,
                        file_name: None,
                        span: Some(arg.span),
                        related: None,
                    });
                    spread_reported = true;
                }
            }
            if spread_reported {
                return true;
            }
            // The name-keyed fn_param_info table is file-level, so a
            // BLOCK-SCOPED shadow (`function foo() {}` inside an if)
            // would be checked against the wrong signature. When the
            // binding isn't at root scope, derive the arity from the
            // callee's own resolved signature instead — its params
            // encode optionality ("?x") and rest ("...x") in the name.
            let scoped_info: Option<(usize, usize, bool)> = {
                let real: Vec<&(std::string::String, Type)> =
                    ft_params.iter().filter(|(n, _)| n != "this").collect();
                let has_rest = real
                    .last()
                    .map(|(n, _)| n.starts_with("..."))
                    .unwrap_or(false);
                let max_params = if has_rest { real.len() - 1 } else { real.len() };
                let min_params = if self.current_file_is_js() {
                    // JavaScript declarations have optional
                    // parameters by default, even though the
                    // source syntax has no `?` markers.
                    0
                } else {
                    // Up to the LAST required parameter (a
                    // defaulted parameter followed by a required
                    // one still has to be passed positionally).
                    real.iter()
                        .take(max_params)
                        .rposition(|(n, _)| !n.starts_with('?'))
                        .map(|index| index + 1)
                        .unwrap_or(0)
                };
                Some((min_params, max_params, has_rest))
            };
            if let Some((min_params, max_params, has_rest)) = scoped_info {
                // A spread of a tuple contributes one argument per element.
                let arg_count: usize = call
                    .args
                    .iter()
                    .map(|arg| match &arg.kind {
                        ExprKind::Spread(inner) => match self.infer_expr_type(inner) {
                            Type::Tuple(elements) => elements.len(),
                            _ => 1,
                        },
                        _ => 1,
                    })
                    .sum();
                if arg_count < min_params {
                    // TypeScript underlines the callee name for "too few
                    // args" — for a method call, just the member name.
                    let callee_span = match &call.callee.kind {
                        ExprKind::Member(member) => Span::new(
                            call.callee
                                .span
                                .end
                                .saturating_sub(member.property.len() as u32),
                            call.callee.span.end,
                        ),
                        _ => call.callee.span,
                    };
                    let mut diagnostic =
                        error_arg_count(min_params, max_params, arg_count, callee_span);
                    // tsc points at the first parameter left without
                    // an argument.
                    let spans_key = match &call.callee.kind {
                        ExprKind::Ident(callee_name) => Some(callee_name.to_string()),
                        ExprKind::Member(member) => {
                            self.method_param_spans_key(&member.object, &member.property)
                        }
                        _ => None,
                    };
                    if let Some(spans_key) = spans_key {
                        if let Some((param_name, param_span)) = self
                            .function_param_spans
                            .get(spans_key.as_str())
                            .and_then(|params| params.get(arg_count))
                        {
                            diagnostic.related =
                                Some(vec![self.missing_argument_note(param_name, *param_span)]);
                        }
                    }
                    self.diagnostics.push(diagnostic);
                } else if !has_rest && arg_count > max_params {
                    // TypeScript underlines the excess arguments for "too many args"
                    let excess_span = if arg_count > 0 && max_params < call.args.len() {
                        // Span from the first excess arg to the last arg
                        let start = call.args[max_params].span;
                        let end = call.args.last().map(|a| a.span).unwrap_or(start);
                        Span {
                            start: start.start,
                            end: end.end,
                        }
                    } else {
                        expr.span
                    };
                    self.diagnostics.push(error_arg_count(
                        min_params,
                        max_params,
                        arg_count,
                        excess_span,
                    ));
                }
            }
        }
        false
    }

    /// A member missing from the builtin model of a primitive: TS2339 when
    /// the configured lib files do not declare it on the primitive's
    /// interface (or `Object`) either; otherwise the member exists and is
    /// simply unmodeled (`any`).
    /// TS2339 for a member missing from a lib-declared interface instance
    /// (`Promise<number>`, `RegExp`, ...). Only fires when the lib model knows
    /// the owner, the member is on no active lib declaration of it (or of
    /// `Object`), and no program augmentation adds it.
    fn lib_instance_member_fallback(
        &mut self,
        owner: &str,
        receiver: &Type,
        property: &str,
        property_span: Span,
        is_js_file: bool,
        reported_future_lib_member: bool,
    ) -> Type {
        if is_js_file
            || reported_future_lib_member
            || self.in_js_expando_assignment
            || property.starts_with('#')
            || property.starts_with('<')
            || property.starts_with('[')
            || property.is_empty()
            || self.interface_info.contains_key(owner)
            || self.class_info.contains_key(owner)
            || self.type_aliases.contains_key(owner)
            || self.contains_unresolved_type_param(receiver)
        {
            return Type::Any;
        }
        let Some(availability) = stdlib::stdlib_member_availability(&self.compiler_options) else {
            return Type::Any;
        };
        if !availability.owner_is_active(owner)
            || availability.member_is_active(owner, property)
            || availability.member_is_active("Object", property)
        {
            return Type::Any;
        }
        let augmented = self.interface_info.iter().any(|(key, info)| {
            (key == owner
                || key == "Object"
                || key
                    .rsplit_once('.')
                    .is_some_and(|(_, tail)| tail == owner || tail == "Object"))
                && info
                    .object_type
                    .properties
                    .iter()
                    .any(|(n, _)| n.trim_start_matches('?') == property)
        });
        if augmented {
            return Type::Any;
        }
        self.diagnostics.push(error_property_not_exist(
            property,
            &receiver.display_string_single_line(),
            property_span,
        ));
        Type::Error
    }

    /// The structural shape we hold for interface `name` is complete: every
    /// declared member is modeled (accessor and computed members are not),
    /// no fragment comes from another file, and every base is itself a
    /// fully declared program interface or a fully resolved class chain.
    fn interface_shape_fully_declared(&self, name: &str) -> bool {
        let mut seen = HashSet::new();
        self.interface_shape_fully_declared_inner(name, &mut seen)
    }

    fn interface_shape_fully_declared_inner(&self, name: &str, seen: &mut HashSet<String>) -> bool {
        if !seen.insert(name.to_string()) {
            return true;
        }
        if self.class_info.contains_key(name) {
            return self.class_chain_fully_resolved(name);
        }
        let Some(info) = self.interface_info.get(name) else {
            return false;
        };
        if info.decl_file.is_empty()
            || self.type_aliases.contains_key(name)
            || stdlib::any_lib_declares_global(name)
        {
            return false;
        }
        if info
            .member_locations
            .iter()
            .any(|(_, (file, _))| file != &info.decl_file)
        {
            return false;
        }
        if info.member_locations.keys().any(|member| {
            !info
                .object_type
                .properties
                .iter()
                .any(|(n, _)| n.trim_start_matches('?') == member.as_str())
        }) {
            return false;
        }
        info.extends
            .iter()
            .all(|(base, _)| self.interface_shape_fully_declared_inner(base, seen))
    }

    fn primitive_member_fallback(
        &mut self,
        owner: &str,
        object: &Expr,
        receiver: &Type,
        property: &str,
        property_span: Span,
        reported_future_lib_member: bool,
    ) -> Type {
        // A receiver narrowed to this primitive from a wider declared type
        // (`let x: string | number`) may be widened again by a loop back
        // edge our flow analysis does not model; only report when the
        // declared type itself is the primitive.
        if let ExprKind::Ident(name) = &object.kind {
            if let Some(declared) = self.lookup_declared_var(name) {
                let declared_is_owner = match owner {
                    "Number" => matches!(declared, Type::Number | Type::NumberLiteral(_)),
                    "String" => matches!(declared, Type::String | Type::StringLiteral(_)),
                    "Boolean" => matches!(declared, Type::Boolean | Type::BooleanLiteral(_)),
                    _ => false,
                };
                if !declared_is_owner && !matches!(declared, Type::Any) {
                    return Type::Any;
                }
            }
        }
        if reported_future_lib_member
            || self.in_js_expando_assignment
            || self.current_file_is_js()
            || property.starts_with('<')
            || property.is_empty()
        {
            return Type::Any;
        }
        let Some(availability) = stdlib::stdlib_member_availability(&self.compiler_options) else {
            return Type::Any;
        };
        if availability.member_is_active(owner, property)
            || availability.member_is_active("Object", property)
            || property.starts_with('[')
        {
            return Type::Any;
        }
        // A program-declared augmentation (`interface Number { extra(): void }`,
        // possibly inside `declare global`) adds members too.
        let augmented = self.interface_info.iter().any(|(key, info)| {
            (key == owner
                || key == "Object"
                || key
                    .rsplit_once('.')
                    .is_some_and(|(_, tail)| tail == owner || tail == "Object"))
                && info
                    .object_type
                    .properties
                    .iter()
                    .any(|(n, _)| n == property)
        });
        if augmented {
            return Type::Any;
        }
        // Only report when the lib model actually knows this owner at all.
        if !availability.owner_is_active(owner) {
            return Type::Any;
        }
        self.diagnostics.push(error_property_not_exist(
            property,
            &receiver.display_string_single_line(),
            property_span,
        ));
        Type::Error
    }

    /// TS7006/TS7019 for a parameter left implicitly `any` although the
    /// expression sits in a contextual position (the syntactic pass skips
    /// those positions because it cannot see whether a signature exists).
    fn report_uncontextualized_parameter(&mut self, param: &Param, contextual: bool) {
        if !contextual
            || param.type_ann.is_some()
            || param.initializer.is_some()
            || !self.implicit_any_enabled_for_current_file()
        {
            return;
        }
        let PatKind::Ident(name) = &param.name.kind else {
            return;
        };
        let diagnostic = if param.dotdotdot {
            diagnostics::error_implicit_any_rest(name, param.span)
        } else {
            diagnostics::error_implicit_any(name, param.span)
        };
        if !self
            .diagnostics
            .iter()
            .any(|existing| existing.code == diagnostic.code && existing.span == diagnostic.span)
        {
            self.diagnostics.push(diagnostic);
        }
    }

    /// Whether an expression is a link of an optional chain (`a?.b`, `a?.b.c`,
    /// `a?.b()`), so a nullish result is owned by the chain.
    pub(crate) fn expr_in_optional_chain(expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Member(member) => {
                member.optional || Self::expr_in_optional_chain(&member.object)
            }
            ExprKind::ElemAccess(access) => {
                access.optional || Self::expr_in_optional_chain(&access.object)
            }
            ExprKind::Call(call) => call.optional || Self::expr_in_optional_chain(&call.callee),
            ExprKind::Paren(inner) | ExprKind::NonNull(inner) => {
                Self::expr_in_optional_chain(inner)
            }
            _ => false,
        }
    }

    /// Whether `name` is a top-level constituent of `ty` (itself, or a union member).
    fn type_param_at_top_level(ty: &Type, name: &str) -> bool {
        match ty {
            Type::TypeParameter(param) => param == name,
            Type::TypeReference(reference, args) if args.is_empty() => reference == name,
            Type::Union(members) => members
                .iter()
                .any(|member| Self::type_param_at_top_level(member, name)),
            _ => false,
        }
    }

    /// Argument types feeding type-argument inference: fresh literals widen,
    /// but under strictNullChecks `null`/`undefined` stay real candidates
    /// (tsc infers `T = B | undefined` from `equal(b, undefined)`).
    fn widen_argument_for_inference(&self, ty: &Type) -> Type {
        if self.strict_null_checks && matches!(ty, Type::Null | Type::Undefined) {
            return ty.clone();
        }
        Self::widen_nested_literals(ty)
    }

    pub(crate) fn try_overload_resolution(
        &self,
        sigs: &[FunctionType],
        arg_types: &[Type],
    ) -> Option<Type> {
        self.select_overload_resolution(sigs, arg_types)
            .map(|(_, ret_ty)| ret_ty)
    }

    pub(crate) fn select_overload_resolution(
        &self,
        sigs: &[FunctionType],
        arg_types: &[Type],
    ) -> Option<(usize, Type)> {
        // tsc reorderCandidates: signatures with literal-typed parameters
        // ("specialized" signatures) are tried before the others.
        fn has_literal_param(sig: &FunctionType) -> bool {
            fn literal_like(ty: &Type) -> bool {
                match ty {
                    Type::StringLiteral(_)
                    | Type::NumberLiteral(_)
                    | Type::BooleanLiteral(_)
                    | Type::BigIntLiteral(_) => true,
                    Type::Union(members) => members.iter().any(literal_like),
                    _ => false,
                }
            }
            sig.params.iter().any(|(_, ty)| literal_like(ty))
        }
        let ordered: Vec<(usize, &FunctionType)> = sigs
            .iter()
            .enumerate()
            .filter(|(_, sig)| has_literal_param(sig))
            .chain(
                sigs.iter()
                    .enumerate()
                    .filter(|(_, sig)| !has_literal_param(sig)),
            )
            .collect();
        for (index, sig) in ordered {
            if self.overload_matches(sig, arg_types) {
                let ret = if !sig.type_params.is_empty() {
                    // Generic overload: infer type arguments and substitute
                    let widened: Vec<Type> = arg_types
                        .iter()
                        .map(|t| self.widen_argument_for_inference(t))
                        .collect();
                    let mut type_param_map =
                        self.infer_type_arguments_for_call(&widened, &sig.params, &sig.type_params);
                    Self::apply_type_param_defaults(sig, &mut type_param_map);
                    let ret = Self::substitute(&sig.return_type, &type_param_map);
                    Self::default_unsolved_to_never_in_return(
                        &ret,
                        &sig.type_params,
                        &type_param_map,
                    )
                } else {
                    Type::clone(&sig.return_type)
                };
                return Some((index, ret));
            }
        }
        None
    }

    /// Check if a single overload signature matches the given argument types.
    /// Canonical decimal text of a bigint literal (`0x10n` → `16n`,
    /// `1_000n` → `1000n`), as tsc prints bigint literal types.
    pub(crate) fn normalize_bigint_literal(text: &str) -> std::string::String {
        let body = text.trim_end_matches('n').replace('_', "");
        let (radix, digits) =
            if let Some(rest) = body.strip_prefix("0x").or_else(|| body.strip_prefix("0X")) {
                (16u32, rest)
            } else if let Some(rest) = body.strip_prefix("0o").or_else(|| body.strip_prefix("0O")) {
                (8, rest)
            } else if let Some(rest) = body.strip_prefix("0b").or_else(|| body.strip_prefix("0B")) {
                (2, rest)
            } else {
                (10, body.as_str())
            };
        if radix == 10 {
            let trimmed = digits.trim_start_matches('0');
            return format!("{}n", if trimmed.is_empty() { "0" } else { trimmed });
        }
        // Arbitrary-precision radix conversion on little-endian decimal digits.
        let mut decimal: Vec<u8> = vec![0];
        for ch in digits.chars() {
            let Some(value) = ch.to_digit(radix) else {
                return text.to_string();
            };
            let mut carry = value;
            for digit in decimal.iter_mut() {
                let v = *digit as u32 * radix + carry;
                *digit = (v % 10) as u8;
                carry = v / 10;
            }
            while carry > 0 {
                decimal.push((carry % 10) as u8);
                carry /= 10;
            }
        }
        let text: std::string::String = decimal.iter().rev().map(|d| (b'0' + d) as char).collect();
        let trimmed = text.trim_start_matches('0');
        format!("{}n", if trimmed.is_empty() { "0" } else { trimmed })
    }

    /// tsc's TS2769: with two or three candidates every candidate's failure
    /// is listed ("Overload i of N, '<signature>', gave the following
    /// error."); with more, only the last. The error anchors at the failing
    /// argument when all candidates fail on the same one.
    fn no_overload_diagnostic(
        &self,
        sigs: &[FunctionType],
        arg_types: &[Type],
        call: &CallExpr,
        expr: &Expr,
    ) -> Diagnostic {
        self.no_overload_arguments(sigs, arg_types, &call.args, expr.span)
    }

    fn no_overload_arguments(
        &self,
        sigs: &[FunctionType],
        arg_types: &[Type],
        arguments: &[Box<Expr>],
        expression_span: Span,
    ) -> Diagnostic {
        struct Failure {
            arg_index: usize,
            lines: Vec<std::string::String>,
            anchor: Option<Span>,
        }
        let fits_arity = |sig: &FunctionType| {
            let required = sig
                .params
                .iter()
                .filter(|(name, _)| !name.starts_with('?') && !name.starts_with("..."))
                .count();
            let has_rest = sig.params.iter().any(|(name, _)| name.starts_with("..."));
            arg_types.len() >= required && (has_rest || arg_types.len() <= sig.params.len())
        };
        // tsc only relates arguments against the candidates of the right
        // arity; a single such candidate reports its own argument error.
        if sigs.len() > 1 {
            let mut candidates = sigs.iter().filter(|sig| fits_arity(sig));
            if let (Some(only), None) = (candidates.next(), candidates.next()) {
                let single = self.no_overload_arguments(
                    std::slice::from_ref(only),
                    arg_types,
                    arguments,
                    expression_span,
                );
                let mut lines = single.message.lines().skip(2);
                if let Some(head) = lines
                    .next()
                    .and_then(|line| line.strip_prefix("    "))
                    .filter(|head| head.starts_with("Argument of type '"))
                {
                    let mut message = head.to_string();
                    for line in lines {
                        message.push('\n');
                        message.push_str(line.strip_prefix("    ").unwrap_or(line));
                    }
                    return Diagnostic {
                        code: 2345,
                        message,
                        category: DiagnosticCategory::Error,
                        file_name: None,
                        span: single.span,
                        related: self.implementation_success_note(sigs, arg_types),
                    };
                }
            }
        }
        let mut failures: Vec<Failure> = Vec::new();
        for sig in sigs {
            let required = sig
                .params
                .iter()
                .filter(|(name, _)| !name.starts_with('?') && !name.starts_with("..."))
                .count();
            let has_rest = sig.params.iter().any(|(name, _)| name.starts_with("..."));
            if arg_types.len() < required || (!has_rest && arg_types.len() > sig.params.len()) {
                return error_no_overload_match(sigs.len(), expression_span);
            }
            let mut failure = None;
            for (index, arg_ty) in arg_types.iter().enumerate() {
                let Some((param_name, param_ty)) = sig
                    .params
                    .get(index)
                    .or_else(|| has_rest.then(|| sig.params.last()).flatten())
                else {
                    break;
                };
                let param_ty = if param_name.starts_with("...") {
                    match param_ty {
                        Type::Array(elem) => Type::clone(elem),
                        other => other.clone(),
                    }
                } else if param_name.starts_with('?') {
                    Type::flatten_union(vec![param_ty.clone(), Type::Undefined])
                } else {
                    param_ty.clone()
                };
                if self.is_assignable_to(arg_ty, &param_ty) {
                    continue;
                }
                // tsc elaborates a fresh literal / arrow argument inside the
                // overload entry: the excess property or the leaf mismatch,
                // anchored there.
                let mut anchor = None;
                let detail = arguments.get(index).and_then(|arg_expr| {
                    if let Some((name, span, target_disp)) =
                        self.first_excess_property(arg_expr, &param_ty)
                    {
                        return Some((
                            format!(
                                "Object literal may only specify known properties, and '{name}' does not exist in type '{target_disp}'."
                            ),
                            span,
                        ));
                    }
                    self.elaboration_leaf(arg_expr, arg_ty, &param_ty).map(
                        |(leaf_source, leaf_target, span)| {
                            (
                                format!(
                                    "Type '{}' is not assignable to type '{}'.",
                                    self.elaboration_display_type(&leaf_source, &leaf_target)
                                        .display_string_single_line(),
                                    leaf_target.display_string_single_line()
                                ),
                                span,
                            )
                        },
                    )
                });
                let mut lines = match detail {
                    Some((line, span)) => {
                        anchor = Some(span);
                        vec![line]
                    }
                    None => vec![format!(
                        "Argument of type '{}' is not assignable to parameter of type '{}'.",
                        self.elaboration_display_type(arg_ty, &param_ty)
                            .display_string_single_line(),
                        if param_name.starts_with('?') {
                            Self::strip_optionality_undefined(&param_ty)
                                .display_string_single_line()
                        } else {
                            param_ty.display_string_single_line()
                        }
                    )],
                };
                if let Type::Union(members) = arg_ty {
                    // tsc holds nullish members first internally (they have
                    // the smallest type ids), so they are the ones reported.
                    let mut nullish_first = members
                        .iter()
                        .filter(|m| matches!(m, Type::Undefined))
                        .chain(members.iter().filter(|m| matches!(m, Type::Null)))
                        .chain(
                            members
                                .iter()
                                .filter(|m| !matches!(m, Type::Undefined | Type::Null)),
                        );
                    if let Some(member) =
                        nullish_first.find(|member| !self.is_assignable_to(member, &param_ty))
                    {
                        lines.push(format!(
                            "Type '{}' is not assignable to type '{}'.",
                            member.display_string_single_line(),
                            param_ty.display_string_single_line()
                        ));
                    }
                }
                failure = Some(Failure {
                    arg_index: index,
                    lines,
                    anchor,
                });
                break;
            }
            match failure {
                Some(failure) => failures.push(failure),
                None => return error_no_overload_match(sigs.len(), expression_span),
            }
        }
        let signature_display = |sig: &FunctionType| -> std::string::String {
            self.signature_text(&sig.type_params, &sig.params, &sig.return_type, false)
        };
        let shown: Vec<(usize, &FunctionType, &Failure)> = if sigs.len() > 3 {
            let last = sigs.len() - 1;
            vec![(last, &sigs[last], &failures[last])]
        } else {
            sigs.iter()
                .zip(failures.iter())
                .enumerate()
                .map(|(index, (sig, failure))| (index, sig, failure))
                .collect()
        };
        let mut message = std::string::String::from("No overload matches this call.");
        for (index, sig, failure) in &shown {
            message.push_str(&format!(
                "\n  Overload {} of {}, '{}', gave the following error.",
                index + 1,
                sigs.len(),
                signature_display(sig)
            ));
            for (depth, line) in failure.lines.iter().enumerate() {
                message.push('\n');
                message.push_str(&"  ".repeat(depth + 2));
                message.push_str(line);
            }
        }
        let same_argument = shown
            .iter()
            .all(|(_, _, failure)| failure.arg_index == shown[0].2.arg_index);
        let span = if same_argument {
            shown[0].2.anchor.unwrap_or_else(|| {
                arguments
                    .get(shown[0].2.arg_index)
                    .map(|arg| arg.span)
                    .unwrap_or(expression_span)
            })
        } else {
            expression_span
        };
        Diagnostic {
            code: 2769,
            message,
            category: DiagnosticCategory::Error,
            file_name: None,
            span: Some(span),
            related: self.implementation_success_note(sigs, arg_types),
        }
    }

    /// tsc's addImplementationSuccessElaboration: when the overloads all
    /// reject a call their implementation would accept, TS2793 points at it.
    fn implementation_success_note(
        &self,
        sigs: &[FunctionType],
        arg_types: &[Type],
    ) -> Option<Vec<RelatedDiagnostic>> {
        let (implementation, file, span) = self
            .overload_implementations
            .lock()
            .expect("overload_implementations lock poisoned")
            .get(sigs)
            .cloned()?;
        if !self.overload_matches(&implementation, arg_types) {
            return None;
        }
        Some(vec![RelatedDiagnostic {
            code: 2793,
            message: "The call would have succeeded against this implementation, but \
                      implementation signatures of overloads are not externally visible."
                .to_string(),
            file_name: file,
            span: Some(span),
        }])
    }

    pub(crate) fn overload_matches(&self, sig: &FunctionType, arg_types: &[Type]) -> bool {
        // LAWYER: overload candidates are checked STRICTLY about nullish
        // sources — tsc re-checks each signature without CFA compensation,
        // which is how `h(x)` with `x: number | undefined` rejects every
        // overload and yields TS2769 instead of leniently matching one.
        use std::sync::atomic::Ordering::Relaxed;
        let prev = self.strict_nullish_relation.swap(true, Relaxed);
        let result = self.overload_matches_inner(sig, arg_types);
        self.strict_nullish_relation.store(prev, Relaxed);
        result
    }

    fn overload_matches_inner(&self, sig: &FunctionType, arg_types: &[Type]) -> bool {
        let min_params = sig
            .params
            .iter()
            .filter(|(name, _)| !name.starts_with('?') && !name.starts_with("..."))
            .count();
        let has_rest = sig.params.iter().any(|(name, _)| name.starts_with("..."));
        let max_params = if has_rest {
            usize::MAX
        } else {
            sig.params.len()
        };

        if arg_types.len() < min_params || arg_types.len() > max_params {
            return false;
        }

        // Generic overloads: infer the type arguments from the call and check
        // the instantiated parameters; slots still generic stay lenient.
        let effective_params: Vec<(std::string::String, Type)> = if !sig.type_params.is_empty() {
            let widened: Vec<Type> = arg_types
                .iter()
                .map(|t| self.widen_argument_for_inference(t))
                .collect();
            let mut inferred =
                self.infer_type_arguments_for_call(&widened, &sig.params, &sig.type_params);
            Self::apply_type_param_defaults(sig, &mut inferred);
            sig.params
                .iter()
                .map(|(name, ty)| (name.clone(), Self::substitute(ty, &inferred)))
                .collect()
        } else {
            sig.params.clone()
        };

        for (index, arg_ty) in arg_types.iter().enumerate() {
            let Some((param_name, param_ty)) = effective_params
                .get(index)
                .or_else(|| has_rest.then(|| effective_params.last()).flatten())
            else {
                return false;
            };

            let effective_param_ty: Type = if param_name.starts_with("...") {
                match param_ty {
                    Type::Array(elem) => elem.as_ref().clone(),
                    _ => param_ty.clone(),
                }
            } else {
                // Optional param (`p?: T`) accepts `T | undefined`.
                Self::optional_param_type(param_name, param_ty.clone())
            };

            if self.contains_unresolved_type_param(&effective_param_ty) {
                continue;
            }
            // A tuple parameter contextually types an array-literal argument
            // as a tuple; the pre-computed array type can't show that.
            if !sig.type_params.is_empty()
                && matches!(effective_param_ty, Type::Tuple(_))
                && matches!(arg_ty, Type::Array(_))
            {
                continue;
            }
            if !self.is_assignable_to(arg_ty, &effective_param_ty) {
                return false;
            }
        }
        true
    }

    /// The enclosing class may extend `null`: its heritage is not a plain
    /// (dotted) name, so the base cannot be ruled out as `null`.
    fn enclosing_class_extends_null(&self) -> bool {
        self.enclosing_class_names
            .last()
            .and_then(|name| self.class_info.get(name.as_str()))
            .and_then(|info| info.extends.as_deref())
            .is_none_or(|base| base.is_empty() || base.contains("<expr>"))
    }

    /// TS2660: `super.x` needs a class member or an object-literal method
    /// or accessor as its nearest non-arrow container (tsc's
    /// getSuperContainer / isLegalUsageOfSuperExpression).
    /// A class member's scope: `super` is legal there only when the class
    /// is derived (TS2335 otherwise).
    pub(crate) fn declare_class_member_super_marker(&mut self) {
        // Unknown (e.g. an anonymous class) counts as derived: no TS2335.
        let derived = self.enclosing_class_derived.last().copied().unwrap_or(true);
        let marker = if derived {
            SUPER_PROPERTY_OK_MARKER
        } else {
            SUPER_NON_DERIVED_MARKER
        };
        self.declare_var(marker, Type::Never);
    }

    /// The nearest `super` container marker in scope, if any.
    fn nearest_super_marker(&self) -> Option<&'static str> {
        let mut scope = Some(self.current_scope);
        while let Some(index) = scope {
            let vars = &self.scopes[index].vars;
            for marker in [
                SUPER_PROPERTY_BARRIER_MARKER,
                SUPER_PROPERTY_OK_MARKER,
                SUPER_NON_DERIVED_MARKER,
            ] {
                if vars.contains_key(marker) {
                    return Some(marker);
                }
            }
            scope = self.scopes[index].parent;
        }
        None
    }

    /// TS2335: `super` in a member of a class without `extends`; TS2466
    /// when it sits directly in a computed property name.
    pub(crate) fn check_super_in_non_derived_class(&mut self, span: Span) -> bool {
        // In a computed key `super` resolves through an enclosing method;
        // with none it is TS2466.
        if self.computed_name_depth == Some(self.fn_nesting_depth)
            && self.nearest_super_marker().is_none()
        {
            if self
                .reported_duplicate_spans
                .insert((2466, span.start, span.end))
            {
                self.diagnostics.push(Diagnostic {
                    code: 2466,
                    message: "'super' cannot be referenced in a computed property name."
                        .to_string(),
                    category: DiagnosticCategory::Error,
                    file_name: None,
                    span: Some(span),
                    related: None,
                });
            }
            return true;
        }
        if self.nearest_super_marker() != Some(SUPER_NON_DERIVED_MARKER) {
            return false;
        }
        if self
            .reported_duplicate_spans
            .insert((2335, span.start, span.end))
        {
            self.diagnostics.push(Diagnostic {
                code: 2335,
                message: "'super' can only be referenced in a derived class.".to_string(),
                category: DiagnosticCategory::Error,
                file_name: None,
                span: Some(span),
                related: None,
            });
        }
        true
    }

    fn check_super_property_container(&mut self, span: Span) {
        if self.check_super_in_non_derived_class(span) {
            return;
        }
        let illegal =
            match self.nearer_binding_is(SUPER_PROPERTY_BARRIER_MARKER, SUPER_PROPERTY_OK_MARKER) {
                Some(barrier) => barrier,
                None => self.enclosing_class_names.is_empty(),
            };
        if illegal {
            self.diagnostics.push(Diagnostic {
                code: 2660,
                message: "'super' can only be referenced in members of derived classes or object literal expressions.".to_string(),
                category: DiagnosticCategory::Error,
                file_name: None,
                span: Some(span),
                related: None,
            });
        }
    }

    /// TS2339 for an object destructuring assignment target property that
    /// the source object does not have. A property with a default may be
    /// missing. Only plain object types without an index signature are
    /// judged; anything else is left alone.
    fn check_object_destructuring_properties(&mut self, properties: &[ObjLitProp], source: &Type) {
        let Type::ObjectType(info) = source else {
            return;
        };
        if info.index_signature.is_some() {
            return;
        }
        for property in properties {
            let (name, span) = match property {
                ObjLitProp::Shorthand(name, span) => (name.to_string(), *span),
                ObjLitProp::Property(property) if !property.computed => {
                    if matches!(&property.value.kind, ExprKind::Assign(a) if a.op == AssignOp::Assign)
                    {
                        continue;
                    }
                    match &property.key {
                        PropName::Ident(name, span) | PropName::String(name, span) => {
                            (name.to_string(), *span)
                        }
                        _ => continue,
                    }
                }
                _ => continue,
            };
            if !info
                .properties
                .iter()
                .any(|(existing, _)| *existing == name)
            {
                self.diagnostics.push(error_property_not_exist(
                    &name,
                    &source.display_string(),
                    span,
                ));
            }
        }
    }

    /// TS18004: a shorthand property whose name resolves to nothing. The name
    /// is resolved like a bare identifier; tsc words the "cannot find name"
    /// error (TS2304/TS2552) for this position as TS18004.
    fn check_shorthand_name_exists(&mut self, name: &str, span: Span) {
        if self.lookup_var(name).is_some()
            || crate::strict_reserved::is_reserved_word(name)
            // A type-only import reports TS1361 instead.
            || self.file_import_binding_names.contains(name)
            || self.imported_type_sources.contains_key(name)
            || self.import_equals_names.contains(name)
        {
            return;
        }
        // Parser recovery can leave a "shorthand" that tsc parses as
        // `name: <missing>` (TS1005 ':' expected); a real shorthand is
        // followed by `,`, `}`, `=`, `;`, a comment or a line break.
        let followed_by = self
            .current_source
            .as_deref()
            .and_then(|text| text.get(span.end as usize..))
            .map(|rest| rest.trim_start_matches([' ', '\t']).chars().next());
        if !matches!(
            followed_by,
            Some(None | Some(',' | '}' | '=' | ';' | '/' | '\n' | '\r'))
        ) {
            return;
        }
        let before = self.diagnostics.len();
        let reference = Expr {
            kind: ExprKind::Ident(name.into()),
            span,
        };
        let saved_depth = self.assignment_pattern_depth;
        self.assignment_pattern_depth = 0;
        self.check_expr(&reference);
        self.assignment_pattern_depth = saved_depth;
        for diagnostic in &mut self.diagnostics[before..] {
            if matches!(diagnostic.code, 2304 | 2552) && diagnostic.span == Some(span) {
                diagnostic.code = 18004;
                diagnostic.message = format!(
                    "No value exists in scope for the shorthand property '{name}'. Either declare one or provide an initializer."
                );
                diagnostic.related = None;
            }
        }
    }

    /// The JSX factory namespace for `jsx: react`: a file `@jsx` pragma,
    /// else a valid `jsxFactory` (or `reactNamespace`), else `React`.
    fn jsx_factory_namespace(&self) -> std::string::String {
        let is_identifier = |text: &str| {
            let mut chars = text.chars();
            chars
                .next()
                .is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$')
                && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
        };
        if let Some(source) = self.current_source.as_deref() {
            let mut rest = source;
            while let Some(at) = rest.find("@jsx") {
                let after = &rest[at + 4..];
                if after.starts_with(|c: char| c == ' ' || c == '\t') {
                    let value: std::string::String = after
                        .trim_start()
                        .chars()
                        .take_while(|c| !c.is_whitespace() && *c != '*')
                        .collect();
                    if let Some(root) = value.split('.').next().filter(|r| !r.is_empty()) {
                        return root.to_string();
                    }
                }
                rest = after;
            }
        }
        if let Some(factory) = self.compiler_options.jsx_factory.as_deref() {
            if factory.split('.').all(is_identifier) {
                return factory.split('.').next().unwrap_or("React").to_string();
            }
            // `reactNamespace` is taken as written, even when invalid.
            if let Some(namespace) = factory.strip_suffix(".createElement") {
                if !namespace.contains('.') {
                    return namespace.to_string();
                }
            }
        }
        "React".to_string()
    }

    /// TS2874: under `jsx: react` every element needs the factory namespace
    /// as a value in scope (tsc's markJsxAliasReferenced), reported at the
    /// tag name in place of "cannot find name".
    fn check_jsx_factory_in_scope(&mut self, tag: &Expr) {
        if !matches!(self.compiler_options.jsx, Some(tsc_rs_ast::JsxEmit::React)) {
            return;
        }
        let namespace = self.jsx_factory_namespace();
        if self.lookup_var(&namespace).is_some()
            || self.file_import_binding_names.contains(namespace.as_str())
            || self.user_global_value_names.contains(namespace.as_str())
            || self.current_file_is_js()
            // Globals we do not model: `declare global` blocks and libraries
            // pulled in by reference (React's UMD global).
            || self.current_file_has_lib_reference
            || self.current_source.as_deref().is_some_and(|text| {
                text.contains("declare global")
                    || text.contains("@jsxRuntime automatic")
                    || text.contains("@jsxImportSource")
            })
        {
            return;
        }
        if !self
            .reported_duplicate_spans
            .insert((2874, tag.span.start, tag.span.end))
        {
            return;
        }
        // A close visible name turns it into tsc's spelling suggestion.
        if let Some(suggestion) = self.attempt_spelling_suggestion(&namespace) {
            let related = self.declared_here_note(&suggestion);
            self.diagnostics.push(Diagnostic {
                code: 2552,
                message: format!("Cannot find name '{namespace}'. Did you mean '{suggestion}'?"),
                category: DiagnosticCategory::Error,
                file_name: None,
                span: Some(tag.span),
                related,
            });
            return;
        }
        self.diagnostics.push(Diagnostic {
            code: 2874,
            message: format!(
                "This JSX tag requires '{namespace}' to be in scope, but it could not be found."
            ),
            category: DiagnosticCategory::Error,
            file_name: None,
            span: Some(tag.span),
            related: None,
        });
    }

    pub(crate) fn check_class_member(&mut self, member: &ClassMember) {
        self.check_class_member_with_siblings(member, &[], false)
    }

    pub(crate) fn check_class_member_with_siblings(
        &mut self,
        member: &ClassMember,
        siblings: &[ClassMember],
        class_is_expression: bool,
    ) {
        // Top-level `var`/`let`/`const` names declared in the sibling
        // constructor's body — an instance initializer that references one is
        // TS2301, the same as referencing a constructor parameter.
        let ctor_local_names: Vec<std::string::String> = siblings
            .iter()
            .filter_map(|m| match &m.kind {
                ClassMemberKind::Constructor(c) => Some(c),
                _ => None,
            })
            .flat_map(|c| Self::constructor_local_names(&c.params, c.body.as_deref()))
            .collect();
        // With [[Define]] semantics (useDefineForClassFields on an ES2022+
        // target) initializers no longer run inside the constructor body,
        // so constructor locals cannot capture them (TS2301 does not apply).
        let target = self.compiler_options.target.unwrap_or(ScriptTarget::ES5);
        let define_fields = self
            .compiler_options
            .use_define_for_class_fields
            .unwrap_or(target >= ScriptTarget::ES2022);
        let ctor_locals_capture_initializers = !(define_fields && target >= ScriptTarget::ES2022);
        match &member.kind {
            ClassMemberKind::Property(prop) => {
                let legacy_decorators = self.compiler_options.experimental_decorators == Some(true);
                let has_private_name = matches!(prop.name, PropName::Private(_, _));
                let valid_decorators = if legacy_decorators {
                    !class_is_expression && !has_private_name
                } else {
                    prop.modifiers & (MOD_ABSTRACT | MOD_DECLARE) == 0
                };
                self.check_decorators_with_validity(&prop.decorators, valid_decorators);
                // A class member's computed name has its own control-flow
                // container (tsc), so outer variables are assumed assigned.
                self.fn_nesting_depth += 1;
                // `[K in T]: V` is tsc's malformed mapped type (TS7061).
                let mapped_type_shape = matches!(
                    &prop.name,
                    PropName::Computed(key, _)
                        if matches!(&key.kind, ExprKind::Binary(b) if b.op == BinaryOp::In)
                );
                self.check_property_name_expression_in(&prop.name, mapped_type_shape);
                self.fn_nesting_depth -= 1;
                if let Some(ref init) = prop.initializer {
                    // Undecorated ambient properties are checked by the parser.
                    // Decorated properties require the selected decorator mode:
                    // an invalid decorator takes precedence over the initializer.
                    if !prop.decorators.is_empty()
                        && valid_decorators
                        && self.check_expression_grammar
                        && (self.ambient_depth > 0 || prop.modifiers & MOD_DECLARE != 0)
                        && (prop.modifiers & tsc_rs_ast::MOD_READONLY == 0
                            || prop.type_ann.is_some())
                    {
                        self.diagnostics.push(Diagnostic {
                            code: 1039,
                            message: "Initializers are not allowed in ambient contexts.".into(),
                            category: DiagnosticCategory::Error,
                            file_name: None,
                            span: Some(init.span),
                            related: None,
                        });
                    }
                    // TS2815: `arguments` is illegal in a property initializer —
                    // mark the initializer scope (function-likes inside it
                    // shadow the marker with their own `arguments` binding).
                    self.push_scope();
                    let initializer_scope = self.current_scope;
                    self.declare_var(ARGS_BLOCKED_MARKER, Type::Never);
                    self.declare_class_member_super_marker();
                    if (prop.modifiers & MOD_STATIC) != 0 {
                        self.declare_var(STATIC_THIS_MARKER, Type::Never);
                    }
                    // TS2715: an instance property initializer runs during
                    // class initialization — abstract `this.prop` access here
                    // is an error (arrows exempt via the depth pairing).
                    let saved_ctx = self.class_init_ctx;
                    // Constructor params are visible to the checker here but
                    // illegal to reference (TS2301). Record them so the
                    // identifier path reports that instead of "cannot find
                    // name".
                    let saved_ctor_params = self.ctor_params_in_initializer.take();
                    if (prop.modifiers & MOD_STATIC) == 0
                        && ctor_locals_capture_initializers
                        && !ctor_local_names.is_empty()
                    {
                        let member = Self::propname_text_opt(&prop.name).unwrap_or_default();
                        self.ctor_params_in_initializer =
                            Some((member, ctor_local_names.clone(), initializer_scope));
                    }
                    let saved_pending_props = self.instance_initializer_pending_props.take();
                    if (prop.modifiers & MOD_STATIC) == 0 {
                        self.class_init_ctx = Some(self.fn_nesting_depth);
                        // TS2729: own instance properties not yet initialized
                        // when this initializer runs.
                        let mut pending: rustc_hash::FxHashMap<std::string::String, Span> =
                            rustc_hash::FxHashMap::default();
                        for sibling in siblings {
                            let ClassMemberKind::Property(other) = &sibling.kind else {
                                continue;
                            };
                            if sibling.span == member.span
                                || other.modifiers & MOD_STATIC != 0
                                || other.optional
                            {
                                continue;
                            }
                            let declared_later = sibling.span.start > member.span.start;
                            let uninitialized = other.initializer.is_none() && !other.definite;
                            if declared_later || uninitialized {
                                if let Some(name) = Self::propname_text_opt(&other.name) {
                                    pending.insert(name, other.name.span());
                                }
                            }
                        }
                        self.instance_initializer_pending_props = Some(pending);
                    }
                    // TS1308: a property initializer is a non-async context.
                    self.return_is_async_stack.push(false);
                    self.generator_stack.push(false);
                    // Pass type annotation as contextual type for function/arrow initializers
                    let decl_ty = prop
                        .type_ann
                        .as_ref()
                        .map(|ann| self.resolve_type_node(ann));
                    let has_own_tp = match &init.kind {
                        ExprKind::Arrow(a) => {
                            a.type_params.as_ref().map_or(false, |tp| !tp.is_empty())
                        }
                        ExprKind::FnExpr(f) => {
                            f.type_params.as_ref().map_or(false, |tp| !tp.is_empty())
                        }
                        _ => false,
                    };
                    let init_ty = if decl_ty.is_some()
                        && !has_own_tp
                        && matches!(
                            init.kind,
                            ExprKind::Arrow(_)
                                | ExprKind::FnExpr(_)
                                | ExprKind::Call(_)
                                | ExprKind::New(_)
                                | ExprKind::Template(_)
                        ) {
                        self.check_expr_contextual(init, decl_ty.as_ref())
                    } else {
                        self.check_expr(init)
                    };
                    if let Some(dt) = decl_ty.clone() {
                        let assignable = self.is_assignable_to(&init_ty, &dt);
                        // tsc: elaborate into a fresh literal / arrow body
                        // first, then TS2353, then the plain TS2322.
                        let mut reported = false;
                        if !assignable {
                            let origin = prop.type_ann.as_ref().map(ExpectedOrigin::Node);
                            reported = self.elaborate_error(init, &init_ty, &dt, origin);
                        }
                        if !reported
                            && (Self::is_fresh_object_literal(init)
                                || matches!(init.kind, ExprKind::ArrayLit(_)))
                        {
                            let before = self.diagnostics.len();
                            self.check_excess_properties(&init_ty, &dt, member.span, Some(init));
                            reported = self.diagnostics.len() > before;
                        }
                        if !reported && !assignable {
                            let widened_init = self.widen_for_message(Some(init), &init_ty, &dt);
                            self.message_source_is_fresh_object_literal =
                                Self::is_fresh_object_literal(init);
                            // tsc marks the property NAME for an initializer mismatch.
                            let name_span = prop.name.span();
                            self.push_not_assignable(&widened_init, &dt, name_span);
                            self.message_source_is_fresh_object_literal = false;
                        }
                    }
                    self.return_is_async_stack.pop();
                    self.generator_stack.pop();
                    self.ctor_params_in_initializer = saved_ctor_params;
                    self.class_init_ctx = saved_ctx;
                    self.instance_initializer_pending_props = saved_pending_props;
                    self.pop_scope();
                }
            }
            ClassMemberKind::Method(method) => {
                let legacy_decorators = self.compiler_options.experimental_decorators == Some(true);
                let has_private_name = matches!(method.name, PropName::Private(_, _));
                if method.body.is_some() {
                    self.check_decorators_with_validity(
                        &method.decorators,
                        (!legacy_decorators || !class_is_expression)
                            && (!legacy_decorators || !has_private_name),
                    );
                } else {
                    self.report_decorator_on_method_overload(&method.decorators);
                }
                // A class member's computed name has its own control-flow
                // container (tsc), so outer variables are assumed assigned.
                self.fn_nesting_depth += 1;
                self.check_property_name_expression(&method.name);
                self.fn_nesting_depth -= 1;
                let method_name = Self::propname_text_opt(&method.name);
                let has_runtime_implementation = method.body.is_some()
                    || siblings.iter().any(|sibling| {
                        matches!(
                            &sibling.kind,
                            ClassMemberKind::Method(candidate)
                                if candidate.body.is_some()
                                    && Self::propname_text_opt(&candidate.name) == method_name
                        )
                    });
                self.check_parameter_decorators(
                    &method.params,
                    legacy_decorators && !class_is_expression && method.body.is_some(),
                );
                self.check_parameter_runtime_expressions(
                    &method.params,
                    has_runtime_implementation,
                    false,
                );
                // Method bodies are NOT class-initialization code (TS2715).
                let saved_ctx = self.class_init_ctx.take();
                self.push_scope();
                self.fn_nesting_depth += 1;
                self.jump_function_depth += 1;
                let saved_var_first_types = std::mem::take(&mut self.var_first_types);
                self.declare_var("arguments", Type::Any);
                self.declare_class_member_super_marker();
                self.declare_var(THIS_OK_MARKER, Type::Never);
                self.bind_function_this(&method.params);
                if method.modifiers & MOD_STATIC == 0 {
                    self.declare_instance_member_marker();
                }
                // TS2369: check for parameter properties in non-constructor methods
                for param in &method.params {
                    self.check_parameter_property(param);
                    // TS1210: class bodies are strict mode (ambient exempt).
                    if self.ambient_depth == 0 {
                        if let PatKind::Ident(n) = &param.name.kind {
                            if n == "arguments" || n == "eval" {
                                self.diagnostics
                                    .push(error_invalid_strict_name(n, param.name.span));
                            }
                        }
                    }
                }
                for param in &method.params {
                    let pty = param
                        .type_ann
                        .as_ref()
                        .map(|t| self.resolve_type_node(t))
                        .or_else(|| {
                            param
                                .initializer
                                .as_ref()
                                .map(|init| self.infer_expr_type(init))
                        })
                        .unwrap_or(Type::Any);
                    self.declare_pattern_vars(&param.name, pty);
                    self.completion_mark_parameter(param);
                }
                // Push declared return type for TS2322 checking in return statements
                let declared_ret = method
                    .return_type
                    .as_ref()
                    .map(|rt| self.resolve_type_node(rt));
                let declared_ret = self.unwrap_async_return(declared_ret, method.is_async);
                self.return_type_stack.push(declared_ret);
                self.return_is_async_stack.push(method.is_async);
                self.generator_stack.push(method.is_generator);
                if let Some(ref body) = method.body {
                    self.hoist_block_declarations(body);
                    if self.ambient_depth == 0 {
                        self.scan_strict_name_vars(body);
                    }
                    for s in body {
                        self.check_stmt(s);
                    }
                    self.check_function_completion(
                        body,
                        method.return_type.as_ref(),
                        method.name.span(),
                        method.is_async,
                        method.is_generator,
                    );
                }
                self.return_type_stack.pop();
                self.return_is_async_stack.pop();
                self.generator_stack.pop();
                self.var_first_types = saved_var_first_types;
                self.fn_nesting_depth -= 1;
                self.jump_function_depth -= 1;
                self.pop_scope();
                self.class_init_ctx = saved_ctx;
            }
            ClassMemberKind::Constructor(ctor) => {
                let legacy_decorators = self.compiler_options.experimental_decorators == Some(true);
                self.report_invalid_decorators(&ctor.decorators);
                let has_runtime_implementation = ctor.body.is_some()
                    || siblings.iter().any(|sibling| {
                        matches!(
                            &sibling.kind,
                            ClassMemberKind::Constructor(candidate) if candidate.body.is_some()
                        )
                    });
                self.check_parameter_decorators(
                    &ctor.params,
                    legacy_decorators && !class_is_expression && ctor.body.is_some(),
                );
                self.check_parameter_runtime_expressions(
                    &ctor.params,
                    has_runtime_implementation,
                    false,
                );
                // TS2715: the constructor body runs during class
                // initialization — abstract `this.prop` access is an error
                // (arrows inside are exempt via the depth pairing).
                let saved_ctx = self.class_init_ctx;
                let saved_in_ctor = self.in_constructor;
                self.in_constructor = true;
                // Member bodies are their own functions for definite
                // assignment analysis (TS2454) and use-before-init checks.
                self.fn_nesting_depth += 1;
                self.jump_function_depth += 1;
                self.class_init_ctx = Some(self.fn_nesting_depth);
                // TS17009: in a DERIVED class, `this` may not be touched
                // until `super()` runs (marker declared into the ctor scope
                // just after it is pushed below).
                let is_derived = self
                    .enclosing_class_names
                    .last()
                    .and_then(|n| self.class_info.get(n.as_str()))
                    .map(|i| i.extends.is_some())
                    .unwrap_or(false);
                // TS1308: constructors are never async.
                self.return_is_async_stack.push(false);
                self.generator_stack.push(false);
                self.push_scope();
                let saved_var_first_types = std::mem::take(&mut self.var_first_types);
                self.declare_var("arguments", Type::Any);
                self.declare_class_member_super_marker();
                self.declare_var(THIS_OK_MARKER, Type::Never);
                self.bind_function_this(&[]);
                self.declare_instance_member_marker();
                let saved_super_pending_depth = self.super_pending_depth;
                if is_derived {
                    self.declare_var(SUPER_PENDING_MARKER, Type::Never);
                    self.super_pending_depth = Some(self.fn_nesting_depth);
                }
                // TS1210: binding `arguments`/`eval` as a parameter inside a
                // class (class bodies are strict mode). Ambient (`declare`)
                // classes are exempt.
                if self.ambient_depth == 0 {
                    for param in &ctor.params {
                        if let PatKind::Ident(n) = &param.name.kind {
                            if n == "arguments" || n == "eval" {
                                self.diagnostics
                                    .push(error_invalid_strict_name(n, param.name.span));
                            }
                        }
                    }
                }
                for param in &ctor.params {
                    let pty = param
                        .type_ann
                        .as_ref()
                        .map(|t| self.resolve_type_node(t))
                        .or_else(|| {
                            param
                                .initializer
                                .as_ref()
                                .map(|init| self.infer_expr_type(init))
                        })
                        .unwrap_or(Type::Any);
                    self.declare_pattern_vars(&param.name, pty);
                    self.completion_mark_parameter(param);
                }
                if let Some(ref body) = ctor.body {
                    self.hoist_block_declarations(body);
                    if self.ambient_depth == 0 {
                        self.scan_strict_name_vars(body);
                    }
                    let saved_own_super = self
                        .ctor_own_super_call
                        .replace((self.fn_nesting_depth, false));
                    for s in body {
                        self.check_stmt(s);
                    }
                    let own_super =
                        std::mem::replace(&mut self.ctor_own_super_call, saved_own_super);
                    // TS2377: a derived class's constructor must call super()
                    // (a class that extends `null` is exempt).
                    if is_derived
                        && own_super == Some((self.fn_nesting_depth, false))
                        && !self.enclosing_class_extends_null()
                    {
                        self.diagnostics.push(Diagnostic {
                            code: 2377,
                            message:
                                "Constructors for derived classes must contain a 'super' call."
                                    .to_string(),
                            category: DiagnosticCategory::Error,
                            file_name: None,
                            span: Some(Span::new(member.span.start, ctor.keyword_span.end)),
                            related: None,
                        });
                    }
                }
                self.var_first_types = saved_var_first_types;
                self.pop_scope();
                self.return_is_async_stack.pop();
                self.generator_stack.pop();
                self.fn_nesting_depth -= 1;
                self.jump_function_depth -= 1;
                self.in_constructor = saved_in_ctor;
                self.super_pending_depth = saved_super_pending_depth;
                self.class_init_ctx = saved_ctx;
            }
            ClassMemberKind::GetAccessor(acc) | ClassMemberKind::SetAccessor(acc) => {
                let legacy_decorators = self.compiler_options.experimental_decorators == Some(true);
                let has_private_name = matches!(acc.name, PropName::Private(_, _));
                self.check_decorators_with_validity(
                    &acc.decorators,
                    acc.body.is_some()
                        && (!legacy_decorators || !class_is_expression)
                        && (!legacy_decorators || !has_private_name),
                );
                // A class member's computed name has its own control-flow
                // container (tsc), so outer variables are assumed assigned.
                self.fn_nesting_depth += 1;
                self.check_property_name_expression(&acc.name);
                self.fn_nesting_depth -= 1;
                let is_setter = matches!(member.kind, ClassMemberKind::SetAccessor(_));
                self.check_parameter_decorators(
                    &acc.params,
                    legacy_decorators && !class_is_expression && is_setter && acc.body.is_some(),
                );
                self.check_parameter_runtime_expressions(&acc.params, acc.body.is_some(), false);
                let is_getter = matches!(member.kind, ClassMemberKind::GetAccessor(_));
                self.check_accessor_pair_modifiers(acc, is_getter, siblings);
                self.check_accessor_signature_grammar(
                    is_getter,
                    &acc.name,
                    &acc.params,
                    acc.return_type.as_ref(),
                    acc.type_params.is_some(),
                );
                if let Some(body) = &acc.body {
                    if is_getter {
                        self.check_getter_missing_return(&acc.name, body);
                    } else {
                        self.check_setter_value_returns(body);
                    }
                }
                // Accessor pair coherence: an unannotated getter's return is
                // checked against its setter's annotated param type, and an
                // unannotated setter param inherits the getter's return type
                // (accessors_spec_section-4.5 error cases).
                let same_name = |pn: &PropName| -> bool {
                    match (pn, &acc.name) {
                        (PropName::Ident(a, _), PropName::Ident(b, _))
                        | (PropName::String(a, _), PropName::String(b, _)) => a == b,
                        _ => false,
                    }
                };
                let pair_ty: Option<Type> = siblings.iter().find_map(|m| match &m.kind {
                    ClassMemberKind::SetAccessor(other)
                        if is_getter
                            && same_name(&other.name)
                            && (other.modifiers & MOD_STATIC) == (acc.modifiers & MOD_STATIC) =>
                    {
                        other
                            .params
                            .first()
                            .and_then(|p| p.type_ann.as_ref())
                            .map(|t| self.resolve_type_node(t))
                    }
                    ClassMemberKind::GetAccessor(other)
                        if !is_getter
                            && same_name(&other.name)
                            && (other.modifiers & MOD_STATIC) == (acc.modifiers & MOD_STATIC) =>
                    {
                        other
                            .return_type
                            .as_ref()
                            .map(|t| self.resolve_type_node(t))
                    }
                    _ => None,
                });
                // Accessor bodies are NOT class-initialization code (TS2715).
                let saved_ctx = self.class_init_ctx.take();
                // TS1308: accessors are never async.
                self.return_is_async_stack.push(false);
                self.generator_stack.push(false);
                self.push_scope();
                self.fn_nesting_depth += 1;
                self.jump_function_depth += 1;
                let saved_var_first_types = std::mem::take(&mut self.var_first_types);
                self.declare_var("arguments", Type::Any);
                self.declare_class_member_super_marker();
                self.declare_var(THIS_OK_MARKER, Type::Never);
                self.bind_function_this(&[]);
                if acc.modifiers & MOD_STATIC == 0 {
                    self.declare_instance_member_marker();
                }
                // TS2369: check for parameter properties in accessors
                for param in &acc.params {
                    self.check_parameter_property(param);
                }
                for param in &acc.params {
                    let pty = param
                        .type_ann
                        .as_ref()
                        .map(|t| self.resolve_type_node(t))
                        .or_else(|| if !is_getter { pair_ty.clone() } else { None })
                        .unwrap_or(Type::Any);
                    self.declare_pattern_vars(&param.name, pty);
                    self.completion_mark_parameter(param);
                }
                // Getters: expected return type = own annotation, else the
                // paired setter's param type.
                let expected_ret = if is_getter {
                    acc.return_type
                        .as_ref()
                        .map(|t| self.resolve_type_node(t))
                        .or_else(|| pair_ty.clone())
                } else {
                    None
                };
                self.return_type_stack.push(expected_ret);
                if let Some(ref body) = acc.body {
                    self.hoist_block_declarations(body);
                    for s in body {
                        self.check_stmt(s);
                    }
                    if is_getter {
                        self.check_function_completion(
                            body,
                            acc.return_type.as_ref(),
                            acc.name.span(),
                            false,
                            false,
                        );
                    }
                }
                self.return_type_stack.pop();
                self.var_first_types = saved_var_first_types;
                self.fn_nesting_depth -= 1;
                self.jump_function_depth -= 1;
                self.pop_scope();
                self.return_is_async_stack.pop();
                self.generator_stack.pop();
                self.class_init_ctx = saved_ctx;
            }
            ClassMemberKind::StaticBlock(stmts) => {
                self.report_js_checked_decorated_static_block(member.span);
                // TS2815: `arguments` is illegal in a static initialization
                // block; TS2715's instance-init context does not apply.
                let saved_ctx = self.class_init_ctx.take();
                // TS1308: a static block is a non-async context.
                self.return_is_async_stack.push(false);
                self.generator_stack.push(false);
                self.push_scope();
                let saved_var_first_types = std::mem::take(&mut self.var_first_types);
                self.declare_var(ARGS_BLOCKED_MARKER, Type::Never);
                self.declare_class_member_super_marker();
                self.declare_var(STATIC_THIS_MARKER, Type::Never);
                for s in stmts {
                    self.check_stmt(s);
                }
                self.var_first_types = saved_var_first_types;
                self.pop_scope();
                self.return_is_async_stack.pop();
                self.generator_stack.pop();
                self.class_init_ctx = saved_ctx;
            }
            ClassMemberKind::IndexSignature(signature) => {
                self.check_index_signature_type_grammar(signature, member.span);
            }
            ClassMemberKind::SemicolonClassElement => {}
        }
    }
}
