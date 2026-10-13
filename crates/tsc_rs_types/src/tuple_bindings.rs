use super::*;

thread_local! {
    pub(super) static INFERRED_CONSTRAINTS: RefCell<Vec<rustc_hash::FxHashMap<String, Option<Type>>>> =
        const { RefCell::new(Vec::new()) };
}

pub(super) struct InferredConstraintScope;

impl Drop for InferredConstraintScope {
    fn drop(&mut self) {
        INFERRED_CONSTRAINTS.with(|frames| frames.borrow_mut().pop());
    }
}

pub(super) fn inferred_constraint(ty: &Type) -> Option<Option<Type>> {
    let name = match ty {
        Type::TypeParameter(name) => name,
        Type::TypeReference(name, args) if args.is_empty() => name,
        _ => return None,
    };
    INFERRED_CONSTRAINTS.with(|frames| {
        frames
            .borrow()
            .iter()
            .rev()
            .find_map(|frame| frame.get(name.as_str()).cloned())
    })
}

impl TypeChecker {
    /// Signature hoisting runs without a mutable checker scope. Keep generic
    /// constraints lexical during that inference, including unconstrained
    /// parameters which shadow an enclosing parameter of the same name.
    pub(super) fn inferred_constraint_scope(
        &self,
        parameters: Option<&[TypeParam]>,
    ) -> InferredConstraintScope {
        let frame = parameters
            .unwrap_or(&[])
            .iter()
            .map(|parameter| {
                (
                    self.resolved_type_parameter_name(&parameter.name)
                        .unwrap_or_else(|| parameter.name.clone()),
                    parameter
                        .constraint
                        .as_ref()
                        .map(|node| self.resolve_type_node(node)),
                )
            })
            .collect();
        INFERRED_CONSTRAINTS.with(|frames| frames.borrow_mut().push(frame));
        InferredConstraintScope
    }

    pub(super) fn collect_inferred_pattern_bindings(
        &self,
        pattern: &Pat,
        ty: Type,
        preserve_default_literals: bool,
        annotated: bool,
        bindings: &mut Vec<(String, Type)>,
    ) {
        match &pattern.kind {
            PatKind::Ident(name) => bindings.push((name.to_string(), ty)),
            PatKind::Array(elements) => {
                for (index, element) in elements.iter().enumerate() {
                    let (pattern, value) = match element {
                        Some(ArrayPatElem::Pat(pattern)) => {
                            (pattern, self.extract_element_type(&ty, index))
                        }
                        Some(ArrayPatElem::Rest(pattern)) => {
                            (pattern, self.array_rest_binding_type(&ty, index))
                        }
                        None => continue,
                    };
                    self.collect_inferred_pattern_bindings(
                        pattern,
                        value,
                        preserve_default_literals,
                        annotated,
                        bindings,
                    );
                }
            }
            PatKind::Object(properties) => {
                let consumed = self.object_pattern_consumed_names(properties);
                for property in properties {
                    match property {
                        ObjPatProp::Shorthand(name, _) => {
                            bindings.push((name.to_string(), self.extract_member_type(&ty, name)));
                        }
                        ObjPatProp::ShorthandAssign(name, default, _) => {
                            let value = crate::with_infer_param_scope(bindings.clone(), || {
                                self.binding_initializer_type(
                                    self.extract_member_type_opt(&ty, name)
                                        .or_else(|| matches!(ty, Type::Any).then_some(Type::Any)),
                                    default,
                                    preserve_default_literals,
                                    annotated,
                                )
                            });
                            bindings.push((name.to_string(), value));
                        }
                        ObjPatProp::KeyValue(key, nested) => {
                            let value = self
                                .pattern_property_name(key)
                                .and_then(|name| self.extract_member_type_opt(&ty, &name))
                                .unwrap_or(Type::Any);
                            self.collect_inferred_pattern_bindings(
                                nested,
                                value,
                                preserve_default_literals,
                                annotated,
                                bindings,
                            );
                        }
                        ObjPatProp::Rest(nested) => self.collect_inferred_pattern_bindings(
                            nested,
                            self.object_rest_binding_type(&ty, &consumed),
                            preserve_default_literals,
                            annotated,
                            bindings,
                        ),
                    }
                }
            }
            PatKind::Assign(nested, default) => {
                let value = crate::with_infer_param_scope(bindings.clone(), || {
                    self.binding_initializer_type(
                        Some(ty),
                        default,
                        preserve_default_literals,
                        annotated,
                    )
                });
                self.collect_inferred_pattern_bindings(
                    nested,
                    value,
                    preserve_default_literals,
                    annotated,
                    bindings,
                );
            }
            PatKind::Rest(nested) => {
                self.collect_inferred_pattern_bindings(
                    nested,
                    ty,
                    preserve_default_literals,
                    annotated,
                    bindings,
                );
            }
        }
    }

    /// An explicit annotation fixes each binding's type. Defaults remove
    /// undefined from that type; they cannot add an incompatible alternative.
    pub(super) fn binding_initializer_type(
        &self,
        projected: Option<Type>,
        default: &Expr,
        preserve: bool,
        annotated: bool,
    ) -> Type {
        if annotated {
            if let Some(ty) = projected {
                if self.strict_null_checks
                    && !Self::type_includes_undefined(&self.infer_expr_type(default))
                {
                    return match ty {
                        Type::Undefined => Type::Never,
                        Type::Optional(inner) => Type::clone(&inner),
                        ty => self.remove_undefined_only(&ty),
                    };
                }
                return ty;
            }
        }
        self.binding_default_type(projected, default, preserve)
    }

    fn binding_source_type(&self, ty: &Type) -> Type {
        self.array_or_tuple_type(ty, 0).unwrap_or_else(|| {
            self.resolve_type_for_assignability(ty)
                .unwrap_or_else(|| ty.clone())
        })
    }

    /// Destructuring projects each union constituent separately. Unlike an
    /// element access on an array/tuple union, it retains known tuple slots.
    pub(super) fn binding_element_type(&self, ty: &Type, index: usize) -> Option<Type> {
        let shape = self.binding_source_type(ty);
        let unchecked = self.strict_null_checks
            && self.compiler_options.no_unchecked_indexed_access == Some(true);
        match &shape {
            Type::Union(members) => Some(Type::flatten_union(
                members
                    .iter()
                    .map(|member| self.extract_element_type(member, index))
                    .collect(),
            )),
            Type::Readonly(inner) => self.binding_element_type(inner, index),
            Type::Array(value) => Some(if unchecked {
                Type::flatten_union(vec![Type::clone(value), Type::Undefined])
            } else {
                Type::clone(value)
            }),
            _ => tuple_access::indexed_type(
                &shape,
                &Type::NumberLiteral(index.to_string()),
                self.strict_null_checks,
                unchecked,
                false,
            ),
        }
    }

    /// Array rest creates a new mutable container. Consuming into a variadic
    /// section loses the tuple's fixed tail positions, yielding an array.
    pub(super) fn array_rest_binding_type(&self, ty: &Type, start: usize) -> Type {
        let shape = self.binding_source_type(ty);
        match &shape {
            Type::Readonly(inner) => self.array_rest_binding_type(inner, start),
            Type::Union(members) => Type::flatten_union(
                members
                    .iter()
                    .map(|member| self.array_rest_binding_type(member, start))
                    .collect(),
            ),
            Type::Tuple(elements) => {
                if let Some(rest) = elements.iter().position(|ty| matches!(ty, Type::Rest(_))) {
                    if start > rest || start == rest && rest + 1 == elements.len() {
                        let remaining = Type::Tuple(elements.iter().skip(rest).cloned().collect());
                        let element = tuple_access::indexed_type(
                            &remaining,
                            &Type::Number,
                            self.strict_null_checks,
                            false,
                            false,
                        )
                        .unwrap_or(Type::Any);
                        return Type::Array(Arc::new(element));
                    }
                }
                Type::Tuple(
                    elements
                        .iter()
                        .skip(start)
                        .map(|element| match element {
                            Type::Optional(inner) if self.strict_null_checks => {
                                Type::Optional(Arc::new(Type::flatten_union(vec![
                                    Type::clone(inner),
                                    Type::Undefined,
                                ])))
                            }
                            other => other.clone(),
                        })
                        .collect(),
                )
            }
            Type::Array(element) => Type::Array(Arc::clone(element)),
            Type::TypeReference(name, args)
                if matches!(name.as_str(), "Array" | "ReadonlyArray") && args.len() == 1 =>
            {
                Type::Array(Arc::new(args[0].clone()))
            }
            _ => shape,
        }
    }
}
