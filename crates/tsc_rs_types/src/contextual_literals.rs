// Adapted from pingdotgg/ts-rust 22ae6fa87aed1d4bde7272ab68452fae1f928e4a:
// checker_p28.rs literal classification/widening and checker_p30.rs maybe_type_of_kind.
// Original: Microsoft TypeScript 673a5f17d713bdc8c7185f18a9c11e3c4ac5d781,
// checker.go isLiteralOfContextualType / maybeTypeOfKind /
// getWidenedLiteralType / getWidenedUniqueESSymbolType /
// getWidenedLiteralLikeTypeForContextualType.
// Changes: native Type variants replace flags and arena IDs; aliases, enums,
// optional wrappers and lexical constraints are resolved by the native adapter.
// Copyright (c) Microsoft Corporation. Copyright (c) 2026 T3 Tools Inc.
// SPDX-License-Identifier: MIT AND Apache-2.0
// See ../NOTICE.txt and ../licenses/ for the retained notices and licenses.

use super::*;

#[derive(Clone, Copy)]
enum LiteralKind {
    String,
    Number,
    BigInt,
    Boolean,
    Symbol,
}

impl LiteralKind {
    fn literal(self, ty: &Type) -> bool {
        matches!(
            (self, ty),
            (Self::String, Type::StringLiteral(_))
                | (Self::Number, Type::NumberLiteral(_))
                | (Self::BigInt, Type::BigIntLiteral(_))
                | (Self::Boolean, Type::BooleanLiteral(_) | Type::Boolean)
                | (Self::Symbol, Type::UniqueSymbol(_))
        )
    }

    fn primitive(self, ty: &Type) -> bool {
        matches!(
            (self, ty),
            (Self::String, Type::String)
                | (Self::Number, Type::Number)
                | (Self::BigInt, Type::BigInt)
                | (Self::Boolean, Type::Boolean)
                | (Self::Symbol, Type::Symbol)
        )
    }
}

impl TypeChecker {
    /// These initializers already checked their array elements or read a
    /// regular array type. Generic call inference can still return fresh
    /// element types and must run its existing declaration widening path.
    pub(super) fn has_checked_array_elements(initializer: &Expr) -> bool {
        matches!(
            Self::skip_reference_wrappers(initializer).kind,
            ExprKind::ArrayLit(_)
                | ExprKind::Ident(_)
                | ExprKind::Member(_)
                | ExprKind::ElemAccess(_)
        )
    }

    /// Adapted getWidenedLiteralType. The caller supplies the occurrence's
    /// freshness; native literal values themselves are structurally interned.
    pub(super) fn widened_literal_type(&self, ty: Type) -> Type {
        match ty {
            Type::Union(members) => Type::flatten_union(
                members
                    .iter()
                    .cloned()
                    .map(|member| self.widened_literal_type(member))
                    .collect(),
            ),
            Type::NumberLiteral(_)
            | Type::StringLiteral(_)
            | Type::BigIntLiteral(_)
            | Type::BooleanLiteral(_)
            | Type::EnumVariant { .. } => self.widen_type(&ty),
            other => other,
        }
    }

    /// Adapted getWidenedUniqueESSymbolType. This applies to the value in a
    /// mutable location, not recursively to members of an already typed object.
    pub(super) fn widened_unique_symbol_type(ty: Type) -> Type {
        match ty {
            Type::UniqueSymbol(_) => Type::Symbol,
            Type::Union(members) => Type::flatten_union(
                members
                    .iter()
                    .cloned()
                    .map(Self::widened_unique_symbol_type)
                    .collect(),
            ),
            other => other,
        }
    }

    /// Adapted getWidenedLiteralLikeTypeForContextualType. Unlike the upstream
    /// arena, a native Type has no fresh/regular wrapper to strip at the end.
    pub(super) fn contextual_literal_type(
        &self,
        ty: Type,
        context: Option<&Type>,
        fresh: bool,
    ) -> Type {
        if context.is_some_and(|context| self.literal_of_contextual_type(&ty, context)) {
            return ty;
        }
        let ty = if fresh {
            self.widened_literal_type(ty)
        } else {
            ty
        };
        Self::widened_unique_symbol_type(ty)
    }

    /// Upstream maybeTypeOfKind: any union/intersection constituent can
    /// supply the kind. In particular, the candidate need not be one literal.
    fn maybe_literal_kind(ty: &Type, kind: LiteralKind, primitive: bool) -> bool {
        if if primitive {
            kind.primitive(ty)
        } else {
            kind.literal(ty)
        } {
            return true;
        }
        match ty {
            Type::Union(members) | Type::Intersection(members) => members
                .iter()
                .any(|member| Self::maybe_literal_kind(member, kind, primitive)),
            Type::EnumVariant {
                value: Some(value), ..
            } => Self::maybe_literal_kind(value, kind, primitive),
            Type::EnumType(info) => info
                .members
                .iter()
                .any(|(_, member)| Self::maybe_literal_kind(member, kind, primitive)),
            _ => false,
        }
    }

    pub(crate) fn literal_of_contextual_type(&self, candidate: &Type, context: &Type) -> bool {
        self.literal_of_contextual_type_inner(candidate, context, &mut Vec::new())
    }

    fn literal_of_contextual_type_inner(
        &self,
        candidate: &Type,
        context: &Type,
        resolving: &mut Vec<Type>,
    ) -> bool {
        match context {
            Type::Union(members) | Type::Intersection(members) => members
                .iter()
                .any(|member| self.literal_of_contextual_type_inner(candidate, member, resolving)),
            Type::Optional(inner) | Type::Readonly(inner) => {
                self.literal_of_contextual_type_inner(candidate, inner, resolving)
            }
            Type::TypeParameter(_) | Type::TypeReference(_, _) => {
                // Upstream's types already contain resolved aliases and enum
                // unions. Resolve those native representations at the boundary.
                if resolving.len() >= MAX_RECURSION_DEPTH as usize || resolving.contains(context) {
                    return false;
                }
                resolving.push(context.clone());
                let constraint = self.contextual_literal_constraint(context);
                let result = if let Some(constraint) = constraint {
                    // Primitive constraints create literal contexts even when
                    // the primitive type by itself would widen a literal.
                    [
                        LiteralKind::String,
                        LiteralKind::Number,
                        LiteralKind::BigInt,
                        LiteralKind::Symbol,
                    ]
                    .into_iter()
                    .any(|kind| {
                        Self::maybe_literal_kind(&constraint, kind, true)
                            && Self::maybe_literal_kind(candidate, kind, false)
                    }) || self.literal_of_contextual_type_inner(candidate, &constraint, resolving)
                } else if let Type::TypeReference(name, _) = context {
                    if self.type_param_name_is_active(name) {
                        false
                    } else if matches!(
                        name.as_str(),
                        "Uppercase" | "Lowercase" | "Capitalize" | "Uncapitalize"
                    ) && !self.type_aliases.contains_key(name.as_str())
                    {
                        Self::maybe_literal_kind(candidate, LiteralKind::String, false)
                    } else if let Some(values) = self.enum_target_member_values(name) {
                        values.iter().any(|value| {
                            self.literal_of_contextual_type_inner(candidate, value, resolving)
                        })
                    } else {
                        self.resolve_type_for_assignability(context)
                            .filter(|resolved| resolved != context)
                            .is_some_and(|resolved| {
                                self.literal_of_contextual_type_inner(
                                    candidate, &resolved, resolving,
                                )
                            })
                    }
                } else {
                    false
                };
                resolving.pop();
                result
            }
            Type::EnumType(info) => info.members.iter().any(|(_, value)| {
                self.literal_of_contextual_type_inner(candidate, value, resolving)
            }),
            Type::EnumVariant {
                value: Some(value), ..
            } => self.literal_of_contextual_type_inner(candidate, value, resolving),
            // TypeScript's boolean primitive is the union false | true.
            Type::Boolean => Self::maybe_literal_kind(candidate, LiteralKind::Boolean, false),
            Type::StringLiteral(_)
            | Type::Keyof(_)
            | Type::TemplateLiteral { .. }
            | Type::StringMapping { .. } => {
                Self::maybe_literal_kind(candidate, LiteralKind::String, false)
            }
            Type::NumberLiteral(_) => {
                Self::maybe_literal_kind(candidate, LiteralKind::Number, false)
            }
            Type::BigIntLiteral(_) => {
                Self::maybe_literal_kind(candidate, LiteralKind::BigInt, false)
            }
            Type::BooleanLiteral(_) => {
                Self::maybe_literal_kind(candidate, LiteralKind::Boolean, false)
            }
            Type::UniqueSymbol(_) => {
                Self::maybe_literal_kind(candidate, LiteralKind::Symbol, false)
            }
            Type::Typeof(name) => self.lookup_var(name).is_some_and(|resolved| {
                matches!(resolved, Type::UniqueSymbol(_))
                    && Self::maybe_literal_kind(candidate, LiteralKind::Symbol, false)
            }),
            _ => false,
        }
    }

    /// Adapt the native lexical constraint tables to the upstream base
    /// constraint lookup. A cycle has no usable constraint; a long acyclic
    /// chain must not be truncated after an arbitrary number of parameters.
    pub(super) fn contextual_literal_constraint(&self, ty: &Type) -> Option<Type> {
        let mut current = ty.clone();
        let mut visited = HashSet::new();
        loop {
            let name = match &current {
                Type::TypeParameter(name) => name,
                Type::TypeReference(name, args) if args.is_empty() => name,
                _ => return (current != *ty).then_some(current),
            };
            if !visited.insert(name.clone()) {
                return None;
            }
            let constraint = if let Some(constraint) = tuple_bindings::inferred_constraint(&current)
            {
                constraint
            } else {
                self.type_param_constraint_stack
                    .iter()
                    .rev()
                    .find_map(|scope| scope.get(name.as_str()).cloned())
            };
            match constraint {
                Some(constraint) => current = constraint,
                None => return (current != *ty).then_some(current),
            }
        }
    }

    pub(super) fn contextual_key_constraint(&self, ty: &Type) -> Option<Type> {
        let Type::Keyof(owner) = ty else { return None };
        self.active_type_parameter_name(owner)?;
        let constraint = self.contextual_literal_constraint(owner)?;
        let constraint = self
            .resolve_type_for_assignability(&constraint)
            .unwrap_or(constraint);
        Some(Self::keyof_of(constraint))
    }
}
