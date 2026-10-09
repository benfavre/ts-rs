use super::*;

#[derive(Clone)]
struct IndexConstraint {
    key: Type,
    value: Type,
    is_static: bool,
    local_span: Option<Span>,
}

#[derive(Debug, Clone)]
pub(super) struct ComputedIndexMember {
    display: String,
    file: String,
    span: Span,
}

impl TypeChecker {
    pub(super) fn class_computed_index_members(
        &self,
        declaration: &ClassDecl,
    ) -> rustc_hash::FxHashMap<(String, bool), ComputedIndexMember> {
        let mut origins = rustc_hash::FxHashMap::default();
        for member in &declaration.members {
            let (name, modifiers) = match &member.kind {
                ClassMemberKind::Property(property) => (&property.name, property.modifiers),
                ClassMemberKind::Method(method) => (&method.name, method.modifiers),
                ClassMemberKind::GetAccessor(accessor) | ClassMemberKind::SetAccessor(accessor) => {
                    (&accessor.name, accessor.modifiers)
                }
                _ => continue,
            };
            if !matches!(name, PropName::Computed(..)) {
                continue;
            }
            origins
                .entry((self.prop_name_to_string(name), modifiers & MOD_STATIC != 0))
                .or_insert_with(|| ComputedIndexMember {
                    display: self.index_property_display(name),
                    file: self
                        .injected_decl_file
                        .as_ref()
                        .or(self.current_file_name.as_ref())
                        .cloned()
                        .unwrap_or_default(),
                    span: name.span(),
                });
        }
        origins
    }

    pub(crate) fn interface_index_locations(
        &self,
        declaration: &InterfaceDecl,
    ) -> rustc_hash::FxHashMap<Type, (String, Span)> {
        let file = self
            .injected_decl_file
            .as_ref()
            .or(self.current_file_name.as_ref())
            .cloned()
            .unwrap_or_default();
        let mut locations = rustc_hash::FxHashMap::default();
        for index in self.own_interface_index_constraints(declaration) {
            if let Some(span) = index.local_span {
                locations.entry(index.key).or_insert((file.clone(), span));
            }
        }
        locations
    }

    pub(crate) fn interface_index_signature_types(
        &self,
        declaration: &InterfaceDecl,
    ) -> Vec<(Type, Type)> {
        self.own_interface_index_constraints(declaration)
            .into_iter()
            .map(|index| (index.key, index.value))
            .collect()
    }

    fn own_interface_index_constraints(&self, declaration: &InterfaceDecl) -> Vec<IndexConstraint> {
        let mut indices = Vec::new();
        for member in &declaration.members {
            let TypeMemberKind::IndexSig(signature) = &member.kind else {
                continue;
            };
            let Some(key) = signature
                .params
                .first()
                .and_then(|param| param.type_ann.as_ref())
            else {
                continue;
            };
            let Some(value) = &signature.type_ann else {
                continue;
            };
            for key in self.constraint_index_keys(&self.resolve_type_node(key), 0) {
                indices.push(IndexConstraint {
                    key,
                    value: self.resolve_type_node(value),
                    is_static: false,
                    local_span: Some(member.span),
                });
            }
        }
        indices
    }

    fn inherited_index_constraints(
        &self,
        ty: &Type,
        seen: &mut HashSet<String>,
    ) -> Vec<IndexConstraint> {
        let Type::TypeReference(name, arguments) = ty else {
            return match ty {
                Type::ObjectType(object) => object
                    .index_signature
                    .iter()
                    .map(|(key, value)| IndexConstraint {
                        key: key.as_ref().clone(),
                        value: value.as_ref().clone(),
                        is_static: false,
                        local_span: None,
                    })
                    .collect(),
                _ => Vec::new(),
            };
        };
        if !seen.insert(name.clone()) || seen.len() > 64 {
            return Vec::new();
        }
        let (parameters, own, bases) = if let Some(info) = self.interface_info.get(name) {
            let bases = info
                .extends
                .iter()
                .enumerate()
                .map(|(position, (name, args))| {
                    Type::TypeReference(
                        self.interface_lookup_name_for_source(
                            name,
                            info.extends_sources
                                .get(position)
                                .and_then(|source| source.as_deref()),
                        ),
                        args.clone(),
                    )
                })
                .collect::<Vec<_>>();
            (&info.type_params, info.index_signatures.clone(), bases)
        } else if let Some(info) = self.class_info.get(name) {
            let own = info
                .index_signatures
                .iter()
                .filter(|(_, _, is_static)| !is_static)
                .map(|(key, value, _)| (key.clone(), value.clone()))
                .collect();
            let bases = info
                .extends
                .iter()
                .map(|name| {
                    Type::TypeReference(name.clone(), info.extends_type_args.clone().into())
                })
                .collect();
            (&info.type_params, own, bases)
        } else {
            let result = self
                .expand_type_alias_once(name, arguments)
                .map(|expanded| self.inherited_index_constraints(&expanded, seen))
                .unwrap_or_default();
            seen.remove(name);
            return result;
        };
        let substitutions: HashMap<_, _> = parameters
            .iter()
            .cloned()
            .zip(arguments.iter().cloned())
            .collect();
        let mut indices: Vec<_> = own
            .into_iter()
            .map(|(key, value)| IndexConstraint {
                key: Self::substitute(&key, &substitutions),
                value: Self::substitute(&value, &substitutions),
                is_static: false,
                local_span: None,
            })
            .collect();
        for base in bases {
            for index in
                self.inherited_index_constraints(&Self::substitute(&base, &substitutions), seen)
            {
                if !indices.iter().any(|own| own.key == index.key) {
                    indices.push(index);
                }
            }
        }
        seen.remove(name);
        indices
    }

    pub(crate) fn check_interface_index_constraints(&mut self, declaration: &InterfaceDecl) {
        if self
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == 2320 && diagnostic.span == declaration.name_span)
        {
            return;
        }
        let mut indices = self.own_interface_index_constraints(declaration);
        let name = self.namespace_owned_type_name(&declaration.name);
        let own_type = Type::TypeReference(
            name.clone(),
            declaration
                .type_params
                .iter()
                .flatten()
                .map(|parameter| Type::TypeReference(parameter.name.clone(), Vec::new().into()))
                .collect(),
        );
        for index in self.inherited_index_constraints(&own_type, &mut HashSet::new()) {
            if !indices.iter().any(|own| own.key == index.key) {
                indices.push(index);
            }
        }
        if indices.is_empty() {
            return;
        }
        let info = self.interface_info.get(&name).cloned();
        let index_locations = info
            .as_ref()
            .map(|info| info.index_locations.clone())
            .unwrap_or_default();
        let bases: Vec<_> = info
            .as_ref()
            .into_iter()
            .flat_map(|info| {
                info.extends
                    .iter()
                    .enumerate()
                    .map(|(position, (name, args))| {
                        let name = self.interface_lookup_name_for_source(
                            name,
                            info.extends_sources
                                .get(position)
                                .and_then(|source| source.as_deref()),
                        );
                        self.inherited_index_constraints(
                            &Type::TypeReference(name, args.clone()),
                            &mut HashSet::new(),
                        )
                        .into_iter()
                        .map(|index| index.key)
                        .collect::<Vec<_>>()
                    })
            })
            .collect();
        let fallback = if info
            .as_ref()
            .is_none_or(|info| info.declaration_span == declaration.span)
        {
            declaration.name_span
        } else {
            None
        };
        self.check_index_constraint_pairs(&indices, &index_locations, fallback, &bases);
        self.check_inherited_interface_index_properties(
            &own_type,
            &indices,
            &index_locations,
            fallback,
        );
        let own = self.build_interface_info(declaration);
        let locations = self
            .interface_info
            .get(&name)
            .map(|info| info.member_locations.clone())
            .unwrap_or_default();
        let mut seen = HashSet::new();
        for member in &declaration.members {
            let (name, optional) = match &member.kind {
                TypeMemberKind::PropertySig(property) => (&property.name, property.optional),
                TypeMemberKind::MethodSig(method) => (&method.name, method.optional),
                TypeMemberKind::GetAccessorSig(accessor)
                | TypeMemberKind::SetAccessorSig(accessor) => (&accessor.name, false),
                _ => continue,
            };
            let property_name = self.prop_name_to_string(name);
            if !seen.insert(property_name.clone()) {
                continue;
            }
            if let Some((file, span)) = locations.get(&property_name) {
                if *span != name.span()
                    || self.current_file_name.as_ref().is_some_and(|current| {
                        !file.is_empty() && !Self::paths_refer_to_same_file(current, file)
                    })
                {
                    continue;
                }
            }
            let Some((_, ty)) = own
                .object_type
                .properties
                .iter()
                .find(|(n, _)| *n == property_name)
            else {
                continue;
            };
            let ty = self.index_property_type(ty.as_ref().clone(), optional);
            let key = if let PropName::Computed(expression, _) = name {
                self.index_computed_property_key(expression)
            } else {
                Type::StringLiteral(property_name.clone())
            };
            let display_name = self.index_property_display(name);
            let span = if matches!(member.kind, TypeMemberKind::MethodSig(_)) {
                member.span
            } else {
                name.span()
            };
            for index in &indices {
                self.check_property_index_constraint(&display_name, &key, &ty, index, span);
            }
        }
    }

    fn check_inherited_interface_index_properties(
        &mut self,
        ty: &Type,
        indices: &[IndexConstraint],
        index_locations: &rustc_hash::FxHashMap<Type, (String, Span)>,
        fallback: Option<Span>,
    ) {
        let Type::TypeReference(name, arguments) = ty else {
            return;
        };
        let Some(info) = self.interface_info.get(name) else {
            return;
        };
        if info.extends.is_empty() {
            return;
        }
        let own_members: HashSet<_> = info
            .object_type
            .properties
            .iter()
            .map(|(name, _)| name.clone())
            .collect();
        let bases: Vec<_> = info
            .extends
            .iter()
            .enumerate()
            .map(|(position, (name, args))| {
                let name = self.interface_lookup_name_for_source(
                    name,
                    info.extends_sources
                        .get(position)
                        .and_then(|source| source.as_deref()),
                );
                let base = Type::TypeReference(name.clone(), args.clone());
                let keys: Vec<_> = self
                    .inherited_index_constraints(&base, &mut HashSet::new())
                    .into_iter()
                    .map(|index| index.key)
                    .collect();
                let properties: HashSet<_> =
                    match self.resolve_type_reference_to_object(&name, args) {
                        Some(Type::ObjectType(object)) => object
                            .into_data()
                            .properties
                            .into_iter()
                            .map(|(name, _)| name)
                            .collect(),
                        _ => HashSet::new(),
                    };
                (keys, properties)
            })
            .collect();
        let Some(Type::ObjectType(object)) = self.resolve_type_reference_to_object(name, arguments)
        else {
            return;
        };
        let diagnostic_start = self.diagnostics.len();
        let mut seen = HashSet::new();
        for (name, property) in object.into_data().properties {
            if own_members.contains(&name) || name.starts_with('#') || !seen.insert(name.clone()) {
                continue;
            }
            let key = Self::inherited_index_property_key(&name);
            let property = match property.as_ref() {
                Type::Optional(inner) => self.index_property_type(inner.as_ref().clone(), true),
                property => property.clone(),
            };
            let display_name = self
                .heritage_member_metadata(ty, &name, &mut HashSet::new())
                .map(|(display, _)| display)
                .unwrap_or_else(|| name.clone());
            for index in indices {
                let location = index_locations.get(&index.key).cloned().or_else(|| {
                    if bases.iter().any(|(keys, properties)| {
                        keys.contains(&index.key) && properties.contains(&name)
                    }) {
                        return None;
                    }
                    fallback.map(|span| (self.current_file_name.clone().unwrap_or_default(), span))
                });
                let Some((file, span)) = location else {
                    continue;
                };
                if self.current_file_name.as_ref().is_some_and(|current| {
                    !file.is_empty() && !Self::paths_refer_to_same_file(current, &file)
                }) {
                    continue;
                }
                self.check_property_index_constraint(&display_name, &key, &property, index, span);
            }
        }
        // TypeScript orders diagnostics sharing a location by their text,
        // independently of the order in which bases contribute their members.
        self.diagnostics[diagnostic_start..].sort_by(|left, right| {
            left.span
                .map(|span| (span.start, span.end))
                .cmp(&right.span.map(|span| (span.start, span.end)))
                .then_with(|| left.message.cmp(&right.message))
        });
    }

    fn inherited_index_property_key(name: &str) -> Type {
        if let Some(identity) = name
            .strip_prefix("[unique:")
            .and_then(|name| name.strip_suffix(']'))
        {
            Type::UniqueSymbol(identity.into())
        } else if name
            .strip_prefix("[Symbol.")
            .and_then(|name| name.strip_suffix(']'))
            .is_some_and(Self::is_well_known_symbol_name)
        {
            Type::Symbol
        } else {
            Type::StringLiteral(name.into())
        }
    }

    fn index_symbol_is_locally_bound(&self) -> bool {
        let mut scope = self.current_scope;
        while let Some(parent) = self.scopes[scope].parent {
            if self.scopes[scope].vars.contains_key("Symbol") {
                return true;
            }
            scope = parent;
        }
        false
    }

    fn index_computed_property_key(&self, expression: &Expr) -> Type {
        match &expression.kind {
            ExprKind::Member(member)
                if !self.file_shadows_global_symbol
                    && !self.index_symbol_is_locally_bound()
                    && Self::is_well_known_symbol_name(&member.property)
                    && matches!(&member.object.kind, ExprKind::Ident(name) if name == "Symbol") =>
            {
                Type::Symbol
            }
            _ => self.infer_expr_type(expression),
        }
    }

    fn index_property_display(&self, name: &PropName) -> String {
        let property_name = self.prop_name_to_string(name);
        match name {
            PropName::Computed(_, span) | PropName::String(_, span) | PropName::Number(_, span) => {
                self.current_source
                    .as_ref()
                    .and_then(|source| source.get(span.start as usize..span.end as usize))
                    .unwrap_or(&property_name)
                    .to_string()
            }
            _ => property_name,
        }
    }

    pub(crate) fn class_index_signature_types(
        &self,
        declaration: &ClassDecl,
    ) -> Vec<(Type, Type, bool)> {
        self.own_class_index_constraints(declaration)
            .into_iter()
            .map(|index| (index.key, index.value, index.is_static))
            .collect()
    }

    fn own_class_index_constraints(&self, declaration: &ClassDecl) -> Vec<IndexConstraint> {
        let mut indices = Vec::new();
        for member in &declaration.members {
            let ClassMemberKind::IndexSignature(signature) = &member.kind else {
                continue;
            };
            let Some(key) = signature
                .params
                .first()
                .and_then(|param| param.type_ann.as_ref())
            else {
                continue;
            };
            let Some(value) = &signature.type_ann else {
                continue;
            };
            for key in self.constraint_index_keys(&self.resolve_type_node(key), 0) {
                indices.push(IndexConstraint {
                    key,
                    value: self.resolve_type_node(value),
                    is_static: signature.modifiers & MOD_STATIC != 0,
                    local_span: Some(member.span),
                });
            }
        }
        indices
    }

    fn constraint_index_keys(&self, key: &Type, depth: usize) -> Vec<Type> {
        if depth > 32 {
            return Vec::new();
        }
        match key {
            Type::String | Type::Number | Type::Symbol | Type::TemplateLiteral { .. } => {
                vec![key.clone()]
            }
            Type::Union(parts) => parts
                .iter()
                .flat_map(|part| self.constraint_index_keys(part, depth + 1))
                .collect(),
            Type::TypeReference(name, args) => self
                .expand_type_alias_once(name, args)
                .map(|expanded| self.constraint_index_keys(&expanded, depth + 1))
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    /// TS2411 applies to both sides of a class, including inherited indexers.
    pub(crate) fn check_index_signature_members(&mut self, declaration: &ClassDecl) {
        let mut indices = self.own_class_index_constraints(declaration);
        let check_inherited = !indices.is_empty();
        let mut base = declaration.extends.as_ref().map(|base| {
            (
                self.namespace_owned_type_name(&self.expr_to_name(base)),
                declaration
                    .extends_type_args
                    .iter()
                    .flatten()
                    .map(|argument| self.resolve_type_node(argument))
                    .collect::<Vec<_>>(),
            )
        });
        let mut inherited = Vec::new();
        let mut seen = HashSet::new();
        while let Some((name, arguments)) = base.take() {
            if !seen.insert(name.clone()) || seen.len() > 64 {
                break;
            }
            let Some(info) = self.class_info.get(&name) else {
                break;
            };
            let mut substitutions = HashMap::new();
            for (position, parameter) in info.type_params.iter().enumerate() {
                if let Some(argument) = arguments.get(position).or_else(|| {
                    info.type_param_defaults
                        .get(position)
                        .and_then(Option::as_ref)
                }) {
                    substitutions.insert(
                        parameter.clone(),
                        Self::substitute(argument, &substitutions),
                    );
                }
            }
            for (key, value, is_static) in &info.index_signatures {
                // Constructor-side index signatures are not inherited. Static
                // members still participate when this class declares an index.
                if *is_static {
                    continue;
                }
                let key = Self::substitute(key, &substitutions);
                if !indices
                    .iter()
                    .any(|index| index.is_static == *is_static && index.key == key)
                {
                    indices.push(IndexConstraint {
                        key,
                        value: Self::substitute(value, &substitutions),
                        is_static: *is_static,
                        local_span: None,
                    });
                }
            }
            for is_static in [false, true] {
                if !check_inherited {
                    break;
                }
                let (properties, methods) = if is_static {
                    (&info.static_properties, &info.static_methods)
                } else {
                    (&info.instance_properties, &info.instance_methods)
                };
                for (name, ty) in properties.iter().cloned().chain(
                    methods
                        .iter()
                        .map(|(name, ty)| (name.clone(), Type::Function(ty.clone()))),
                ) {
                    if !inherited
                        .iter()
                        .any(|(n, _, s, _)| *n == name && *s == is_static)
                    {
                        let optional = if is_static {
                            &info.optional_static_properties
                        } else {
                            &info.optional_instance_properties
                        };
                        let ty = self.index_property_type(
                            Self::substitute(&ty, &substitutions),
                            optional.contains(&name),
                        );
                        let origin = info
                            .computed_member_origins
                            .get(&(name.clone(), is_static))
                            .cloned();
                        inherited.push((name, ty, is_static, origin));
                    }
                }
            }
            base = info.extends.clone().map(|name| {
                (
                    name,
                    info.extends_type_args
                        .iter()
                        .map(|arg| Self::substitute(arg, &substitutions))
                        .collect(),
                )
            });
        }
        if indices.is_empty() {
            return;
        }
        self.check_index_constraint_pairs(&indices, &rustc_hash::FxHashMap::default(), None, &[]);
        let own = self.build_class_info(declaration);
        let mut own_names = HashSet::new();
        for member in &declaration.members {
            let (name, modifiers, optional) = match &member.kind {
                ClassMemberKind::Property(property) => {
                    (&property.name, property.modifiers, property.optional)
                }
                ClassMemberKind::Method(method) => {
                    (&method.name, method.modifiers, method.optional)
                }
                ClassMemberKind::GetAccessor(accessor) | ClassMemberKind::SetAccessor(accessor) => {
                    (&accessor.name, accessor.modifiers, false)
                }
                _ => continue,
            };
            if matches!(name, PropName::Private(..)) {
                continue;
            }
            let is_static = modifiers & MOD_STATIC != 0;
            let property_name = self.prop_name_to_string(name);
            if !own_names.insert((property_name.clone(), is_static)) {
                continue;
            }
            let (properties, methods) = if is_static {
                (&own.static_properties, &own.static_methods)
            } else {
                (&own.instance_properties, &own.instance_methods)
            };
            let ty = properties
                .iter()
                .find(|(n, _)| *n == property_name)
                .map(|(_, ty)| ty.clone())
                .or_else(|| {
                    methods
                        .iter()
                        .find(|(n, _)| *n == property_name)
                        .map(|(_, ty)| Type::Function(ty.clone()))
                })
                .unwrap_or(Type::Any);
            let ty = self.index_property_type(ty, optional);
            let key = if let PropName::Computed(expression, _) = name {
                let ty = self.index_computed_property_key(expression);
                if matches!(ty, Type::Any | Type::Error) {
                    continue;
                }
                ty
            } else {
                Type::StringLiteral(property_name.clone())
            };
            let display_name = if let PropName::Computed(_, span)
            | PropName::String(_, span)
            | PropName::Number(_, span) = name
            {
                self.current_source
                    .as_ref()
                    .and_then(|source| source.get(span.start as usize..span.end as usize))
                    .unwrap_or(&property_name)
                    .to_string()
            } else {
                property_name
            };
            for index in indices.iter().filter(|index| index.is_static == is_static) {
                self.check_property_index_constraint(&display_name, &key, &ty, index, name.span());
            }
        }
        for (name, ty, is_static, origin) in inherited {
            if name.starts_with('#') || own_names.contains(&(name.clone(), is_static)) {
                continue;
            }
            let key = Self::inherited_index_property_key(&name);
            for index in indices.iter().filter(|index| index.is_static == is_static) {
                if let Some(span) = index.local_span {
                    let display = origin
                        .as_ref()
                        .map_or(name.as_str(), |origin| origin.display.as_str());
                    let previous = self.diagnostics.len();
                    self.check_property_index_constraint(display, &key, &ty, index, span);
                    if let Some(origin) = &origin {
                        if self.diagnostics.len() > previous {
                            self.diagnostics[previous].related = Some(vec![RelatedDiagnostic {
                                code: 2728,
                                message: format!("'{}' is declared here.", origin.display),
                                file_name: Some(origin.file.clone()),
                                span: Some(origin.span),
                            }]);
                        }
                    }
                }
            }
        }
    }

    fn index_property_type(&self, ty: Type, optional: bool) -> Type {
        if optional
            && self
                .compiler_options
                .strict_null_checks
                .or(self.compiler_options.strict)
                .unwrap_or(false)
        {
            Type::flatten_union(vec![ty, Type::Undefined])
        } else {
            ty
        }
    }

    fn check_index_constraint_pairs(
        &mut self,
        indices: &[IndexConstraint],
        locations: &rustc_hash::FxHashMap<Type, (String, Span)>,
        interface_span: Option<Span>,
        base_keys: &[Vec<Type>],
    ) {
        for source in indices {
            for target in indices {
                if source.is_static != target.is_static
                    || source.key == target.key
                    || !self.index_applies_to_property(&source.key, &target.key)
                {
                    continue;
                }
                let location = |index: &IndexConstraint| {
                    locations.get(&index.key).cloned().or_else(|| {
                        index
                            .local_span
                            .map(|span| (self.current_file_name.clone().unwrap_or_default(), span))
                    })
                };
                let error_location = location(source).or_else(|| location(target)).or_else(|| {
                    if base_keys
                        .iter()
                        .any(|keys| keys.contains(&source.key) && keys.contains(&target.key))
                    {
                        return None;
                    }
                    interface_span
                        .map(|span| (self.current_file_name.clone().unwrap_or_default(), span))
                });
                let Some((file, span)) = error_location else {
                    continue;
                };
                if self.current_file_name.as_ref().is_some_and(|current| {
                    !file.is_empty() && !Self::paths_refer_to_same_file(current, &file)
                }) {
                    continue;
                }
                if self.is_assignable_to(&source.value, &target.value) {
                    continue;
                }
                let message = format!(
                    "'{}' index type '{}' is not assignable to '{}' index type '{}'.",
                    source.key.display_string_single_line(),
                    source.value.display_string_single_line(),
                    target.key.display_string_single_line(),
                    target.value.display_string_single_line()
                );
                if !self.diagnostics.iter().any(|diagnostic| {
                    diagnostic.code == 2413
                        && diagnostic.span == Some(span)
                        && diagnostic.message == message
                }) {
                    self.diagnostics.push(Diagnostic {
                        code: 2413,
                        message,
                        category: DiagnosticCategory::Error,
                        file_name: None,
                        span: Some(span),
                        related: None,
                    });
                }
            }
        }
    }

    fn index_applies_to_property(&self, key: &Type, index: &Type) -> bool {
        match (key, index) {
            (
                Type::String | Type::StringLiteral(_) | Type::Number | Type::NumberLiteral(_),
                Type::String,
            ) => true,
            (Type::StringLiteral(name), Type::Number) => {
                matches!(name.as_str(), "NaN" | "Infinity" | "-Infinity")
                    || name.parse::<f64>().is_ok_and(|number| {
                        number.is_finite() && Self::canonical_numeric_property_name(name) == *name
                    })
            }
            (Type::UniqueSymbol(_) | Type::Symbol, Type::Symbol) => true,
            (Type::Any | Type::Error, _) => false,
            _ => self.is_assignable_to(key, index),
        }
    }

    fn check_property_index_constraint(
        &mut self,
        name: &str,
        key: &Type,
        ty: &Type,
        index: &IndexConstraint,
        span: Span,
    ) {
        if self.index_applies_to_property(key, &index.key)
            && !self.is_assignable_to(ty, &index.value)
        {
            let diagnostic = crate::diagnostics::error_property_not_assignable_to_index(
                name,
                &ty.display_string_single_line(),
                &index.key.display_string_single_line(),
                &index.value.display_string_single_line(),
                span,
            );
            if !self.diagnostics.iter().any(|d| {
                d.code == diagnostic.code
                    && d.span == diagnostic.span
                    && d.message == diagnostic.message
            }) {
                self.diagnostics.push(diagnostic);
            }
        }
    }
}
