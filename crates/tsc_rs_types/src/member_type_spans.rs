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
