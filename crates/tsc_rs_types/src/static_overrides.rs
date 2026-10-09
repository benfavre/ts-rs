//! Compatibility of the constructor's static members during class inheritance.
use super::*;

#[derive(Clone)]
struct StaticMember {
    name: String,
    ty: Type,
    modifiers: u32,
    method: bool,
    optional: bool,
}

impl TypeChecker {
    fn static_base_identity(&self, name: &str) -> Option<String> {
        if self.class_info.contains_key(name) {
            return Some(name.to_owned());
        }
        match self.lookup_var(name) {
            Some(Type::TypeReference(reference, _)) => reference
                .strip_prefix("typeof ")
                .filter(|identity| self.class_info.contains_key(*identity))
                .map(str::to_owned),
            _ => None,
        }
    }

    fn own_static_members(&self, identity: &str) -> Vec<StaticMember> {
        let Some(info) = self.class_info.get(identity) else {
            return Vec::new();
        };
        info.static_member_modifiers
            .iter()
            .filter_map(|(name, modifiers)| {
                // Private identifiers in distinct classes do not share an identity.
                if name.starts_with('#') {
                    return None;
                }
                let (ty, method) = if let Some(ty) = info.overloaded_static_methods.get(name) {
                    (ty.clone(), true)
                } else if let Some((_, signature)) =
                    info.static_methods.iter().find(|(n, _)| n == name)
                {
                    (Type::Function(signature.clone()), true)
                } else {
                    (
                        info.static_properties
                            .iter()
                            .find(|(n, _)| n == name)?
                            .1
                            .clone(),
                        false,
                    )
                };
                Some(StaticMember {
                    name: name.clone(),
                    ty,
                    modifiers: *modifiers,
                    method,
                    optional: info.optional_static_properties.contains(name),
                })
            })
            .collect()
    }

    pub(crate) fn member_type_assignable(
        &self,
        source: &Type,
        target: &Type,
        method: bool,
    ) -> bool {
        // Every target overload must have a compatible source signature.
        // Reapply method variance to each pair, since relation modes are local
        // to one call and an intersection introduces nested relation calls.
        if let Type::Intersection(members) = target {
            if members
                .iter()
                .all(|member| matches!(member, Type::Function(_)))
            {
                return members
                    .iter()
                    .all(|member| self.member_type_assignable(source, member, method));
            }
        }
        if let Type::Intersection(members) = source {
            if members
                .iter()
                .all(|member| matches!(member, Type::Function(_)))
            {
                return members
                    .iter()
                    .any(|member| self.member_type_assignable(member, target, method));
            }
        }
        if method {
            self.relation_mode
                .store(RELATION_TARGET_METHOD, Ordering::Relaxed);
        }
        self.is_assignable_to(source, target)
    }

    pub(crate) fn check_static_extends(&mut self, class: &ClassDecl, identity: &str) {
        let Some(base) = self
            .class_info
            .get(identity)
            .and_then(|info| info.extends.as_deref())
            .and_then(|base| self.static_base_identity(base))
        else {
            return;
        };
        let own = self.own_static_members(identity);
        if own.is_empty() {
            return;
        }
        let mut inherited = Vec::<StaticMember>::new();
        let mut seen = rustc_hash::FxHashSet::default();
        seen.insert(identity.to_owned());
        let mut current = Some(base.clone());
        while let Some(name) = current.take() {
            if !seen.insert(name.clone()) {
                break;
            }
            for member in self.own_static_members(&name) {
                if !inherited
                    .iter()
                    .any(|existing| existing.name == member.name)
                {
                    inherited.push(member);
                }
            }
            current = self
                .class_info
                .get(&name)
                .and_then(|info| info.extends.as_deref())
                .and_then(|base| self.static_base_identity(base));
        }
        let derived_display = class.name.as_deref().unwrap_or_else(|| {
            if identity.starts_with("__class_expression_") {
                "(Anonymous class)"
            } else {
                identity
            }
        });
        let base_display = base.rsplit('.').next().unwrap_or(&base);
        for target in inherited {
            let Some(source) = own.iter().find(|member| member.name == target.name) else {
                continue;
            };
            let name = &target.name;
            let source_private = source.modifiers & MOD_PRIVATE != 0;
            let target_private = target.modifiers & MOD_PRIVATE != 0;
            let visibility = if source_private && target_private {
                Some(format!(
                    "Types have separate declarations of a private property '{name}'."
                ))
            } else if source_private || target_private {
                let (private, public) = if source_private {
                    (derived_display, base_display)
                } else {
                    (base_display, derived_display)
                };
                Some(format!("Property '{name}' is private in type 'typeof {private}' but not in type 'typeof {public}'."))
            } else if source.modifiers & MOD_PROTECTED != 0 && target.modifiers & MOD_PROTECTED == 0
            {
                Some(format!("Property '{name}' is protected in type 'typeof {derived_display}' but public in type 'typeof {base_display}'."))
            } else {
                None
            };
            let source_ty = if source.optional {
                self.optional_property_type(Type::Optional(Arc::new(source.ty.clone())))
            } else {
                source.ty.clone()
            };
            let target_ty = if target.optional {
                self.optional_property_type(Type::Optional(Arc::new(target.ty.clone())))
            } else {
                target.ty.clone()
            };
            let assignable = self.member_type_assignable(&source_ty, &target_ty, target.method);
            let elaboration = if let Some(visibility) = visibility {
                visibility
            } else if !assignable {
                let object = |ty: Type, method: bool| {
                    Type::ObjectType(ObjectTypeInfo::new(ObjectTypeData {
                        properties: vec![(name.clone(), Arc::new(ty))],
                        call_signatures: Vec::new(),
                        construct_signatures: Vec::new(),
                        index_signature: None,
                        index_signature_name: None,
                        method_names: if method {
                            vec![name.clone()]
                        } else {
                            Vec::new()
                        },
                    }))
                };
                self.assignability_elaboration(
                    &object(source_ty.clone(), source.method),
                    &object(target_ty.clone(), target.method),
                )
                .map(|(message, _)| message)
                .unwrap_or_else(|| {
                    format!(
                        "Types of property '{name}' are incompatible.\n  {}",
                        self.leaf_reason_text(&source_ty, &target_ty)
                            .replace('\n', "\n  ")
                    )
                })
            } else if source.optional && !target.optional {
                format!("Property '{name}' is optional in type 'typeof {derived_display}' but required in type 'typeof {base_display}'.")
            } else {
                continue;
            };
            let elaboration = elaboration.replace('\n', "\n  ");
            self.diagnostics.push(Diagnostic {
                code: 2417,
                message: format!("Class static side 'typeof {derived_display}' incorrectly extends base class static side 'typeof {base_display}'.\n  {elaboration}"),
                category: DiagnosticCategory::Error,
                file_name: None,
                span: Some(class.name_span.unwrap_or(Span::new(class.span.start, class.span.start + 5))),
                related: None,
            });
            break;
        }
    }
}
