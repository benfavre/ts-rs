use super::*;

impl TypeChecker {
    /// Reduce nullish intersections through aliases and generic constraints.
    /// A constraint can prove `T & undefined` impossible, but a constraint
    /// admitting undefined does not prove every instantiation of T admits it.
    pub(super) fn reduce_nullish_intersection(&self, members: Vec<Type>) -> Type {
        let mut reduced = type_ops::simplify_intersection(members);
        if let Type::Intersection(members) = &reduced {
            let constraints: Vec<_> = members
                .iter()
                .filter(|member| self.active_type_parameter_name(member).is_some())
                .filter_map(|member| self.typeparam_constraint_apparent(member))
                .filter(|constraint| !matches!(constraint, Type::Any | Type::Unknown))
                .collect();
            // A generic already constrained to non-nullish values needs no
            // further `{}` guard. Preserve the intersection when its
            // constraint still admits a nullish value excluded by the guard.
            if !constraints.is_empty() {
                reduced = type_ops::simplify_intersection(
                    members
                        .iter()
                        .filter(|member| {
                            self.unknown_union_coverage(member, 0) == 0
                                || !constraints
                                    .iter()
                                    .any(|constraint| self.is_assignable_to(constraint, member))
                        })
                        .cloned()
                        .collect(),
                );
            }
        }
        let Type::Intersection(members) = &reduced else {
            return reduced;
        };
        let Some(nullish) = members
            .iter()
            .find(|ty| matches!(ty, Type::Null | Type::Undefined))
        else {
            return reduced;
        };
        let mut kept = Vec::new();
        for member in members.iter() {
            match self.nullish_intersection_fact(member, nullish, 0) {
                Some(false) => return Type::Never,
                Some(true) if member != nullish => {}
                _ => kept.push(member.clone()),
            }
        }
        type_ops::simplify_intersection(kept)
    }

    fn nullish_intersection_fact(&self, ty: &Type, nullish: &Type, depth: usize) -> Option<bool> {
        if depth >= 16 {
            return None;
        }
        if self.active_type_parameter_name(ty).is_some() {
            if !self.strict_null_checks {
                return Some(true);
            }
            return self
                .typeparam_constraint_apparent(ty)
                .filter(|constraint| constraint != ty)
                .and_then(|constraint| {
                    self.nullish_intersection_fact(&constraint, nullish, depth + 1)
                })
                .filter(|admits| !admits);
        }
        if let Some(fact) = type_ops::nullish_intersection_member(ty, nullish) {
            return Some(fact);
        }
        match ty {
            Type::ObjectType(_)
            | Type::Array(_)
            | Type::Tuple(_)
            | Type::Function(_)
            | Type::Constructor(_)
            | Type::Module(_)
            | Type::Namespace(_) => Some(!self.strict_null_checks),
            Type::TypeReference(name, _)
                if self.class_info.contains_key(name)
                    || self.interface_info.contains_key(name)
                    || self.builtins.instance_members.contains_key(name) =>
            {
                Some(!self.strict_null_checks)
            }
            Type::TypeReference(..) => self
                .resolve_type_for_assignability(ty)
                .or_else(|| match ty {
                    Type::TypeReference(name, args) => {
                        self.resolve_type_reference_to_object(name, args)
                    }
                    _ => None,
                })
                .filter(|resolved| resolved != ty)
                .and_then(|resolved| self.nullish_intersection_fact(&resolved, nullish, depth + 1)),
            Type::Union(members) => {
                let facts: Vec<_> = members
                    .iter()
                    .map(|member| self.nullish_intersection_fact(member, nullish, depth + 1))
                    .collect();
                if facts.contains(&Some(true)) {
                    Some(true)
                } else if facts.iter().all(|fact| *fact == Some(false)) {
                    Some(false)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}
