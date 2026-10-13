//! Declaration names visited by the TypeScript type-baseline walker.

use super::*;

#[derive(Debug, Clone)]
pub(super) struct MethodDeclaration {
    pub name: String,
    pub file: String,
    pub span: Span,
    pub ty: Type,
}

impl TypeChecker {
    pub(crate) fn record_class_member_declaration_types(
        &mut self,
        declaration: &ClassDecl,
        info: &ClassInfo,
    ) {
        if self.expression_type_spans.is_none() {
            return;
        }
        for member in &declaration.members {
            let (name, flags) = match &member.kind {
                ClassMemberKind::Method(method) => (&method.name, method.modifiers),
                ClassMemberKind::Property(property) => (&property.name, property.modifiers),
                ClassMemberKind::GetAccessor(accessor) | ClassMemberKind::SetAccessor(accessor) => {
                    (&accessor.name, accessor.modifiers)
                }
                _ => continue,
            };
            let span = name.span();
            // Recovery can turn a statement keyword into an empty property;
            // the TypeScript walker does not visit that token as a name.
            if let ClassMemberKind::Property(property) = &member.kind {
                if property.type_ann.is_none()
                    && property.initializer.is_none()
                    && self.syntax_error_starts.binary_search(&span.start).is_ok()
                    && matches!(name, PropName::Ident(name, _)
                        if tsc_rs_scanner::TokenKind::keyword_from_str(name).is_keyword())
                {
                    continue;
                }
            }
            let key = self.prop_name_to_string(name);
            let (properties, methods, overloads, optional) = if flags & MOD_STATIC != 0 {
                (
                    &info.static_properties,
                    &info.static_methods,
                    &info.overloaded_static_methods,
                    &info.optional_static_properties,
                )
            } else {
                (
                    &info.instance_properties,
                    &info.instance_methods,
                    &info.overloaded_methods,
                    &info.optional_instance_properties,
                )
            };
            // All declarations of an overloaded method expose the complete
            // callable set, including its implementation's name token.
            let property = properties
                .iter()
                .find(|(name, _)| name == &key)
                .map(|(_, ty)| ty.clone());
            let callable = overloads.get(&key).cloned().or_else(|| {
                methods
                    .iter()
                    .find(|(name, _)| name == &key)
                    .map(|(_, signature)| Type::Function(signature.clone()))
            });
            // Invalid method/accessor conflicts retain separate declaration
            // symbols; the accessor must not replace the method's name type.
            let ty = if matches!(member.kind, ClassMemberKind::Method(_)) {
                callable.or(property)
            } else {
                property.or(callable)
            }
            .unwrap_or(Type::Any);
            let ty = if optional.contains(&key) && self.strict_null_checks {
                Type::flatten_union(vec![ty, Type::Undefined])
            } else {
                ty
            };
            self.record_expression_type_span(span, &ty);
        }
    }

    pub(crate) fn record_method_signature_types(
        &mut self,
        members: &[TypeMember],
        registered: Option<Vec<(String, Arc<Type>)>>,
    ) {
        if self.expression_type_spans.is_none()
            || !members
                .iter()
                .any(|member| matches!(member.kind, TypeMemberKind::MethodSig(_)))
        {
            return;
        }
        // Interfaces use their registered shape so merged declarations share
        // the complete overload set. A type literal owns its own members.
        let properties = registered.or_else(|| {
            let node = TypeNode {
                kind: TypeNodeKind::TypeLit(members.to_vec()),
                span: Span::default(),
            };
            match self.resolve_type_node(&node) {
                Type::ObjectType(object) => Some(object.properties.clone()),
                _ => None,
            }
        });
        let Some(properties) = properties else {
            return;
        };
        for member in members {
            let TypeMemberKind::MethodSig(method) = &member.kind else {
                continue;
            };
            let span = method.name.span();
            let after_comment = self
                .expression_type_comments
                .partition_point(|comment| comment.start <= span.start);
            if after_comment > 0
                && self.expression_type_comments[after_comment - 1].end > span.start
            {
                continue;
            }
            let key = self.prop_name_to_string(&method.name);
            let signatures: Vec<_> = properties
                .iter()
                .filter(|(name, _)| name == &key)
                .map(|(_, ty)| match ty.as_ref() {
                    Type::Optional(inner) => Type::clone(inner),
                    _ => Type::clone(ty),
                })
                .collect();
            let Some(first) = signatures.first() else {
                continue;
            };
            // Declaration identity matters here: equal signatures from two
            // distinct overload declarations both appear in the writer.
            let ty = if signatures.len() > 1
                && signatures.iter().all(|ty| matches!(ty, Type::Function(_)))
            {
                Type::Intersection(signatures.into())
            } else {
                first.clone()
            };
            let ty = if method.optional && self.strict_null_checks {
                Type::flatten_union(vec![ty, Type::Undefined])
            } else {
                ty
            };
            self.record_expression_type_span(span, &ty);
        }
    }
}
