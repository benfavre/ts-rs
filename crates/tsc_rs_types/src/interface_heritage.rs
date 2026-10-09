//! Declaration-site compatibility of interface members with their bases.

use super::*;

mod identity;

impl TypeChecker {
    pub(crate) fn check_interface_heritage(&mut self, declaration: &InterfaceDecl) {
        if !self.check_inherited_property_identity(declaration) {
            return;
        }
        if declaration.extends.is_empty() {
            return;
        }
        let own = self.build_interface_info(declaration);
        let constraints = declaration
            .type_params
            .iter()
            .flatten()
            .filter_map(|parameter| {
                parameter
                    .constraint
                    .as_ref()
                    .map(|constraint| (parameter.name.clone(), self.resolve_type_node(constraint)))
            })
            .collect();
        self.type_param_constraint_stack.push(constraints);
        let derived_name = if own.type_params.is_empty() {
            declaration.name.clone()
        } else {
            format!("{}<{}>", declaration.name, own.type_params.join(", "))
        };
        let bases: Vec<_> = declaration
            .extends
            .iter()
            .map(|base| self.resolve_type_node(base))
            .collect();
        let inherited_index = own.object_type.index_signature.clone().or_else(|| {
            bases
                .iter()
                .find_map(|base| match self.heritage_apparent_type(base) {
                    Type::ObjectType(object) => object.into_data().index_signature,
                    _ => None,
                })
        });
        for base in &bases {
            let base_name = self.heritage_display(base);
            let apparent_base = self.heritage_apparent_type(base);
            let mut detail = None;
            for (name, property) in &own.object_type.properties {
                if let Type::TypeReference(class_name, _) = base {
                    let mut current = Some(class_name.as_str());
                    let mut seen = HashSet::new();
                    while let Some(name_of_class) = current.filter(|name| seen.insert(*name)) {
                        let Some(class) = self.class_info.get(name_of_class) else {
                            break;
                        };
                        if class.own_protected_members.contains(name) {
                            detail = Some(format!("Property '{name}' is protected but type '{derived_name}' is not a class derived from '{name_of_class}'."));
                            break;
                        }
                        current = class.extends.as_deref();
                    }
                    if detail.is_some() {
                        break;
                    }
                    if self
                        .private_member_origins(class_name)
                        .iter()
                        .any(|(member, _)| member == name)
                    {
                        detail = Some(format!(
                            "Property '{name}' is private in type '{base_name}' but not in type '{derived_name}'."
                        ));
                        break;
                    }
                }
                let Some(base_property) =
                    self.heritage_base_property(base, name, &mut HashSet::new())
                else {
                    continue;
                };
                let property = if own.optional_props.contains(name) {
                    Type::Optional(Arc::clone(property))
                } else {
                    property.as_ref().clone()
                };
                let (display_name, method) = self
                    .heritage_member_metadata(base, name, &mut HashSet::new())
                    .or_else(|| own.member_names.get(name).cloned())
                    .unwrap_or_else(|| (name.clone(), false));
                if let Some(reason) = self.heritage_property_error(
                    &property,
                    &base_property,
                    &display_name,
                    0,
                    method,
                ) {
                    detail = Some(reason);
                    break;
                }
            }
            if detail.is_none() {
                if let (Some((source_key, source_value)), Type::ObjectType(base_object)) =
                    (&inherited_index, &apparent_base)
                {
                    if let Some((target_key, target_value)) = &base_object.index_signature {
                        if source_key == target_key {
                            if let Some(reason) =
                                self.heritage_type_error(source_value, target_value, 0)
                            {
                                detail = Some(format!(
                                    "'{}' index signatures are incompatible.\n{}",
                                    source_key.display_string(),
                                    indent_heritage_reason(&reason)
                                ));
                            }
                        }
                    }
                }
            }
            if let Some(detail) = detail {
                self.diagnostics.push(Diagnostic {
                    code: 2430,
                    message: format!(
                        "Interface '{derived_name}' incorrectly extends interface '{base_name}'.\n{}",
                        indent_heritage_reason(&detail)
                    ),
                    category: DiagnosticCategory::Error,
                    file_name: None,
                    span: declaration.name_span.or(Some(declaration.span)),
                    related: None,
                });
            }
        }
        self.type_param_constraint_stack.pop();
    }

    fn heritage_display(&self, ty: &Type) -> String {
        match ty {
            Type::Optional(inner) => format!("{} | undefined", self.heritage_display(inner)),
            Type::Function(_) => {
                self.heritage_signature_display(&heritage_signatures(ty, false).unwrap()[0], false)
            }
            Type::Constructor(signature) => self.heritage_signature_display(signature, true),
            Type::Union(members) => members
                .iter()
                .map(|member| self.heritage_display(member))
                .collect::<Vec<_>>()
                .join(" | "),
            Type::TypeReference(name, args) => {
                let mut namespace = self.type_resolution_namespace.as_deref();
                let mut display_name = name.as_str();
                while let Some(current) = namespace {
                    if let Some(relative) = name.strip_prefix(&format!("{current}.")) {
                        display_name = relative;
                        break;
                    }
                    namespace = current.rsplit_once('.').map(|(parent, _)| parent);
                }
                let name = display_name;
                if args.is_empty() {
                    name.to_string()
                } else {
                    format!(
                        "{name}<{}>",
                        args.iter()
                            .map(|arg| self.heritage_display(arg))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }
            }
            _ => ty.display_string(),
        }
    }

    fn heritage_signature_display(&self, signature: &ConstructorType, construct: bool) -> String {
        let generic = if signature.type_params.is_empty() {
            String::new()
        } else {
            format!(
                "<{}>",
                signature
                    .type_params
                    .iter()
                    .enumerate()
                    .map(|(index, name)| {
                        match signature
                            .type_param_constraints
                            .get(index)
                            .and_then(Option::as_ref)
                        {
                            Some(constraint) => {
                                format!("{name} extends {}", self.heritage_display(constraint))
                            }
                            None => name.clone(),
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        let params = signature
            .params
            .iter()
            .map(|(name, ty)| {
                if let Some(name) = name.strip_prefix('?') {
                    let ty = if self.strict_null_checks {
                        Type::flatten_union(vec![ty.clone(), Type::Undefined])
                    } else {
                        ty.clone()
                    };
                    format!("{name}?: {}", self.heritage_display(&ty))
                } else {
                    format!("{name}: {}", self.heritage_display(ty))
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "{}{generic}({params}) => {}",
            if construct { "new " } else { "" },
            self.heritage_display(&signature.return_type)
        )
    }

    pub(super) fn heritage_member_metadata(
        &self,
        ty: &Type,
        member: &str,
        seen: &mut HashSet<String>,
    ) -> Option<(String, bool)> {
        let Type::TypeReference(name, _) = ty else {
            return None;
        };
        if !seen.insert(name.clone()) {
            return None;
        }
        if let Some(interface) = self.interface_info.get(name) {
            if let Some(metadata) = interface.member_names.get(member) {
                return Some(metadata.clone());
            }
            return interface.extends.iter().find_map(|(name, args)| {
                self.heritage_member_metadata(
                    &Type::TypeReference(name.clone(), args.clone()),
                    member,
                    seen,
                )
            });
        }
        if let Some(class) = self.class_info.get(name) {
            if class
                .instance_methods
                .iter()
                .any(|(name, _)| name == member)
            {
                return Some((member.to_string(), true));
            }
        }
        None
    }

    /// Expand only the current relation's operands. Recursive properties stay
    /// lazy so inspecting a heritage clause cannot expand an entire type graph.
    fn heritage_apparent_type(&self, ty: &Type) -> Type {
        if let Type::TypeReference(name, args) = ty {
            // A qualified base (`React.Props`) whose bare name is declared
            // more than once program-wide cannot be resolved by name safely
            // (`Props` would pick an unrelated interface): keep it opaque.
            if let Some((_, bare)) = name.rsplit_once('.') {
                if !self.interface_info.contains_key(name.as_str())
                    && !self.class_info.contains_key(name.as_str())
                    && self.duplicate_type_names.contains(bare)
                {
                    return ty.clone();
                }
            }
            // Namespace exports can also have an alias entry containing an
            // uninstantiated shape. The declaration retains the generic binders.
            if self.interface_info.contains_key(name) || self.class_info.contains_key(name) {
                if let Some(resolved) = self.resolve_type_reference_to_object(name, args) {
                    return resolved;
                }
            }
            if let Some(resolved) = self.resolve_type_for_assignability(ty) {
                if let Type::Mapped {
                    param,
                    constraint,
                    template,
                    name_type,
                    optional_mod,
                    ..
                } = &resolved
                {
                    if let Some(object) = self.eval_mapped_display(
                        param,
                        constraint,
                        template.as_deref(),
                        name_type.as_deref(),
                        *optional_mod,
                        true,
                    ) {
                        return object;
                    }
                }
                return resolved;
            }
            // The built-in utility can be recognized without loading its lib
            // alias. A homomorphic mapped type copies properties, not call or
            // construct signatures, and adds optionality to each property.
            if name == "Partial" && args.len() == 1 {
                if let Type::ObjectType(mut object) = self.heritage_apparent_type(&args[0]) {
                    for (_, ty) in &mut object.properties {
                        if !matches!(ty.as_ref(), Type::Optional(_)) {
                            *ty = Arc::new(Type::Optional(Arc::clone(ty)));
                        }
                    }
                    object.call_signatures.clear();
                    object.construct_signatures.clear();
                    if let Some((_, value)) = &mut object.index_signature {
                        *value = Arc::new(Type::flatten_union(vec![
                            value.as_ref().clone(),
                            Type::Undefined,
                        ]));
                    }
                    return Type::ObjectType(object);
                }
            }
            if let Some(resolved) = self.resolve_type_reference_to_object(name, args) {
                return resolved;
            }
        }
        ty.clone()
    }

    fn heritage_base_property(
        &self,
        ty: &Type,
        member: &str,
        seen: &mut HashSet<String>,
    ) -> Option<Type> {
        if let Type::TypeReference(name, args) = ty {
            if let Some(interface) = self.interface_info.get(name) {
                if !seen.insert(name.clone()) {
                    return None;
                }
                let substitutions = interface
                    .type_params
                    .iter()
                    .cloned()
                    .zip(args.iter().cloned())
                    .collect();
                if let Some((_, property)) = interface
                    .object_type
                    .properties
                    .iter()
                    .find(|(name, _)| name == member)
                {
                    let property = Self::substitute(property, &substitutions);
                    return Some(
                        if interface.optional_props.iter().any(|name| name == member) {
                            Type::Optional(Arc::new(property))
                        } else {
                            property
                        },
                    );
                }
                return interface.extends.iter().find_map(|(base_name, base_args)| {
                    let base = Type::TypeReference(
                        base_name.clone(),
                        base_args
                            .iter()
                            .map(|arg| Self::substitute(arg, &substitutions))
                            .collect(),
                    );
                    self.heritage_base_property(&base, member, seen)
                });
            }
        }
        self.heritage_property(&self.heritage_apparent_type(ty), member)
    }

    fn heritage_property(&self, ty: &Type, name: &str) -> Option<Type> {
        if matches!(ty, Type::Array(_) | Type::Tuple(_)) && name == "length" {
            return Some(Type::Number);
        }
        self.qualified_property_type(ty.clone(), name)
    }

    fn heritage_property_error(
        &self,
        source: &Type,
        target: &Type,
        path: &str,
        depth: usize,
        method: bool,
    ) -> Option<String> {
        self.heritage_property_error_inner(source, target, path, depth, method, &mut HashSet::new())
    }

    fn heritage_property_error_inner(
        &self,
        source: &Type,
        target: &Type,
        path: &str,
        depth: usize,
        method: bool,
        seen: &mut HashSet<(Type, Type)>,
    ) -> Option<String> {
        // A recursive pair is a coinductive assumption. Remembering pairs
        // also prevents a branching recursive interface from expanding once
        // per possible property path before reaching the depth guard.
        if depth > 30 || source == target || !seen.insert((source.clone(), target.clone())) {
            return None;
        }
        if self.heritage_has_base(source, target, &mut HashSet::new()) {
            return None;
        }
        let source_apparent = self.heritage_apparent_type(source);
        let target_apparent = self.heritage_apparent_type(target);
        if let (Type::ObjectType(source_object), Type::ObjectType(target_object)) =
            (&source_apparent, &target_apparent)
        {
            for (name, target_property) in &target_object.properties {
                if let Some((_, source_property)) = source_object
                    .properties
                    .iter()
                    .find(|(member, _)| member == name)
                {
                    let method = self
                        .heritage_member_metadata(target, name, &mut HashSet::new())
                        .is_some_and(|(_, method)| method);
                    if let Some(reason) = self.heritage_property_error_inner(
                        source_property,
                        target_property,
                        &format!("{path}.{name}"),
                        depth + 1,
                        method,
                        seen,
                    ) {
                        return Some(reason);
                    }
                }
            }
        }
        self.heritage_type_error_with_variance(source, target, depth + 1, method)
            .map(|reason| {
                let heading = if depth == 0 {
                    format!("Types of property '{path}' are incompatible.")
                } else {
                    format!("The types of '{path}' are incompatible between these types.")
                };
                format!("{heading}\n{}", indent_heritage_reason(&reason))
            })
    }

    fn heritage_type_error(&self, source: &Type, target: &Type, depth: usize) -> Option<String> {
        self.heritage_type_error_with_variance(source, target, depth, false)
    }

    fn heritage_type_error_with_variance(
        &self,
        source: &Type,
        target: &Type,
        depth: usize,
        method: bool,
    ) -> Option<String> {
        let pair = (source.clone(), target.clone(), method);
        if !self
            .heritage_relation_stack
            .lock()
            .expect("heritage relation lock poisoned")
            .insert(pair.clone())
        {
            return None;
        }
        let result = self.heritage_type_error_inner(source, target, depth, method);
        self.heritage_relation_stack
            .lock()
            .expect("heritage relation lock poisoned")
            .remove(&pair);
        result
    }

    fn heritage_type_error_inner(
        &self,
        source: &Type,
        target: &Type,
        depth: usize,
        method: bool,
    ) -> Option<String> {
        if depth > 30 || source == target {
            return None;
        }
        // An instantiated declared base is a supertype. Its own declaration
        // is checked separately; expanding both sides here needlessly follows
        // covariant recursive methods back through the entire inheritance graph.
        if self.heritage_has_base(source, target, &mut HashSet::new()) {
            return None;
        }
        if let Type::Optional(inner) = source {
            let value_error =
                self.heritage_type_error_with_variance(inner, target, depth + 1, method);
            let undefined_error = self.heritage_type_error(&Type::Undefined, target, depth + 1);
            return value_error.or(undefined_error).map(|reason| {
                format!(
                    "Type '{}' is not assignable to type '{}'.\n{}",
                    self.heritage_display(source),
                    self.heritage_display(target),
                    indent_heritage_reason(&reason)
                )
            });
        }
        if let Type::Optional(inner) = target {
            if matches!(source, Type::Undefined) {
                return None;
            }
            return self.heritage_type_error_with_variance(source, inner, depth + 1, method);
        }
        let target_parameter = match target {
            Type::TypeParameter(name) => Some(name),
            Type::TypeReference(name, args)
                if args.is_empty()
                    && self
                        .active_type_param_names
                        .iter()
                        .any(|names| names.contains(name)) =>
            {
                Some(name)
            }
            _ => None,
        };
        if let Some(name) = target_parameter {
            if !matches!(source, Type::Any | Type::Never | Type::Error) {
                let explanation = if let Some(constraint) = self
                    .typeparam_constraint_apparent(target)
                    .filter(|constraint| {
                        self.is_assignable_to(
                            &self.heritage_apparent_type(source),
                            &self.heritage_apparent_type(constraint),
                        )
                    }) {
                    format!("'{}' is assignable to the constraint of type '{name}', but '{name}' could be instantiated with a different subtype of constraint '{}'.", self.heritage_display(source), self.heritage_display(&constraint))
                } else {
                    format!("'{name}' could be instantiated with an arbitrary type which could be unrelated to '{}'.", self.heritage_display(source))
                };
                return Some(format!(
                    "Type '{}' is not assignable to type '{name}'.\n  {explanation}",
                    self.heritage_display(source)
                ));
            }
        }
        let source_apparent = self.heritage_apparent_type(source);
        let target_apparent = self.heritage_apparent_type(target);
        for construct in [false, true] {
            if let (Some(source_signatures), Some(target_signatures)) = (
                heritage_signatures(&source_apparent, construct),
                heritage_signatures(&target_apparent, construct),
            ) {
                for target_signature in &target_signatures {
                    let errors: Vec<_> = source_signatures
                        .iter()
                        .map(|source_signature| {
                            self.heritage_signature_error(
                                source_signature,
                                target_signature,
                                depth + 1,
                                method,
                            )
                        })
                        .collect();
                    if errors.iter().all(Option::is_some) {
                        let reason = errors.into_iter().flatten().next().unwrap_or_default();
                        return Some(format!(
                            "Type '{}' is not assignable to type '{}'.\n{}",
                            self.heritage_display(source),
                            self.heritage_display(target),
                            indent_heritage_reason(&reason)
                        ));
                    }
                }
                return None;
            }
        }
        if self.is_assignable_to(&source_apparent, &target_apparent) {
            return None;
        }
        if let (Type::ObjectType(source_object), Type::ObjectType(target_object)) =
            (&source_apparent, &target_apparent)
        {
            if let Some((name, _)) = target_object.properties.iter().find(|(name, ty)| {
                !matches!(ty.as_ref(), Type::Optional(_))
                    && !source_object
                        .properties
                        .iter()
                        .any(|(member, _)| member == name)
            }) {
                return Some(format!(
                    "Property '{name}' is missing in type '{}' but required in type '{}'.",
                    self.heritage_display(source),
                    self.heritage_display(target)
                ));
            }
        }
        Some(format!(
            "Type '{}' is not assignable to type '{}'.",
            self.heritage_display(source),
            self.heritage_display(target)
        ))
    }

    fn heritage_has_base(&self, source: &Type, target: &Type, seen: &mut HashSet<String>) -> bool {
        let (Type::TypeReference(name, args), Type::TypeReference(target_name, target_args)) =
            (source, target)
        else {
            return false;
        };
        if name == target_name
            && args.len() == target_args.len()
            && args.iter().zip(target_args.iter()).all(|(source, target)| {
                source == target || matches!(source, Type::Any) || matches!(target, Type::Any)
            })
        {
            return true;
        }
        if !seen.insert(name.clone()) {
            return false;
        }
        let Some(interface) = self.interface_info.get(name) else {
            return false;
        };
        let substitutions = interface
            .type_params
            .iter()
            .cloned()
            .zip(args.iter().cloned())
            .collect();
        interface.extends.iter().any(|(base_name, base_args)| {
            let base = Type::TypeReference(
                base_name.clone(),
                base_args
                    .iter()
                    .map(|arg| Self::substitute(arg, &substitutions))
                    .collect(),
            );
            &base == target || self.heritage_has_base(&base, target, seen)
        })
    }

    fn heritage_signature_error(
        &self,
        source: &ConstructorType,
        target: &ConstructorType,
        depth: usize,
        method: bool,
    ) -> Option<String> {
        let source_required = constructor_required_count(source);
        if let Some(target_count) = constructor_max_count(target) {
            if source_required > target_count {
                return Some(format!("Target signature provides too few arguments. Expected {source_required} or more, but got {target_count}."));
            }
        }
        let mut source = source.clone();
        if !source.type_params.is_empty() && source.type_params == target.type_params {
            // These binders already correspond. Contextual inference would
            // conflate a method's generic with same-named outer arguments.
        } else if !source.type_params.is_empty() {
            let parameters = source.type_params.iter().map(String::as_str).collect();
            let mut substitutions = HashMap::new();
            for ((_, target_type), (_, source_type)) in target.params.iter().zip(&source.params) {
                // Inference into a single callback uses the last overload of
                // its contextual type. Earlier overloads remain available to
                // the subsequent compatibility check.
                let contextual = match (target_type, source_type) {
                    (Type::ObjectType(object), Type::Function(_)) => {
                        object.call_signatures.last().cloned().map(Type::Function)
                    }
                    (Type::ObjectType(object), Type::Constructor(_)) => object
                        .construct_signatures
                        .last()
                        .cloned()
                        .map(Type::Constructor),
                    _ => None,
                };
                Self::infer_from_types(
                    contextual.as_ref().unwrap_or(target_type),
                    source_type,
                    &parameters,
                    &mut substitutions,
                );
            }
            if !matches!(target.return_type.as_ref(), Type::Void | Type::Any) {
                Self::infer_from_types(
                    &target.return_type,
                    &source.return_type,
                    &parameters,
                    &mut substitutions,
                );
            }
            for (index, parameter) in source.type_params.iter().enumerate() {
                let constraint = source
                    .type_param_constraints
                    .get(index)
                    .and_then(Option::as_ref);
                let inferred = substitutions
                    .entry(parameter.clone())
                    .or_insert_with(|| constraint.cloned().unwrap_or(Type::Unknown));
                if let Some(constraint) = constraint {
                    if !self.is_assignable_to(
                        &self.heritage_apparent_type(inferred),
                        &self.heritage_apparent_type(constraint),
                    ) {
                        *inferred = constraint.clone();
                    }
                }
            }
            source.params = source
                .params
                .iter()
                .map(|(name, ty)| (name.clone(), Self::substitute(ty, &substitutions)))
                .collect();
            source.return_type = Arc::new(Self::substitute(&source.return_type, &substitutions));
        }
        let strict = !method
            && self
                .compiler_options
                .strict_function_types
                .unwrap_or(self.compiler_options.strict.unwrap_or(true));
        let compared = constructor_max_count(&source)
            .unwrap_or(source.params.len())
            .max(constructor_max_count(target).unwrap_or(target.params.len()));
        for index in 0..compared {
            let (Some(source_type), Some(target_type)) = (
                constructor_parameter_at(&source, index),
                constructor_parameter_at(target, index),
            ) else {
                continue;
            };
            if !strict
                && self
                    .heritage_type_error(&source_type, &target_type, depth + 1)
                    .is_none()
            {
                continue;
            }
            if let Some(reason) = self.heritage_type_error(&target_type, &source_type, depth + 1) {
                let parameter_name = |signature: &ConstructorType| {
                    signature
                        .params
                        .get(index)
                        .or_else(|| signature.params.last())
                        .map(|(name, _)| name.trim_start_matches('?').trim_start_matches("..."))
                        .unwrap_or("_")
                        .to_string()
                };
                return Some(format!(
                    "Types of parameters '{}' and '{}' are incompatible.\n{}",
                    parameter_name(&source),
                    parameter_name(target),
                    indent_heritage_reason(&reason)
                ));
            }
        }
        if matches!(target.return_type.as_ref(), Type::Void | Type::Any) {
            return None;
        }
        self.heritage_type_error(&source.return_type, &target.return_type, depth + 1)
    }
}

fn heritage_signatures(ty: &Type, construct: bool) -> Option<Vec<ConstructorType>> {
    let from_function = |signature: &FunctionType| ConstructorType {
        is_abstract: false,
        params: signature.params.clone(),
        return_type: Arc::clone(&signature.return_type),
        type_params: signature.type_params.clone(),
        type_param_constraints: signature.type_param_constraints.clone(),
        type_param_defaults: signature.type_param_defaults.clone(),
    };
    match ty {
        Type::Function(signature) if !construct => Some(vec![from_function(signature)]),
        Type::Constructor(signature) if construct => Some(vec![signature.clone()]),
        Type::ObjectType(info) if construct && !info.construct_signatures.is_empty() => {
            Some(info.construct_signatures.clone())
        }
        Type::ObjectType(info) if !construct && !info.call_signatures.is_empty() => {
            Some(info.call_signatures.iter().map(from_function).collect())
        }
        Type::Intersection(parts) => {
            let signatures: Vec<_> = parts
                .iter()
                .filter_map(|part| heritage_signatures(part, construct))
                .flatten()
                .collect();
            (!signatures.is_empty()).then_some(signatures)
        }
        _ => None,
    }
}

fn indent_heritage_reason(reason: &str) -> String {
    reason
        .lines()
        .map(|line| format!("  {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}
