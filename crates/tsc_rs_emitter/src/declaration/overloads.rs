use super::*;

impl DeclarationEmitter<'_> {
    fn statement_function(statement: &Stmt) -> Option<&FnDecl> {
        match &statement.kind {
            StmtKind::FnDecl(function) => Some(function),
            StmtKind::Export(export) => match &export.kind {
                ExportDeclKind::Decl(inner) | ExportDeclKind::DefaultDecl(inner) => {
                    Self::statement_function(inner)
                }
                _ => None,
            },
            _ => None,
        }
    }

    pub(super) fn is_function_overload_implementation(statement: &Stmt, siblings: &[Stmt]) -> bool {
        let Some(function) = Self::statement_function(statement) else {
            return false;
        };
        function.body.is_some()
            && siblings
                .iter()
                .filter_map(Self::statement_function)
                .any(|other| other.body.is_none() && other.name == function.name)
    }

    pub(super) fn is_class_overload_implementation(
        &self,
        member: &ClassMember,
        siblings: &[ClassMember],
    ) -> bool {
        match &member.kind {
            ClassMemberKind::Constructor(constructor) if constructor.body.is_some() => {
                siblings.iter().any(|other| {
                    matches!(
                        &other.kind,
                        ClassMemberKind::Constructor(other) if other.body.is_none()
                    )
                })
            }
            ClassMemberKind::Method(method) if method.body.is_some() => {
                let key = self.overload_property_key(&method.name);
                siblings.iter().any(|other| match &other.kind {
                    ClassMemberKind::Method(other) => {
                        other.body.is_none()
                            && other.modifiers & MOD_STATIC == method.modifiers & MOD_STATIC
                            && self.overload_property_key(&other.name) == key
                    }
                    _ => false,
                })
            }
            _ => false,
        }
    }

    pub(super) fn overload_property_key(&self, name: &PropName) -> (u8, String) {
        match name {
            PropName::Ident(name, _) | PropName::Number(name, _) => (0, name.to_string()),
            PropName::String(name, _) => (0, crate::emit_class::decode_js_string_content(name)),
            PropName::Private(name, _) => (1, name.to_string()),
            PropName::Computed(expr, _) => match &expr.kind {
                ExprKind::StrLit(value) => (0, crate::emit_class::decode_js_string_content(value)),
                _ => (
                    2,
                    self.source[expr.span.start as usize..expr.span.end as usize]
                        .trim()
                        .to_string(),
                ),
            },
        }
    }
}
