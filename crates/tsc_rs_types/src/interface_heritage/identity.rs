//! Identity of overlapping inherited properties (TS2320).

use super::*;

impl TypeChecker {
    pub(super) fn check_inherited_property_identity(
        &mut self,
        declaration: &InterfaceDecl,
    ) -> bool {
        let name = self.namespace_owned_type_name(&declaration.name);
        let Some(info) = self.interface_info.get(&name) else {
            return true;
        };
        let class = self.class_info.get(&name);
        let class_base = class.and_then(|class| {
            class
                .extends
                .as_ref()
                .map(|base| (base.clone(), class.extends_type_args.clone().into()))
        });
        if info.extends.len() + usize::from(class_base.is_some()) < 2
            || info.declaration_span != declaration.span
        {
            return true;
        }
        if let Some(file) = &self.current_file_name {
            if !info.decl_file.is_empty() && !Self::paths_refer_to_same_file(file, &info.decl_file)
            {
                return true;
            }
        }
        let mut info = info.clone();
        if let Some(base) = class_base {
            info.extends.insert(0, base);
            info.extends_sources.insert(0, None);
        }
        let class_members: Vec<_> = class
            .into_iter()
            .flat_map(|class| {
                class
                    .instance_properties
                    .iter()
                    .map(|(name, _)| name.as_str())
                    .chain(class.instance_methods.iter().map(|(name, _)| name.as_str()))
            })
            .collect();
        let derived = Type::TypeReference(
            name,
            info.type_params
                .iter()
                .map(|name| Type::TypeReference(name.clone(), Vec::new().into()))
                .collect(),
        );
        let own: HashSet<_> = info
            .object_type
            .properties
            .iter()
            .map(|(name, _)| name.as_str())
            .chain(class_members)
            .collect();
        let mut seen: HashMap<String, Type> = HashMap::new();
        let mut valid = true;
        for (index, (name, args)) in info.extends.iter().enumerate() {
            let name = self.interface_lookup_name_for_source(
                name,
                info.extends_sources
                    .get(index)
                    .and_then(|source| source.as_deref()),
            );
            let base = Type::TypeReference(name, args.clone());
            let members = self.inherited_property_names(&base, &mut HashSet::new());
            for member in &members {
                if own.contains(member.as_str()) {
                    continue;
                }
                if let Some(first_base) = seen.get(member) {
                    if first_base == &base {
                        continue;
                    }
                    let (Some(first_property), Some(property)) = (
                        self.heritage_base_property(first_base, member, &mut HashSet::new()),
                        self.heritage_base_property(&base, member, &mut HashSet::new()),
                    ) else {
                        continue;
                    };
                    let identical =
                        self.inherited_property_flags(first_base, member, &mut HashSet::new())
                            == self.inherited_property_flags(&base, member, &mut HashSet::new())
                            && self.inherited_types_identical(
                                &first_property,
                                &property,
                                0,
                                &mut HashSet::new(),
                            );
                    if !identical {
                        valid = false;
                        let first_name = self.inherited_type_name(first_base);
                        let base_name = self.inherited_type_name(&base);
                        let display_member = self
                            .heritage_member_metadata(&base, member, &mut HashSet::new())
                            .map(|(name, _)| name)
                            .unwrap_or_else(|| member.clone());
                        self.diagnostics.push(Diagnostic {
                            code: 2320,
                            message: format!("Interface '{}' cannot simultaneously extend types '{first_name}' and '{base_name}'.\n  Named property '{display_member}' of types '{first_name}' and '{base_name}' are not identical.", self.inherited_type_name(&derived)),
                            category: DiagnosticCategory::Error,
                            file_name: None,
                            span: declaration.name_span.or(Some(declaration.span)),
                            related: None,
                        });
                    }
                } else {
                    seen.insert(member.clone(), base.clone());
                }
            }
        }
        valid
    }

    /// TS2320 prints the declaration's symbol name without an enclosing
    /// lexical scope, while identity still uses the fully qualified reference.
    fn inherited_type_name(&self, ty: &Type) -> String {
        if let Type::TypeReference(name, args) = ty {
            let name = name.rsplit('.').next().unwrap_or(name);
            return if args.is_empty() {
                name.to_string()
            } else {
                format!(
                    "{name}<{}>",
                    args.iter()
                        .map(|arg| self.inherited_type_name(arg))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
        }
        self.heritage_display(ty)
    }

    fn inherited_property_names(&self, ty: &Type, seen: &mut HashSet<String>) -> Vec<String> {
        if let Type::TypeReference(name, args) = ty {
            if let Some(info) = self.interface_info.get(name) {
                if !seen.insert(name.clone()) {
                    return Vec::new();
                }
                let substitutions = info
                    .type_params
                    .iter()
                    .cloned()
                    .zip(args.iter().cloned())
                    .collect();
                let mut names = Vec::new();
                for (base, arguments) in &info.extends {
                    let base = Type::TypeReference(
                        base.clone(),
                        arguments
                            .iter()
                            .map(|arg| Self::substitute(arg, &substitutions))
                            .collect(),
                    );
                    for member in self.inherited_property_names(&base, seen) {
                        if !names.contains(&member) {
                            names.push(member);
                        }
                    }
                }
                for (member, _) in &info.object_type.properties {
                    if !names.contains(member) {
                        names.push(member.clone());
                    }
                }
                return names;
            }
        }
        match self.heritage_apparent_type(ty) {
            Type::ObjectType(object) => object
                .properties
                .into_iter()
                .map(|(name, _)| name)
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Readonly and non-public declaration identity are independent of a
    /// member's structural type. Inheriting the same private member twice is
    /// valid; two separate declarations with the same spelling are different.
    fn inherited_property_flags(
        &self,
        ty: &Type,
        member: &str,
        seen: &mut HashSet<String>,
    ) -> (bool, Option<(String, bool)>) {
        let Type::TypeReference(name, _) = ty else {
            return (false, None);
        };
        if !seen.insert(name.clone()) {
            return (false, None);
        }
        if let Some(info) = self.interface_info.get(name) {
            if info
                .object_type
                .properties
                .iter()
                .any(|(name, _)| name == member)
            {
                return (info.readonly_members.contains(member), None);
            }
            for (base, args) in &info.extends {
                let base = Type::TypeReference(base.clone(), args.clone());
                if self
                    .heritage_base_property(&base, member, &mut HashSet::new())
                    .is_some()
                {
                    return self.inherited_property_flags(&base, member, seen);
                }
            }
        }
        if let Some(info) = self.class_info.get(name) {
            let readonly = info.readonly_members.contains(member);
            if info.own_private_members.contains(member) {
                return (readonly, Some((name.clone(), true)));
            }
            if info.own_protected_members.contains(member) {
                return (readonly, Some((name.clone(), false)));
            }
            if info
                .instance_properties
                .iter()
                .any(|(name, _)| name == member)
                || info.instance_methods.iter().any(|(name, _)| name == member)
            {
                return (readonly, None);
            }
            if let Some(base) = &info.extends {
                return self.inherited_property_flags(
                    &Type::TypeReference(base.clone(), Vec::new().into()),
                    member,
                    seen,
                );
            }
        }
        (false, None)
    }

    fn inherited_types_identical(
        &self,
        left: &Type,
        right: &Type,
        depth: usize,
        active: &mut HashSet<(Type, Type)>,
    ) -> bool {
        if left == right || depth > 30 {
            return true;
        }
        let pair = (left.clone(), right.clone());
        if !active.insert(pair.clone()) {
            return true;
        }
        let result = self.inherited_types_identical_inner(left, right, depth, active);
        active.remove(&pair);
        result
    }

    fn inherited_types_identical_inner(
        &self,
        left: &Type,
        right: &Type,
        depth: usize,
        active: &mut HashSet<(Type, Type)>,
    ) -> bool {
        match (left, right) {
            (Type::Optional(left), Type::Optional(right)) => {
                let normalize = |ty: &Type| {
                    if self
                        .compiler_options
                        .exact_optional_property_types
                        .unwrap_or(false)
                    {
                        ty.clone()
                    } else {
                        Type::flatten_union(vec![ty.clone(), Type::Undefined])
                    }
                };
                return self.inherited_types_identical(
                    &normalize(left),
                    &normalize(right),
                    depth + 1,
                    active,
                );
            }
            (Type::Readonly(left), Type::Readonly(right))
            | (Type::Rest(left), Type::Rest(right))
            | (Type::Array(left), Type::Array(right)) => {
                return self.inherited_types_identical(left, right, depth + 1, active)
            }
            (Type::Tuple(left), Type::Tuple(right)) => {
                return left.len() == right.len()
                    && left.iter().zip(right.iter()).all(|(left, right)| {
                        self.inherited_types_identical(left, right, depth + 1, active)
                    })
            }
            (Type::Union(left), Type::Union(right))
            | (Type::Intersection(left), Type::Intersection(right)) => {
                return left.len() == right.len()
                    && left.iter().all(|left| {
                        right.iter().any(|right| {
                            self.inherited_types_identical(left, right, depth + 1, active)
                        })
                    })
            }
            (Type::Function(left), Type::Function(right)) => {
                if left.type_predicate != right.type_predicate {
                    return false;
                }
                return self.inherited_signatures_identical(
                    &heritage_signatures(&Type::Function(left.clone()), false).unwrap()[0],
                    &heritage_signatures(&Type::Function(right.clone()), false).unwrap()[0],
                    depth,
                    active,
                );
            }
            (Type::Constructor(left), Type::Constructor(right)) => {
                return self.inherited_signatures_identical(left, right, depth, active)
            }
            (Type::TypeParameter(left), Type::TypeReference(right, args))
            | (Type::TypeReference(left, args), Type::TypeParameter(right))
                if args.is_empty() =>
            {
                return left == right
            }
            _ => {}
        }
        let left_apparent = self.heritage_apparent_type(left);
        let right_apparent = self.heritage_apparent_type(right);
        if let (Type::ObjectType(left_object), Type::ObjectType(right_object)) =
            (&left_apparent, &right_apparent)
        {
            if left_object.properties.len() != right_object.properties.len()
                || left_object.call_signatures.len() != right_object.call_signatures.len()
                || left_object.construct_signatures.len() != right_object.construct_signatures.len()
            {
                return false;
            }
            for (name, left_property) in &left_object.properties {
                let Some((_, right_property)) = right_object
                    .properties
                    .iter()
                    .find(|(member, _)| member == name)
                else {
                    return false;
                };
                if self.inherited_property_flags(left, name, &mut HashSet::new())
                    != self.inherited_property_flags(right, name, &mut HashSet::new())
                    || !self.inherited_types_identical(
                        left_property,
                        right_property,
                        depth + 1,
                        active,
                    )
                {
                    return false;
                }
            }
            for (left, right) in left_object
                .call_signatures
                .iter()
                .zip(&right_object.call_signatures)
            {
                if !self.inherited_types_identical(
                    &Type::Function(left.clone()),
                    &Type::Function(right.clone()),
                    depth + 1,
                    active,
                ) {
                    return false;
                }
            }
            for (left, right) in left_object
                .construct_signatures
                .iter()
                .zip(&right_object.construct_signatures)
            {
                if !self.inherited_signatures_identical(left, right, depth + 1, active) {
                    return false;
                }
            }
            return match (&left_object.index_signature, &right_object.index_signature) {
                (None, None) => true,
                (Some((left_key, left_value)), Some((right_key, right_value))) => {
                    self.inherited_types_identical(left_key, right_key, depth + 1, active)
                        && self.inherited_types_identical(
                            left_value,
                            right_value,
                            depth + 1,
                            active,
                        )
                }
                _ => false,
            };
        }
        if left_apparent != *left || right_apparent != *right {
            return self.inherited_types_identical(
                &left_apparent,
                &right_apparent,
                depth + 1,
                active,
            );
        }
        false
    }

    fn inherited_signatures_identical(
        &self,
        left: &ConstructorType,
        right: &ConstructorType,
        depth: usize,
        active: &mut HashSet<(Type, Type)>,
    ) -> bool {
        if left.params.len() != right.params.len()
            || left.type_params.len() != right.type_params.len()
            || left.is_abstract != right.is_abstract
        {
            return false;
        }
        let substitutions: HashMap<_, _> = left
            .type_params
            .iter()
            .cloned()
            .zip(
                right
                    .type_params
                    .iter()
                    .map(|name| Type::TypeReference(name.clone(), Vec::new().into())),
            )
            .collect();
        for index in 0..left.type_params.len() {
            for (left_values, right_values) in [
                (&left.type_param_constraints, &right.type_param_constraints),
                (&left.type_param_defaults, &right.type_param_defaults),
            ] {
                let left_value = left_values
                    .get(index)
                    .and_then(Option::as_ref)
                    .unwrap_or(&Type::Unknown);
                let right_value = right_values
                    .get(index)
                    .and_then(Option::as_ref)
                    .unwrap_or(&Type::Unknown);
                if !self.inherited_types_identical(
                    &Self::substitute(left_value, &substitutions),
                    right_value,
                    depth + 1,
                    active,
                ) {
                    return false;
                }
            }
        }
        for ((left_name, left_type), (right_name, right_type)) in
            left.params.iter().zip(&right.params)
        {
            if left_name.starts_with('?') != right_name.starts_with('?')
                || left_name.starts_with("...") != right_name.starts_with("...")
                || !self.inherited_types_identical(
                    &Self::substitute(left_type, &substitutions),
                    right_type,
                    depth + 1,
                    active,
                )
            {
                return false;
            }
        }
        self.inherited_types_identical(
            &Self::substitute(&left.return_type, &substitutions),
            &right.return_type,
            depth + 1,
            active,
        )
    }
}
