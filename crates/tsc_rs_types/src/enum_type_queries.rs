//! Enum symbol meanings, adapted from getDeclaredTypeOfEnum and
//! getTypeOfEnumMember in pingdotgg/ts-rust checker_p26/p20.rs. See NOTICE.txt.

use super::*;

impl TypeChecker {
    pub(super) fn hoist_scoped_enum(&mut self, declaration: &EnumDecl) {
        if self.current_scope == 0 || self.type_resolution_namespace.is_some() {
            return;
        }
        let name = &declaration.name;
        if !self.scopes[self.current_scope]
            .shadowed_enums
            .contains_key(name)
        {
            let previous = self.enum_info.get(name).cloned();
            self.scopes[self.current_scope]
                .shadowed_enums
                .insert(name.clone(), previous);
            self.enum_info.insert(name.clone(), Vec::new());
        }
        let members = self.enum_declaration_values(declaration, self.ambient_depth > 0);
        Self::merge_enum_members(&mut self.enum_info, name.clone(), members);
    }

    pub(super) fn enum_union_name(&self, types: &[Type]) -> Option<String> {
        let mut name: Option<String> = None;
        let mut variants = HashSet::new();
        for ty in types {
            let member = match ty {
                Type::EnumVariant { .. } => ty.clone(),
                Type::TypeReference(path, args) if args.is_empty() => {
                    let (enumeration, member) = path.rsplit_once('.')?;
                    self.enum_member_type(enumeration, member)?
                }
                _ => return None,
            };
            let Type::EnumVariant {
                enum_name,
                variant_name,
                ..
            } = member
            else {
                return None;
            };
            if name.as_ref().is_some_and(|name| *name != enum_name) {
                return None;
            }
            name = Some(enum_name);
            variants.insert(variant_name);
        }
        let name = name?;
        let members = self.enum_query_members(&name)?;
        if members.iter().all(|(member, _)| {
            matches!(self.enum_member_type(&name, member),
            Some(Type::EnumVariant { variant_name, .. }) if variants.contains(&variant_name))
        }) {
            Some(name)
        } else {
            None
        }
    }

    pub(super) fn enum_object_member(&self, object: &Type, property: &str) -> Option<Type> {
        let name = self.enum_object_name(object)?;
        if let Type::Intersection(members) = object {
            for member in members.iter() {
                if let Type::ObjectType(info) = member {
                    if let Some((_, ty)) = info.properties.iter().find(|(name, _)| name == property)
                    {
                        if let Type::TypeReference(reference, args) = ty.as_ref() {
                            if args.is_empty()
                                && reference == &format!("{name}.{property}")
                                && self.class_constructor_signatures(reference).is_some()
                            {
                                return Some(Type::TypeReference(
                                    format!("typeof {reference}"),
                                    Arc::from([]),
                                ));
                            }
                        }
                        return Some(ty.as_ref().clone());
                    }
                }
            }
        }
        self.enum_member_type(name, property)
    }
    pub(super) fn record_type_reference_qualifiers(&mut self, name: &Expr) {
        if self.expression_type_spans.is_none() {
            return;
        }
        if let ExprKind::Member(member) = &name.kind {
            self.record_type_reference_qualifiers(&member.object);
            self.record_expression_type_span(member.object.span, &Type::Any);
        }
    }

    pub(super) fn regular_enum_query_type(&self, ty: &Type) -> Option<Type> {
        let Type::EnumVariant {
            enum_name,
            value: Some(value),
            ..
        } = ty
        else {
            return None;
        };
        let members = self.enum_query_members(enum_name)?;
        let single = members.len() == 1
            || matches!(
                value.as_ref(),
                Type::NumberLiteral(_) | Type::StringLiteral(_)
            ) && members.iter().all(|(_, member)| member == value.as_ref());
        single.then(|| Type::TypeReference(enum_name.clone(), Arc::from([])))
    }

    pub(super) fn enum_type_node_name(
        &self,
        name: &str,
        value: bool,
        parameters: &[String],
    ) -> String {
        let qualify_shadowed = |name: &str| {
            if self.file_is_module_flag {
                format!(
                    "{}.{}",
                    crate::namespace_type_spans::namespace_file_import(
                        self.current_file_name.as_deref().unwrap_or("case.ts")
                    ),
                    name
                )
            } else {
                format!("globalThis.{name}")
            }
        };
        if !name.contains('.') {
            let shadowed = value
                && (parameters.iter().any(|parameter| parameter == name)
                    || self
                        .lookup_var(name)
                        .is_some_and(|ty| self.enum_object_name(ty) != Some(name)));
            return if shadowed {
                qualify_shadowed(name)
            } else {
                name.into()
            };
        }
        if let Some(owner) = &self.type_resolution_namespace {
            if let Some(short) = name.strip_prefix(&format!("{owner}.")) {
                return short.into();
            }
        }
        let mut shadowed: HashSet<&str> = parameters.iter().map(String::as_str).collect();
        let mut scope = self.current_scope;
        loop {
            let current = &self.scopes[scope];
            for alias in &current.namespace_alias_order {
                if current
                    .vars
                    .get(alias)
                    .is_some_and(|id| self.enum_object_name(self.get_type(*id)) == Some(name))
                {
                    return if value && shadowed.contains(alias.as_str()) {
                        qualify_shadowed(alias)
                    } else {
                        alias.clone()
                    };
                }
            }
            shadowed.extend(current.vars.keys().map(String::as_str));
            let Some(parent) = current.parent else {
                break;
            };
            scope = parent;
        }
        name.into()
    }

    fn inferred_enum_marker(name: &str) -> String {
        format!("\0enum-members:{name}")
    }

    fn enum_query_members(&self, name: &str) -> Option<Vec<(String, Type)>> {
        if let Some(Type::ObjectType(info)) =
            crate::infer_param_binding(&Self::inferred_enum_marker(name))
        {
            return Some(
                info.properties
                    .iter()
                    .map(|(name, value)| (name.clone(), value.as_ref().clone()))
                    .collect(),
            );
        }
        self.enum_info.get(name).cloned()
    }

    pub(super) fn inferred_enum_bindings(&self, statements: &[Stmt]) -> Vec<(String, Type)> {
        let mut enums: HashMap<String, Vec<(String, Type)>> = HashMap::new();
        let mut names = Vec::new();
        for statement in statements {
            if let StmtKind::EnumDecl(declaration) = &Self::unwrap_export_stmt(statement).kind {
                if !enums.contains_key(&declaration.name) {
                    names.push(declaration.name.clone());
                }
                let values = self.enum_declaration_values(declaration, self.ambient_depth > 0);
                let members = enums.entry(declaration.name.clone()).or_default();
                for member in values {
                    if !members.iter().any(|(name, _)| name == &member.0) {
                        members.push(member);
                    }
                }
            }
        }
        let mut bindings = Vec::new();
        for name in names {
            let members = enums.remove(&name).unwrap();
            bindings.push((
                name.clone(),
                Type::TypeReference(format!("typeof {name}"), Arc::from([])),
            ));
            bindings.push((
                Self::inferred_enum_marker(&name),
                Type::ObjectType(ObjectTypeInfo::new(ObjectTypeData {
                    properties: members
                        .into_iter()
                        .map(|(name, value)| (name, Arc::new(value)))
                        .collect(),
                    ..Default::default()
                })),
            ));
        }
        bindings
    }

    pub(super) fn enum_object_name<'a>(&self, ty: &'a Type) -> Option<&'a str> {
        let name = match ty {
            Type::Intersection(members) => {
                return members
                    .iter()
                    .find_map(|member| self.enum_object_name(member))
            }
            Type::TypeReference(name, args) if args.is_empty() => name.strip_prefix("typeof ")?,
            Type::Typeof(name) => name.as_str(),
            _ => return None,
        };
        self.enum_query_members(name).is_some().then_some(name)
    }

    pub(super) fn enum_member_type(&self, enumeration: &str, member: &str) -> Option<Type> {
        let qualified = self.namespace_owned_type_name(enumeration);
        let enumeration = if self.enum_info.contains_key(&qualified) {
            qualified.as_str()
        } else {
            enumeration
        };
        let members = self.enum_query_members(enumeration)?;
        let (_, value) = members.iter().find(|(name, _)| name == member)?;
        // Equal constant values share the first member's literal identity.
        // Computed members have distinct identities despite their numeric type.
        let canonical = if matches!(value, Type::NumberLiteral(_) | Type::StringLiteral(_)) {
            &members.iter().find(|(_, other)| other == value)?.0
        } else {
            member
        };
        Some(Type::EnumVariant {
            enum_name: enumeration.into(),
            variant_name: canonical.into(),
            value: Some(Arc::new(value.clone())),
        })
    }

    pub(super) fn enum_object_structure(&self, name: &str) -> Option<Type> {
        let members = self.enum_query_members(name)?;
        let properties = members
            .iter()
            .filter_map(|(member, _)| {
                self.enum_member_type(name, member)
                    .map(|ty| (member.clone(), Arc::new(ty)))
            })
            .collect();
        let numeric = members.is_empty()
            || members
                .iter()
                .any(|(_, value)| matches!(value, Type::Number | Type::NumberLiteral(_)));
        Some(Type::ObjectType(ObjectTypeInfo::new(ObjectTypeData {
            properties,
            readonly_names: members.iter().map(|(name, _)| name.clone()).collect(),
            index_signature: numeric.then(|| (Arc::new(Type::Number), Arc::new(Type::String))),
            ..Default::default()
        })))
    }

    pub(super) fn enum_spread_index(&self, props: &[ObjLitProp]) -> Option<(Arc<Type>, Arc<Type>)> {
        props.iter().find_map(|property| {
            let ObjLitProp::Spread(expression, _) = property else {
                return None;
            };
            let ty = self.infer_expr_type(expression);
            let name = self.enum_object_name(&ty)?;
            let Type::ObjectType(info) = self.enum_object_structure(name)? else {
                return None;
            };
            info.index_signature.clone()
        })
    }

    pub(super) fn record_enum_declaration_types(&mut self, enumeration: &EnumDecl) {
        let ty = Type::TypeReference(enumeration.name.clone(), Arc::from([]));
        if let Some(span) = enumeration.name_span {
            self.record_expr_type(span.start, &ty);
            self.record_expression_type_span(span, &ty);
        }
        for member in &enumeration.members {
            let name = self.prop_name_to_string(&member.name);
            if let Some(ty) = self.enum_member_type(&enumeration.name, &name) {
                // Hover renders the enum declaration's constant value, while
                // getTypeAtLocation supplies the member's declared enum type.
                if let Type::EnumVariant {
                    value: Some(value), ..
                } = &ty
                {
                    self.record_expr_type(member.name.span().start, value);
                }
                self.record_expression_type_span(member.name.span(), &ty);
            }
        }
    }
}
