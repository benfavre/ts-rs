//! Public class construct signatures, shared by calls and type relations.
use super::*;

impl TypeChecker {
    fn constructor_class_identity<'a>(&self, mut name: &'a str) -> Option<&'a str> {
        while !self.class_info.contains_key(name) {
            name = name.split_once('.')?.1;
        }
        Some(name)
    }

    pub(crate) fn constructor_context_is_acyclic(&self, name: &str) -> bool {
        let Some(name) = self.constructor_class_identity(name) else {
            return true;
        };
        let Some(own) = self.class_info.get(name) else {
            return true;
        };
        // A class's own signature can provide context even if its base is invalid.
        if own.has_explicit_constructor {
            return true;
        }
        let mut name = name;
        let mut seen = rustc_hash::FxHashSet::default();
        loop {
            if !seen.insert(name) {
                return false;
            }
            let Some(base) = self
                .class_info
                .get(name)
                .and_then(|info| info.extends.as_deref())
            else {
                return true;
            };
            let Some(base) = self.constructor_class_identity(base) else {
                return true;
            };
            name = base;
        }
    }

    pub(crate) fn class_constructor_signatures(&self, name: &str) -> Option<Vec<ConstructorType>> {
        let name = self.constructor_class_identity(name)?;
        let own = self.class_info.get(name)?;
        let mut current = own;
        let mut identity = name;
        let mut seen = rustc_hash::FxHashSet::default();
        let mut heritage = Vec::new();
        // Walk iteratively: long inheritance chains must not consume the Rust stack.
        let mut parameters = loop {
            if !seen.insert(identity) {
                break vec![Vec::new()];
            }
            if current.has_explicit_constructor {
                break if !current.constructor_overloads.is_empty() {
                    current.constructor_overloads.clone()
                } else {
                    vec![current
                        .constructor_params
                        .iter()
                        .map(|(name, ty, optional, rest)| {
                            (
                                if *rest {
                                    format!("...{name}")
                                } else if *optional {
                                    format!("?{name}")
                                } else {
                                    name.clone()
                                },
                                ty.clone(),
                            )
                        })
                        .collect()]
                };
            }
            let Some(base) = &current.extends else {
                break vec![Vec::new()];
            };
            identity = self.constructor_class_identity(base)?;
            let base = self.class_info.get(identity)?;
            heritage.push((current, base));
            current = base;
        };
        // Instantiate each heritage edge once, without constructing temporary
        // return types and generic signatures for every intermediate class.
        for (derived, base) in heritage.into_iter().rev() {
            if base.type_params.is_empty() {
                continue;
            }
            let mut substitutions = HashMap::new();
            for (index, parameter) in base.type_params.iter().enumerate() {
                let argument = derived
                    .extends_type_args
                    .get(index)
                    .cloned()
                    .or_else(|| {
                        base.type_param_defaults
                            .get(index)
                            .and_then(Option::as_ref)
                            .map(|default| Self::substitute(default, &substitutions))
                    })
                    .unwrap_or(Type::Any);
                substitutions.insert(parameter.clone(), argument);
            }
            for signature in &mut parameters {
                for (_, ty) in signature {
                    *ty = Self::substitute(ty, &substitutions);
                }
            }
        }
        let return_type = Arc::new(Type::TypeReference(
            name.to_owned(),
            own.type_params
                .iter()
                .map(|p| Type::TypeParameter(p.clone()))
                .collect(),
        ));
        Some(
            parameters
                .into_iter()
                .map(|params| ConstructorType {
                    is_abstract: own.is_abstract,
                    params,
                    return_type: return_type.clone(),
                    type_params: own.type_params.clone(),
                    type_param_constraints: own.type_param_constraints.clone(),
                    type_param_defaults: own.type_param_defaults.clone(),
                })
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deep_constructor_inheritance_uses_bounded_stack() {
        let mut source = String::from("class C0 {constructor(value:number){}}\n");
        for index in 1..4096 {
            source.push_str(&format!("class C{index} extends C{} {{}}\n", index - 1));
        }
        let file = tsc_rs_parser::parse("deep.ts", &source);
        let mut checker = TypeChecker::new();
        for statement in &file.statements {
            if let StmtKind::ClassDecl(class) = &statement.kind {
                let info = checker.build_class_info(class);
                checker.insert_class_info(class.name.as_ref().unwrap().to_string(), info);
            }
        }
        let signatures = checker.class_constructor_signatures("C4095").unwrap();
        assert_eq!(signatures.len(), 1);
        assert_eq!(
            signatures[0].params,
            vec![("value".to_string(), Type::Number)]
        );
        assert_eq!(
            signatures[0].return_type.as_ref(),
            &Type::TypeReference("C4095".to_string(), Arc::from([]))
        );
    }
}
