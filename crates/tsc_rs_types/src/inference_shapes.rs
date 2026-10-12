use super::*;

impl TypeChecker {
    /// Expose structural inference sites hidden behind aliases, class values,
    /// and callback signatures. A bare inference variable keeps the source's
    /// named identity; only pairs that require structural matching expand.
    pub(super) fn prepare_inference_shapes(
        &self,
        source: &Type,
        target: &Type,
        parameters: &[String],
        depth: usize,
    ) -> (Type, Type) {
        let unchanged = || (source.clone(), target.clone());
        if depth >= 16 {
            return unchanged();
        }
        let bare_parameter = |ty: &Type| match ty {
            Type::TypeParameter(name) => parameters.contains(name),
            Type::TypeReference(name, args) => args.is_empty() && parameters.contains(name),
            _ => false,
        };
        if bare_parameter(target) {
            return unchanged();
        }
        if matches!((source, target), (Type::TypeReference(s, _), Type::TypeReference(t, _)) if s == t)
        {
            let callable_alias = self.resolve_type_for_assignability(target).is_some_and(|body| {
                matches!(body, Type::Function(_)) || matches!(body, Type::Union(ref members) if members.iter().any(|member| matches!(member, Type::Function(_))))
            });
            if !callable_alias {
                return unchanged();
            }
        }
        let expand = |ty: &Type| match ty {
            Type::IndexedAccess(object, key) => self.known_literal_index_read(object, key, 0),
            Type::TypeReference(name, args)
                if name == "Readonly"
                    && args.len() == 1
                    && !self.type_aliases.contains_key(name)
                    && !self.interface_info.contains_key(name)
                    && !self.class_info.contains_key(name) =>
            {
                Some(args[0].clone())
            }
            Type::TypeReference(name, _) if name.starts_with("typeof ") => {
                self.inference_constructor_shape(ty)
            }
            Type::TypeReference(name, args) if !parameters.contains(name) => self
                .resolve_type_for_assignability(ty)
                .or_else(|| self.resolve_type_reference_to_object(name, args)),
            Type::Typeof(_) => self.inference_constructor_shape(ty),
            Type::Readonly(inner) => Some(Type::clone(inner)),
            _ => None,
        };
        if let Some(expanded) = expand(target).filter(|expanded| expanded != target) {
            return self.prepare_inference_shapes(source, &expanded, parameters, depth + 1);
        }
        if let Type::Constructor(_) = target {
            if !matches!(source, Type::Constructor(_)) {
                if let Some(signature) = self
                    .direct_constructor_signatures(source)
                    .and_then(|s| s.last().cloned())
                {
                    return self.prepare_inference_shapes(
                        &Type::Constructor(signature),
                        target,
                        parameters,
                        depth + 1,
                    );
                }
            }
        }
        if let Some(expanded) = expand(source).filter(|expanded| expanded != source) {
            return self.prepare_inference_shapes(&expanded, target, parameters, depth + 1);
        }
        if let Type::Intersection(members) = target {
            // Every intersection constituent provides inference sites. Keep
            // each prepared source paired with its own target, since alias
            // expansion and reverse mapped inference can transform both.
            let (sources, targets): (Vec<_>, Vec<_>) = members
                .iter()
                .map(|member| self.prepare_inference_shapes(source, member, parameters, depth + 1))
                .unzip();
            return (Type::Tuple(sources.into()), Type::Tuple(targets.into()));
        }
        if let Type::Union(members) = target {
            if !matches!(source, Type::Union(_)) {
                let matching: Vec<_> = members
                    .iter()
                    .filter_map(|member| {
                        let member = expand(member).unwrap_or_else(|| member.clone());
                        matches!(
                            (source, &member),
                            (Type::Function(_), Type::Function(_))
                                | (Type::Constructor(_), Type::Constructor(_))
                                | (Type::ObjectType(_), Type::ObjectType(_))
                        )
                        .then_some(member)
                    })
                    .collect();
                // Prefer a matching wrapper over the bare parameter in
                // `T | (() => T)`, and over unrelated wrapper alternatives.
                if let [member] = matching.as_slice() {
                    return self.prepare_inference_shapes(source, member, parameters, depth + 1);
                }
            }
        }
        match (source, target) {
            (Type::Union(source), Type::Union(target)) if source.len() == target.len() => {
                let (sources, targets): (Vec<_>, Vec<_>) = source
                    .iter()
                    .zip(target.iter())
                    .map(|(s, t)| self.prepare_inference_shapes(s, t, parameters, depth + 1))
                    .unzip();
                (Type::Union(sources.into()), Type::Union(targets.into()))
            }
            (Type::Function(source), Type::Function(target)) => {
                let (mut s, mut t) = (source.clone(), target.clone());
                for ((_, source), (_, target)) in s.params.iter_mut().zip(t.params.iter_mut()) {
                    (*source, *target) =
                        self.prepare_inference_shapes(source, target, parameters, depth + 1);
                }
                let (source, target) = self.prepare_inference_shapes(
                    &s.return_type,
                    &t.return_type,
                    parameters,
                    depth + 1,
                );
                s.return_type = Arc::new(source);
                t.return_type = Arc::new(target);
                (Type::Function(s), Type::Function(t))
            }
            (Type::Constructor(source), Type::Constructor(target)) => {
                let mut source = source.clone();
                if !source.type_params.is_empty() && target.type_params.is_empty() {
                    let signature = FunctionType {
                        type_params: source.type_params.clone(),
                        type_param_constraints: source.type_param_constraints.clone(),
                        type_param_defaults: source.type_param_defaults.clone(),
                        params: Vec::new(),
                        return_type: Arc::new(Type::Any),
                        type_predicate: None,
                    };
                    let mut defaults = HashMap::new();
                    Self::apply_type_param_defaults(&signature, &mut defaults);
                    for name in &source.type_params {
                        defaults.entry(name.clone()).or_insert(Type::Unknown);
                    }
                    source.type_params.clear();
                    if let Type::Constructor(instantiated) =
                        Self::substitute(&Type::Constructor(source.clone()), &defaults)
                    {
                        source = instantiated;
                    }
                }
                let mut target = target.clone();
                let (s, t) = self.prepare_inference_shapes(
                    &source.return_type,
                    &target.return_type,
                    parameters,
                    depth + 1,
                );
                source.return_type = Arc::new(s);
                target.return_type = Arc::new(t);
                (Type::Constructor(source), Type::Constructor(target))
            }
            (Type::ObjectType(source), Type::ObjectType(target)) => {
                let (mut s, mut t) = ((**source).clone(), (**target).clone());
                for (name, target) in &mut t.properties {
                    if let Some((_, source)) = s.properties.iter_mut().find(|(n, _)| n == name) {
                        let (a, b) =
                            self.prepare_inference_shapes(source, target, parameters, depth + 1);
                        *source = Arc::new(a);
                        *target = Arc::new(b);
                    }
                }
                for (source, target) in s
                    .construct_signatures
                    .iter_mut()
                    .zip(t.construct_signatures.iter_mut())
                {
                    let (a, b) = self.prepare_inference_shapes(
                        &Type::Constructor(source.clone()),
                        &Type::Constructor(target.clone()),
                        parameters,
                        depth + 1,
                    );
                    if let (Type::Constructor(a), Type::Constructor(b)) = (a, b) {
                        *source = a;
                        *target = b;
                    }
                }
                (
                    Type::ObjectType(ObjectTypeInfo::new(s)),
                    Type::ObjectType(ObjectTypeInfo::new(t)),
                )
            }
            (
                Type::ObjectType(source),
                Type::Mapped {
                    param,
                    constraint,
                    template: Some(template),
                    name_type: None,
                    ..
                },
            ) => {
                let Type::Keyof(object) = constraint.as_ref() else {
                    return unchanged();
                };
                if !bare_parameter(object) {
                    return unchanged();
                }
                let name = match object.as_ref() {
                    Type::TypeParameter(name) | Type::TypeReference(name, _) => name,
                    _ => return unchanged(),
                };
                let slot = "\0reverse_inference".to_string();
                let mut properties = Vec::new();
                for (key, value) in &source.properties {
                    let substituted = HashMap::from([
                        (param.clone(), Type::StringLiteral(key.clone())),
                        (
                            name.clone(),
                            Type::ObjectType(ObjectTypeInfo::new(ObjectTypeData {
                                properties: vec![(
                                    key.clone(),
                                    Arc::new(Type::TypeParameter(slot.clone())),
                                )],
                                ..ObjectTypeData::default()
                            })),
                        ),
                    ]);
                    let projected = self.simplify_type(&Self::substitute(template, &substituted));
                    let (s, t) = self.prepare_inference_shapes(
                        value,
                        &projected,
                        std::slice::from_ref(&slot),
                        depth + 1,
                    );
                    let mut inferred = Self::infer_type_arguments(
                        &[s],
                        &[(key.clone(), t)],
                        std::slice::from_ref(&slot),
                    );
                    let Some(value) = inferred.remove(&slot) else {
                        return unchanged();
                    };
                    properties.push((key.clone(), Arc::new(value)));
                }
                if properties.is_empty() {
                    return unchanged();
                }
                (
                    Type::ObjectType(ObjectTypeInfo::new(ObjectTypeData {
                        properties,
                        ..ObjectTypeData::default()
                    })),
                    Type::clone(object),
                )
            }
            (Type::Optional(source), Type::Optional(target)) => {
                let (s, t) = self.prepare_inference_shapes(source, target, parameters, depth + 1);
                (Type::Optional(Arc::new(s)), Type::Optional(Arc::new(t)))
            }
            (source, Type::Optional(target)) => {
                self.prepare_inference_shapes(source, target, parameters, depth + 1)
            }
            _ => unchanged(),
        }
    }

    pub(super) fn inference_constructor_shape(&self, ty: &Type) -> Option<Type> {
        self.direct_constructor_static_members(ty)
            .map(|mut members| {
                let signatures = self.direct_constructor_signatures(ty).unwrap_or_default();
                if !members.iter().any(|(name, _)| name == "prototype") {
                    if let Some(signature) = signatures.first() {
                        let class_value = match ty {
                            Type::TypeReference(name, _) => name.starts_with("typeof "),
                            Type::Typeof(name) => {
                                self.class_info.contains_key(name)
                                    || matches!(self.lookup_var(name), Some(Type::TypeReference(name, _)) if name.starts_with("typeof "))
                            }
                            _ => false,
                        };
                        let arguments = signature
                            .type_params
                            .iter()
                            .map(|name| (name.clone(), Type::Any))
                            .collect();
                        members.push((
                            "prototype".into(),
                            if class_value {
                                Self::substitute(&signature.return_type, &arguments)
                            } else {
                                // A construct signature alone inherits the
                                // untyped Function.prototype property.
                                Type::Any
                            },
                        ));
                    }
                }
                Type::ObjectType(ObjectTypeInfo::new(ObjectTypeData {
                    properties: members
                        .into_iter()
                        .map(|(name, ty)| (name, Arc::new(ty)))
                        .collect(),
                    construct_signatures: signatures,
                    ..ObjectTypeData::default()
                }))
            })
    }
}
