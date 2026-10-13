use super::*;

impl TypeChecker {
    /// A spread embeds the elements of its sequence. Readonly applies to
    /// the containing sequence, not to the values supplied by the spread.
    pub(super) fn expanded_tuple_elements(&self, elements: &[Type], depth: usize) -> Vec<Type> {
        if depth >= 8 {
            return elements.to_vec();
        }
        let mut result = Vec::new();
        for element in elements {
            if let Type::Rest(inner) = element {
                // An uninstantiated spread remains the type parameter itself.
                // Its array constraint does not admit arbitrary arrays: T may
                // later be instantiated as a narrower tuple.
                if matches!(inner.as_ref(), Type::TypeParameter(_))
                    || self.active_type_parameter_name(inner).is_some()
                {
                    result.push(element.clone());
                    continue;
                }
                let shape = self
                    .array_or_tuple_type(inner, 0)
                    .unwrap_or_else(|| Type::clone(inner));
                let shape = match &shape {
                    Type::Readonly(inner) => inner.as_ref(),
                    other => other,
                };
                match shape {
                    Type::Tuple(items) => {
                        result.extend(self.expanded_tuple_elements(items, depth + 1));
                    }
                    Type::Array(_) => result.push(Type::Rest(Arc::new(shape.clone()))),
                    _ => result.push(element.clone()),
                }
            } else {
                result.push(element.clone());
            }
        }
        result
    }

    pub(super) fn const_tuple_context(&self, ty: Type, context: &Type, expression: &Expr) -> Type {
        let expression = match &expression.kind {
            ExprKind::As(assertion) => assertion.expr.as_ref(),
            ExprKind::TypeAssertion(assertion) => assertion.expr.as_ref(),
            ExprKind::Paren(inner) => inner.as_ref(),
            _ => expression,
        };
        let ExprKind::ArrayLit(elements) = &expression.kind else {
            // A const assertion freezes literal syntax, not referenced values.
            return ty;
        };
        fn sequences(checker: &TypeChecker, context: &Type, out: &mut Vec<Type>) {
            if let Type::Union(members) = context {
                for member in members.iter() {
                    sequences(checker, member, out);
                }
            } else if let Some(shape) = checker.array_or_tuple_type(context, 0) {
                match shape {
                    Type::Union(members) => out.extend(members.iter().cloned()),
                    other => out.push(other),
                }
            }
        }
        let mut contexts = Vec::new();
        sequences(self, context, &mut contexts);
        if contexts.is_empty() {
            return ty;
        }
        let mutable = contexts
            .iter()
            .any(|ty| matches!(ty, Type::Array(_) | Type::Tuple(_)));
        let (shape, readonly) = match &ty {
            Type::Readonly(inner) => (inner.as_ref(), true),
            other => (other, false),
        };
        let Type::Tuple(items) = shape else {
            return ty;
        };
        let mut items = items.to_vec();
        // A spread can occupy more than one tuple slot. Do not align the
        // following syntax elements with a slot until its width is known.
        for (index, element) in elements.iter().enumerate() {
            let Some(element) = element else {
                continue;
            };
            if matches!(element.kind, ExprKind::Spread(_)) {
                break;
            }
            let Some(item) = items.get_mut(index) else {
                break;
            };
            let context = Type::flatten_union(
                contexts
                    .iter()
                    .map(|context| self.extract_element_type(context, index))
                    .collect(),
            );
            *item = self.const_tuple_context(item.clone(), &context, element);
        }
        let result = Type::Tuple(items.into());
        if readonly && !mutable {
            Type::Readonly(Arc::new(result))
        } else {
            result
        }
    }

    pub(super) fn readonly_missing_properties(&self, source: &Type, target: &Type) -> Vec<String> {
        if !matches!(self.array_or_tuple_type(source, 0), Some(Type::Readonly(_))) {
            return Vec::new();
        }
        let target = self
            .resolve_type_for_assignability(target)
            .unwrap_or_else(|| target.clone());
        let Type::ObjectType(info) = target else {
            return Vec::new();
        };
        info.properties
            .iter()
            .filter(|(name, ty)| {
                !name.starts_with('?')
                    && !matches!(ty.as_ref(), Type::Optional(_))
                    && name.parse::<usize>().is_err()
                    && self.readonly_sequence_member_missing(name)
            })
            .map(|(name, _)| name.clone())
            .collect()
    }

    pub(super) fn array_keyof_accepts(&self, key: &Type, target: &Type) -> Option<bool> {
        let Type::Keyof(object) = target else {
            return None;
        };
        let shape = self.array_or_tuple_type(object, 0)?;
        fn contains(
            checker: &TypeChecker,
            key: &Type,
            shape: &Type,
            readonly: bool,
        ) -> Option<bool> {
            if let Type::Union(keys) = key {
                return keys
                    .iter()
                    .map(|key| contains(checker, key, shape, readonly))
                    .collect::<Option<Vec<_>>>()
                    .map(|results| results.into_iter().all(|value| value));
            }
            match shape {
                Type::Readonly(inner) => return contains(checker, key, inner, true),
                Type::Union(members) => {
                    return members
                        .iter()
                        .map(|member| contains(checker, key, member, readonly))
                        .collect::<Option<Vec<_>>>()
                        .map(|results| results.into_iter().all(|value| value))
                }
                _ => {}
            }
            let owner = if readonly { "ReadonlyArray" } else { "Array" };
            match key {
                Type::Number | Type::NumberLiteral(_) => Some(true),
                Type::String | Type::Symbol | Type::Boolean | Type::BooleanLiteral(_) => {
                    Some(false)
                }
                Type::StringLiteral(name) => {
                    if let Type::Tuple(items) = shape {
                        if name.parse::<usize>().is_ok_and(|index| {
                            index.to_string() == *name
                                && index
                                    < items
                                        .iter()
                                        .position(|item| matches!(item, Type::Rest(_)))
                                        .unwrap_or(items.len())
                        }) {
                            return Some(true);
                        }
                    }
                    stdlib::stdlib_member_availability(&checker.compiler_options)?
                        .declares_member(owner, name)
                }
                _ => None,
            }
        }
        contains(self, key, &shape, false)
    }

    pub(super) fn readonly_assignment_reason(
        &self,
        source: &Type,
        target: &Type,
    ) -> Option<String> {
        let source_shape = self.array_or_tuple_type(source, 0)?;
        let target_shape = self.array_or_tuple_type(target, 0)?;
        if matches!(source_shape, Type::Readonly(_))
            && matches!(target_shape, Type::Array(_) | Type::Tuple(_))
        {
            Some(format!(
                "The type '{}' is 'readonly' and cannot be assigned to the mutable type '{}'.",
                source.display_string_single_line(),
                target.display_string_single_line(),
            ))
        } else {
            None
        }
    }

    pub(super) fn readonly_sequence_member_missing(&self, property: &str) -> bool {
        stdlib::stdlib_member_availability(&self.compiler_options).is_some_and(|library| {
            library.declares_member("ReadonlyArray", property) == Some(false)
                && library.declares_member("Object", property) != Some(true)
        })
    }

    pub(super) fn readonly_sequence_member_type(&self, inner: &Type, property: &str) -> Type {
        let mut ty = self.resolve_member_on_type(inner, property);
        // Array callbacks receive the original readonly array. Array-producing
        // methods (slice/map/filter) still return a new mutable array.
        if let Type::Function(function) = &mut ty {
            for (_, parameter) in &mut function.params {
                if let Type::Function(callback) = parameter {
                    for (name, parameter) in &mut callback.params {
                        if name == "array" && matches!(parameter, Type::Array(_)) {
                            *parameter = Type::Readonly(Arc::new(parameter.clone()));
                        }
                    }
                }
            }
        }
        ty
    }
}
