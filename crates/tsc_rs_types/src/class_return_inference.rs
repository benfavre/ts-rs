//! Receiver-aware resolution of class signatures before registration.
//!
//! The resolution stack follows getReturnTypeOfSignature in pingdotgg/ts-rust
//! checker_p22.rs; see NOTICE.txt. Native class shapes and AST bodies replace
//! the donor's symbols, signature links, and resolution targets.

use std::{cell::RefCell, rc::Rc};

use super::*;

type Key = (bool, std::string::String);

// Lazy resolution enters the declaration's environment, not the method which
// happened to request its signature. Preserve enclosing lexical captures while
// excluding sibling methods' parameter and type-parameter scopes.
#[derive(Clone, Default)]
struct Environment {
    values: Vec<Vec<(String, Type)>>,
    parameters: Vec<rustc_hash::FxHashSet<String>>,
    mapped_parameters: Vec<String>,
    constraints: Vec<rustc_hash::FxHashMap<String, Option<Type>>>,
}

impl Environment {
    fn capture() -> Self {
        Self {
            values: INFER_PARAM_SCOPE.with(|scopes| scopes.borrow().clone()),
            parameters: TYPE_RESOLUTION_TYPE_PARAMS.with(|scopes| scopes.borrow().clone()),
            mapped_parameters: MAPPED_PARAMS_IN_RESOLUTION.with(|scopes| scopes.borrow().clone()),
            constraints: crate::tuple_bindings::INFERRED_CONSTRAINTS
                .with(|scopes| scopes.borrow().clone()),
        }
    }

    fn enter(self) -> EnvironmentGuard {
        EnvironmentGuard(self.replace())
    }

    fn replace(self) -> Self {
        Self {
            values: INFER_PARAM_SCOPE.with(|scopes| scopes.replace(self.values)),
            parameters: TYPE_RESOLUTION_TYPE_PARAMS.with(|scopes| scopes.replace(self.parameters)),
            mapped_parameters: MAPPED_PARAMS_IN_RESOLUTION
                .with(|scopes| scopes.replace(self.mapped_parameters)),
            constraints: crate::tuple_bindings::INFERRED_CONSTRAINTS
                .with(|scopes| scopes.replace(self.constraints)),
        }
    }
}

pub(super) struct EnvironmentGuard(Environment);

/// Cached library declarations are resolved in their own environment, never
/// in the signature which first caused a library file to be loaded.
pub(super) fn isolated_environment() -> EnvironmentGuard {
    Environment::default().enter()
}

impl Drop for EnvironmentGuard {
    fn drop(&mut self) {
        std::mem::take(&mut self.0).replace();
    }
}

#[derive(Clone)]
struct Pending {
    body: Vec<Stmt>,
    params: Vec<Param>,
    type_params: Option<Vec<TypeParam>>,
    signature: Option<FunctionType>,
    index: usize,
    is_async: bool,
}

struct Resolution {
    name: std::string::String,
    environment: Environment,
    info: ClassInfo,
    pending: HashMap<Key, Pending>,
    resolved: HashMap<Key, Type>,
    stack: Vec<Key>,
    circular: HashSet<Key>,
}

#[derive(Clone)]
enum Receiver {
    Class(Rc<RefCell<Resolution>>, bool),
    Function(Type),
}

thread_local! {
    static RECEIVERS: RefCell<Vec<Receiver>> = const { RefCell::new(Vec::new()) };
}

pub(crate) struct ReceiverGuard;

impl Drop for ReceiverGuard {
    fn drop(&mut self) {
        RECEIVERS.with(|stack| {
            stack.borrow_mut().pop();
        });
    }
}

fn enter(receiver: Receiver) -> ReceiverGuard {
    RECEIVERS.with(|stack| stack.borrow_mut().push(receiver));
    ReceiverGuard
}

fn current() -> Option<Receiver> {
    RECEIVERS.with(|stack| stack.borrow().last().cloned())
}

pub(crate) fn active() -> bool {
    matches!(current(), Some(Receiver::Class(..)))
}

pub(crate) fn this_type() -> Option<Type> {
    current().map(|receiver| match receiver {
        Receiver::Function(ty) => ty,
        Receiver::Class(state, true) => {
            Type::TypeReference(format!("typeof {}", state.borrow().name), Arc::from([]))
        }
        Receiver::Class(_, false) => Type::This,
    })
}

impl TypeChecker {
    // A function has its own receiver; arrows intentionally keep the frame.
    pub(crate) fn infer_function_receiver(&self, params: &[Param]) -> Option<ReceiverGuard> {
        current()?;
        let ty = params
            .iter()
            .find_map(|param| {
                matches!(&param.name.kind, PatKind::Ident(name) if name == "this")
                    .then(|| {
                        param
                            .type_ann
                            .as_ref()
                            .map(|node| self.resolve_type_node(node))
                    })
                    .flatten()
            })
            .unwrap_or(Type::Any);
        Some(enter(Receiver::Function(ty)))
    }

    pub(crate) fn refine_class_returns(
        &self,
        declaration: &ClassDecl,
        info: ClassInfo,
    ) -> ClassInfo {
        let _class_parameters =
            crate::type_params_in_resolution(declaration.type_params.as_deref());
        let mut pending = HashMap::new();
        for member in &declaration.members {
            let (name, is_static, item) = match &member.kind {
                ClassMemberKind::Method(method)
                    if method.return_type.is_none() && !method.is_generator =>
                {
                    let Some(body) = &method.body else { continue };
                    let name = self.prop_name_to_string(&method.name);
                    let is_static = method.modifiers & MOD_STATIC != 0;
                    let methods = if is_static {
                        &info.static_methods
                    } else {
                        &info.instance_methods
                    };
                    let Some((index, (_, signature))) = methods
                        .iter()
                        .enumerate()
                        .find(|(_, (key, _))| key == &name)
                    else {
                        continue;
                    };
                    // Overload declarations supply the callable signature.
                    // Their implementation is independently checked later.
                    if methods.iter().filter(|(key, _)| key == &name).count() != 1 {
                        continue;
                    }
                    (
                        name,
                        is_static,
                        Pending {
                            body: body.clone(),
                            params: method.params.clone(),
                            type_params: method.type_params.clone(),
                            signature: Some(signature.clone()),
                            index,
                            is_async: method.is_async,
                        },
                    )
                }
                ClassMemberKind::GetAccessor(accessor) if accessor.return_type.is_none() => {
                    let Some(body) = &accessor.body else { continue };
                    let name = self.prop_name_to_string(&accessor.name);
                    let is_static = accessor.modifiers & MOD_STATIC != 0;
                    let properties = if is_static {
                        &info.static_properties
                    } else {
                        &info.instance_properties
                    };
                    let Some(index) = properties.iter().position(|(key, _)| key == &name) else {
                        continue;
                    };
                    (
                        name,
                        is_static,
                        Pending {
                            body: body.clone(),
                            params: accessor.params.clone(),
                            type_params: accessor.type_params.clone(),
                            signature: None,
                            index,
                            is_async: false,
                        },
                    )
                }
                _ => continue,
            };
            pending.entry((is_static, name)).or_insert(item);
        }
        if pending.is_empty() {
            return info;
        }
        // Resolve in source order for deterministic circular-signature recovery.
        let keys: Vec<_> = declaration
            .members
            .iter()
            .filter_map(|member| {
                let (name, flags) = match &member.kind {
                    ClassMemberKind::Method(m) => (&m.name, m.modifiers),
                    ClassMemberKind::GetAccessor(a) => (&a.name, a.modifiers),
                    _ => return None,
                };
                let key = (flags & MOD_STATIC != 0, self.prop_name_to_string(name));
                pending.contains_key(&key).then_some(key)
            })
            .collect();
        let state = Rc::new(RefCell::new(Resolution {
            environment: Environment::capture(),
            name: declaration
                .name
                .clone()
                .unwrap_or_else(|| format!("__class_expression_{}", declaration.span.start)),
            info,
            pending,
            resolved: HashMap::new(),
            stack: Vec::new(),
            circular: HashSet::new(),
        }));
        for key in keys {
            self.resolve_class_return(&state, &key);
        }
        Rc::try_unwrap(state)
            .ok()
            .expect("class inference scope escaped")
            .into_inner()
            .info
    }

    fn resolve_class_return(&self, state: &Rc<RefCell<Resolution>>, key: &Key) -> Type {
        let item = {
            let mut resolution = state.borrow_mut();
            if let Some(ty) = resolution.resolved.get(key) {
                return ty.clone();
            }
            let item = resolution.pending[key].clone();
            if let Some(index) = resolution.stack.iter().position(|entry| entry == key) {
                let cycle = resolution.stack[index..].to_vec();
                resolution.circular.extend(cycle);
                return item
                    .signature
                    .map(|mut signature| {
                        signature.return_type = Arc::new(Type::Any);
                        Type::Function(signature)
                    })
                    .unwrap_or(Type::Any);
            }
            resolution.stack.push(key.clone());
            item
        };
        let _receiver = enter(Receiver::Class(Rc::clone(state), key.0));
        let _environment = state.borrow().environment.clone().enter();
        let _explicit_receiver = item
            .params
            .iter()
            .any(|parameter| matches!(&parameter.name.kind, PatKind::Ident(name) if name == "this"))
            .then(|| self.infer_function_receiver(&item.params));
        let _parameters = crate::type_params_in_resolution(item.type_params.as_deref());
        let _constraints = self.inferred_constraint_scope(item.type_params.as_deref());
        let mut bindings = Vec::new();
        if let Some(signature) = &item.signature {
            for (parameter, (_, ty)) in item.params.iter().zip(&signature.params) {
                self.collect_inferred_pattern_bindings(
                    &parameter.name,
                    ty.clone(),
                    false,
                    parameter.type_ann.is_some(),
                    &mut bindings,
                );
            }
        }
        let inferred = crate::with_infer_param_scope(bindings, || {
            self.infer_return_type_from_stmts(&item.body)
        });
        // Resolve intrinsic aliases in their declaration environment before a
        // caller's same-named generic parameter can reinterpret the reference.
        let inferred = if matches!(&inferred, Type::TypeReference(name, args)
            if args.is_empty() && MAPPED_PARAMS_IN_RESOLUTION.with(|scopes| scopes.borrow().contains(name)))
        {
            inferred
        } else {
            self.display_type_through_primitive_alias(&inferred)
        };
        let circular = {
            let mut resolution = state.borrow_mut();
            assert_eq!(resolution.stack.pop().as_ref(), Some(key));
            resolution.circular.contains(key)
        };
        let inferred = if circular {
            Type::Any
        } else if item.is_async {
            self.wrap_async_inferred(inferred)
        } else {
            inferred
        };
        let ty = if let Some(mut signature) = item.signature {
            // The syntactic return serializer can reuse the annotation of a
            // directly returned assertion, even when its primitive alias has
            // been resolved for semantic inference.
            let return_annotation = match item.body.as_slice() {
                [Stmt {
                    kind: StmtKind::Return(Some(expression)),
                    ..
                }] if !Self::is_const_assertion_expr(expression) => match &expression.kind {
                    ExprKind::As(assertion) => Some(&assertion.type_node),
                    ExprKind::TypeAssertion(assertion) => Some(&assertion.type_node),
                    _ => None,
                },
                _ => None,
            };
            signature.source = self.signature_source(
                &item.params,
                &signature.params,
                item.type_params.as_deref(),
                return_annotation,
                &inferred,
            );
            signature.return_type = Arc::new(inferred);
            let mut resolution = state.borrow_mut();
            let methods = if key.0 {
                &mut resolution.info.static_methods
            } else {
                &mut resolution.info.instance_methods
            };
            methods[item.index].1 = signature.clone();
            Type::Function(signature)
        } else {
            let mut resolution = state.borrow_mut();
            let properties = if key.0 {
                &mut resolution.info.static_properties
            } else {
                &mut resolution.info.instance_properties
            };
            properties[item.index].1 = inferred.clone();
            inferred
        };
        state.borrow_mut().resolved.insert(key.clone(), ty.clone());
        ty
    }

    pub(crate) fn infer_receiver_member(&self, ty: &Type, name: &str) -> Option<Type> {
        // An alias captured by a nested function still refers to the lexical
        // class receiver; the function's own `this` has already become its
        // explicit receiver type (or any) before member lookup.
        let (state, is_static) = RECEIVERS.with(|stack| {
            stack.borrow().iter().rev().find_map(|receiver| {
                if let Receiver::Class(state, is_static) = receiver {
                    Some((Rc::clone(state), *is_static))
                } else {
                    None
                }
            })
        })?;
        let class_name = state.borrow().name.clone();
        let (is_static, type_args) = match ty {
            Type::This if !is_static => (false, &[][..]),
            Type::TypeReference(name, args) if name == &class_name => (false, args.as_ref()),
            Type::TypeReference(name, _) if name == &format!("typeof {class_name}") => {
                (true, &[][..])
            }
            _ => return None,
        };
        let key = (is_static, name.to_owned());
        let pending = state.borrow().pending.contains_key(&key);
        let value = if pending {
            Some(self.resolve_class_return(&state, &key))
        } else {
            let resolution = state.borrow();
            let info = &resolution.info;
            let (properties, methods, overloads) = if is_static {
                (
                    &info.static_properties,
                    &info.static_methods,
                    &info.overloaded_static_methods,
                )
            } else {
                (
                    &info.instance_properties,
                    &info.instance_methods,
                    &info.overloaded_methods,
                )
            };
            properties
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, t)| t.clone())
                .or_else(|| overloads.get(name).cloned())
                .or_else(|| {
                    methods
                        .iter()
                        .find(|(n, _)| n == name)
                        .map(|(_, f)| Type::Function(f.clone()))
                })
        };
        if let Some(value) = value {
            let resolution = state.borrow();
            let value = if type_args.is_empty() {
                value
            } else {
                let substitutions = resolution
                    .info
                    .type_params
                    .iter()
                    .cloned()
                    .zip(type_args.iter().cloned())
                    .collect();
                Self::substitute(&value, &substitutions)
            };
            let optional = if is_static {
                &resolution.info.optional_static_properties
            } else {
                &resolution.info.optional_instance_properties
            };
            return Some(if self.strict_null_checks && optional.contains(name) {
                Type::flatten_union(vec![value, Type::Undefined])
            } else {
                value
            });
        }
        let (base, args) = {
            let resolution = state.borrow();
            (
                resolution.info.extends.clone(),
                resolution.info.extends_type_args.clone(),
            )
        };
        Some(
            base.map(|base| {
                if is_static {
                    self.direct_constructor_static_members(&Type::TypeReference(
                        format!("typeof {base}"),
                        Arc::from([]),
                    ))
                    .and_then(|members| {
                        members
                            .into_iter()
                            .find(|(n, _)| n == name)
                            .map(|(_, ty)| ty)
                    })
                    .unwrap_or(Type::Any)
                } else {
                    self.resolve_member_on_type(&Type::TypeReference(base, args.into()), name)
                }
            })
            .unwrap_or(Type::Any),
        )
    }
}
