//! Type annotation resolution, template literal evaluation, function type
//! resolution, and lightweight expression type inference for TypeChecker.

use std::sync::Arc;

use num_traits::ToPrimitive;

use super::*;

/// Widen literal types to their base types (e.g., StringLiteral → String).
/// Used for parameter types inferred from default initializers.
fn widen_literal(ty: Type) -> Type {
    match ty {
        Type::StringLiteral(_) => Type::String,
        Type::NumberLiteral(_) => Type::Number,
        Type::BooleanLiteral(_) => Type::Boolean,
        Type::BigIntLiteral(_) => Type::BigInt,
        other => other,
    }
}

impl TypeChecker {
    // -----------------------------------------------------------------------
    // Type annotation resolution
    // -----------------------------------------------------------------------

    /// Remember where each named member of a type literal is declared, so a
    /// missing-property error against it can say "'x' is declared here.".
    fn record_type_literal_member_locations(&self, ty: &Type, members: &[TypeMember]) {
        let mut table = self
            .type_literal_member_locations
            .lock()
            .expect("type_literal_member_locations lock poisoned");
        if table.contains_key(ty) {
            return;
        }
        let file = self
            .injected_decl_file
            .as_ref()
            .or(self.current_file_name.as_ref())
            .cloned()
            .unwrap_or_default();
        let mut locations = rustc_hash::FxHashMap::default();
        for member in members {
            let name = match &member.kind {
                TypeMemberKind::PropertySig(property) => &property.name,
                TypeMemberKind::MethodSig(method) => &method.name,
                TypeMemberKind::GetAccessorSig(accessor)
                | TypeMemberKind::SetAccessorSig(accessor) => &accessor.name,
                _ => continue,
            };
            locations
                .entry(self.prop_name_to_string(name))
                .or_insert_with(|| (file.clone(), name.span()));
        }
        if !locations.is_empty() {
            table.insert(ty.clone(), locations);
        }
    }

    /// The object-literal counterpart of
    /// [`Self::record_type_literal_member_locations`], for a variable whose
    /// type is inferred from its initializer (recorded both as inferred and
    /// widened, since either may reach a relation check).
    pub(crate) fn record_object_literal_member_locations(&self, ty: &Type, props: &[ObjLitProp]) {
        let file = self
            .injected_decl_file
            .as_ref()
            .or(self.current_file_name.as_ref())
            .cloned()
            .unwrap_or_default();
        let mut locations = rustc_hash::FxHashMap::default();
        for prop in props {
            let (name, span) = match prop {
                ObjLitProp::Property(property) if !property.computed => {
                    (self.prop_name_to_string(&property.key), property.key.span())
                }
                ObjLitProp::Shorthand(name, span) => (name.to_string(), *span),
                ObjLitProp::Method(method) => {
                    (self.prop_name_to_string(&method.name), method.name.span())
                }
                ObjLitProp::Get(accessor) | ObjLitProp::Set(accessor) => (
                    self.prop_name_to_string(&accessor.name),
                    accessor.name.span(),
                ),
                _ => continue,
            };
            locations
                .entry(name)
                .or_insert_with(|| (file.clone(), span));
        }
        if locations.is_empty() {
            return;
        }
        let widened = self.widen_type(ty);
        let mut table = self
            .type_literal_member_locations
            .lock()
            .expect("type_literal_member_locations lock poisoned");
        if widened != *ty {
            table.entry(widened).or_insert_with(|| locations.clone());
        }
        table.entry(ty.clone()).or_insert(locations);
    }

    pub(crate) fn resolve_type_node(&self, node: &TypeNode) -> Type {
        if !self.enter_recursion() {
            return Type::Any;
        }
        let result = self.resolve_type_node_inner(node);
        self.exit_recursion();
        result
    }

    pub(crate) fn resolve_type_node_inner(&self, node: &TypeNode) -> Type {
        match &node.kind {
            TypeNodeKind::Keyword(kw) => match kw {
                KeywordTypeKind::Any => Type::Any,
                KeywordTypeKind::Unknown => Type::Unknown,
                KeywordTypeKind::Number => Type::Number,
                KeywordTypeKind::BigInt => Type::BigInt,
                KeywordTypeKind::String => Type::String,
                KeywordTypeKind::Boolean => Type::Boolean,
                KeywordTypeKind::Void => Type::Void,
                KeywordTypeKind::Undefined => Type::Undefined,
                KeywordTypeKind::Null => Type::Null,
                KeywordTypeKind::Never => Type::Never,
                KeywordTypeKind::Object => Type::Object,
                KeywordTypeKind::Symbol => Type::Symbol,
                KeywordTypeKind::Intrinsic => Type::Any,
            },
            TypeNodeKind::Reference(type_ref) => {
                let mut name = self.expr_to_name(&type_ref.name);
                name = self.namespace_owned_type_name(&name);
                // A named class expression has a body-local type binding even
                // though its source name does not escape the expression. Its
                // value binding points at the synthetic class identity created
                // by check_expr; preserve that identity in member annotations
                // instead of colliding with an outer same-named class.
                if matches!(type_ref.name.kind, ExprKind::Ident(_)) {
                    if let Some(Type::TypeReference(value_name, value_args)) =
                        self.lookup_var(&name)
                    {
                        if value_args.is_empty() {
                            if let Some(identity) =
                                value_name.strip_prefix("typeof __class_expression_")
                            {
                                name = format!("__class_expression_{identity}");
                            }
                        }
                    }
                }
                // Note: No cycle detection needed here. This function only builds
                // TypeReference(name, args) without expanding the reference. Actual
                // expansion (and cycle risk) happens in resolve_type_reference_to_object
                // which has its own cycle detection.  Keep namespace-qualified
                // references intact as well: reducing `core.output<T>` to `any`
                // loses exactly the alias information the guarded qualified-name
                // resolver needs later.  Its namespace-binding check prevents an
                // arbitrary dotted name from colliding with an unrelated global
                // interface. The depth guard in resolve_type_node
                // (MAX_RECURSION_DEPTH=50) prevents stack overflow.
                let args: Arc<[Type]> = type_ref
                    .type_args
                    .as_ref()
                    .map(|args| args.iter().map(|a| self.resolve_type_node(a)).collect())
                    .unwrap_or_default();
                // Expand Array<T> and ReadonlyArray<T> to T[]
                if matches!(name.as_str(), "Array" | "ReadonlyArray") && args.len() == 1 {
                    return Type::Array(Arc::new(args.first().cloned().unwrap()));
                }
                // Module-scoped resolution for collision-prone (duplicate) bare
                // names that this file imported as a type — resolve through the
                // real source module instead of the ambiguous global registry.
                if let Some(t) = self.try_resolve_imported_duplicate_type(&name) {
                    return t;
                }
                Type::TypeReference(name, args)
            }
            TypeNodeKind::Array(inner) => Type::Array(Arc::new(self.resolve_type_node(inner))),
            TypeNodeKind::Tuple(elements) => {
                let types = elements
                    .iter()
                    .map(|e| {
                        let ty = self.resolve_type_node(&e.type_node);
                        if e.dotdotdot {
                            Type::Rest(Arc::new(ty))
                        } else {
                            ty
                        }
                    })
                    .collect();
                Type::Tuple(types)
            }
            TypeNodeKind::Union(types) => {
                let resolved: Vec<_> = types.iter().map(|t| self.resolve_type_node(t)).collect();
                Type::flatten_union(resolved)
            }
            TypeNodeKind::Intersection(types) => {
                let resolved = types.iter().map(|t| self.resolve_type_node(t)).collect();
                Type::Intersection(resolved)
            }
            TypeNodeKind::Function(fn_type) => {
                let type_params: Vec<_> = fn_type
                    .type_params
                    .as_ref()
                    .map(|params| params.iter().map(|param| param.name.clone()).collect())
                    .unwrap_or_default();
                self.enter_type_resolution_type_params(type_params.iter());
                let params = fn_type
                    .params
                    .iter()
                    .map(|p| {
                        let base_name = match &p.name.kind {
                            PatKind::Ident(n) => n.to_string(),
                            _ => "_".to_string(),
                        };
                        // Encode rest/optional in the name so downstream
                        // call-checking unwraps `...args: T[]` → `T` and
                        // skips `?param` when counting required args.
                        let pname = if p.dotdotdot {
                            format!("...{}", base_name)
                        } else if p.optional || p.initializer.is_some() {
                            format!("?{}", base_name)
                        } else {
                            base_name
                        };
                        let pty = p
                            .type_ann
                            .as_ref()
                            .map(|t| self.resolve_type_node(t))
                            .unwrap_or(Type::Any);
                        (pname, self.declared_optional_param_type(p, pty))
                    })
                    .collect();
                let ret = self.resolve_type_node(&fn_type.return_type);
                let fn_type_pred = self.extract_type_predicate(&fn_type.return_type);
                let function = Type::Function(FunctionType {
                    type_param_constraints: self
                        .resolve_type_param_constraints(fn_type.type_params.as_deref()),
                    params,
                    return_type: Arc::new(ret),
                    type_params,
                    type_param_defaults: self
                        .resolve_type_param_defaults(fn_type.type_params.as_deref()),
                    type_predicate: fn_type_pred,
                });
                self.exit_type_resolution_type_params();
                function
            }
            TypeNodeKind::Constructor(fn_type) => {
                let type_params: Vec<_> = fn_type
                    .type_params
                    .as_ref()
                    .map(|params| params.iter().map(|param| param.name.clone()).collect())
                    .unwrap_or_default();
                self.enter_type_resolution_type_params(type_params.iter());
                let params = fn_type
                    .params
                    .iter()
                    .map(|p| {
                        let base_name = match &p.name.kind {
                            PatKind::Ident(n) => n.to_string(),
                            _ => "_".to_string(),
                        };
                        // Encode rest/optional in the name so downstream
                        // call-checking unwraps `...args: T[]` → `T` and
                        // skips `?param` when counting required args.
                        let pname = if p.dotdotdot {
                            format!("...{}", base_name)
                        } else if p.optional || p.initializer.is_some() {
                            format!("?{}", base_name)
                        } else {
                            base_name
                        };
                        let pty = p
                            .type_ann
                            .as_ref()
                            .map(|t| self.resolve_type_node(t))
                            .unwrap_or(Type::Any);
                        (pname, self.declared_optional_param_type(p, pty))
                    })
                    .collect();
                let ret = self.resolve_type_node(&fn_type.return_type);
                let constructor = Type::Constructor(ConstructorType {
                    is_abstract: fn_type.is_abstract,
                    params,
                    return_type: Arc::new(ret),
                    type_params,
                    type_param_constraints: fn_type
                        .type_params
                        .as_ref()
                        .map(|params| {
                            params
                                .iter()
                                .map(|param| {
                                    param
                                        .constraint
                                        .as_ref()
                                        .map(|constraint| self.resolve_type_node(constraint))
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                    type_param_defaults: self
                        .resolve_type_param_defaults(fn_type.type_params.as_deref()),
                });
                self.exit_type_resolution_type_params();
                constructor
            }
            TypeNodeKind::TypeLit(members) => {
                let mut method_names: Vec<std::string::String> = Vec::new();
                let mut props = Vec::new();
                let mut call_sigs = Vec::new();
                let mut construct_sigs = Vec::new();
                let mut index_sig = None;
                let mut index_sig_name: Option<std::string::String> = None;
                for member in members {
                    match &member.kind {
                        TypeMemberKind::PropertySig(prop) => {
                            let name = self.prop_name_to_string(&prop.name);
                            let ty = prop
                                .type_ann
                                .as_ref()
                                .map(|t| self.resolve_type_node(t))
                                .unwrap_or(Type::Any);
                            // Wrap optional properties so structural check can skip them
                            let ty = if prop.optional {
                                Type::Optional(Arc::new(ty))
                            } else {
                                ty
                            };
                            props.push((name, Arc::new(ty)));
                        }
                        TypeMemberKind::MethodSig(method) => {
                            let name = self.prop_name_to_string(&method.name);
                            let type_params: Vec<_> = method
                                .type_params
                                .as_ref()
                                .map(|params| {
                                    params.iter().map(|param| param.name.clone()).collect()
                                })
                                .unwrap_or_default();
                            self.enter_type_resolution_type_params(type_params.iter());
                            let params: Vec<_> = method
                                .params
                                .iter()
                                .map(|p| {
                                    let base_name = match &p.name.kind {
                                        PatKind::Ident(n) => n.to_string(),
                                        _ => "_".to_string(),
                                    };
                                    let pname = if p.dotdotdot {
                                        format!("...{}", base_name)
                                    } else if p.optional || p.initializer.is_some() {
                                        format!("?{}", base_name)
                                    } else {
                                        base_name
                                    };
                                    let pty = p
                                        .type_ann
                                        .as_ref()
                                        .map(|t| self.resolve_type_node(t))
                                        .unwrap_or(Type::Any);
                                    (pname, self.declared_optional_param_type(p, pty))
                                })
                                .collect();
                            let ret = method
                                .return_type
                                .as_ref()
                                .map(|t| self.resolve_type_node(t))
                                .unwrap_or(Type::Any);
                            // Preserve method-level type params (e.g.
                            // `input<NewI>(schema: { _t: NewI }): ...`)
                            // so generic-call inference can find and bind
                            // them. Previously hardcoded to empty —
                            // chained method calls through type aliases
                            // (`type MyProc<C> = { input<NewI>(...): ... }`)
                            // never inferred NewI and returned an
                            // unsubstituted reference.
                            let ft = FunctionType {
                                type_param_constraints: Vec::new(),
                                params: params.clone(),
                                return_type: Arc::new(ret.clone()),
                                type_params,
                                type_param_defaults: Vec::new(),
                                type_predicate: None,
                            };
                            self.exit_type_resolution_type_params();
                            // Wrap optional methods (`m?(): T`) like optional
                            // properties so the structural check treats them as
                            // not-required (a missing optional method is fine).
                            let method_ty = if method.optional {
                                Type::Optional(Arc::new(Type::Function(ft.clone())))
                            } else {
                                Type::Function(ft.clone())
                            };
                            method_names.push(name.clone());
                            props.push((name, Arc::new(method_ty)));
                        }
                        TypeMemberKind::CallSig(cs) => {
                            let type_params: Vec<_> = cs
                                .type_params
                                .as_ref()
                                .map(|params| {
                                    params.iter().map(|param| param.name.clone()).collect()
                                })
                                .unwrap_or_default();
                            self.enter_type_resolution_type_params(type_params.iter());
                            let params: Vec<_> = cs
                                .params
                                .iter()
                                .map(|p| {
                                    let base_name = match &p.name.kind {
                                        PatKind::Ident(n) => n.to_string(),
                                        _ => "_".to_string(),
                                    };
                                    let pname = if p.dotdotdot {
                                        format!("...{}", base_name)
                                    } else if p.optional || p.initializer.is_some() {
                                        format!("?{}", base_name)
                                    } else {
                                        base_name
                                    };
                                    let pty = p
                                        .type_ann
                                        .as_ref()
                                        .map(|t| self.resolve_type_node(t))
                                        .unwrap_or(Type::Any);
                                    (pname, self.declared_optional_param_type(p, pty))
                                })
                                .collect();
                            let ret = cs
                                .return_type
                                .as_ref()
                                .map(|t| self.resolve_type_node(t))
                                .unwrap_or(Type::Any);
                            let function = FunctionType {
                                type_param_constraints: Vec::new(),
                                params,
                                return_type: Arc::new(ret),
                                type_params,
                                type_param_defaults: Vec::new(),
                                type_predicate: None,
                            };
                            self.exit_type_resolution_type_params();
                            call_sigs.push(function);
                        }
                        TypeMemberKind::ConstructSig(cs) => {
                            let type_params: Vec<_> = cs
                                .type_params
                                .as_ref()
                                .map(|params| {
                                    params.iter().map(|param| param.name.clone()).collect()
                                })
                                .unwrap_or_default();
                            self.enter_type_resolution_type_params(type_params.iter());
                            let params: Vec<_> = cs
                                .params
                                .iter()
                                .map(|p| {
                                    let base_name = match &p.name.kind {
                                        PatKind::Ident(n) => n.to_string(),
                                        _ => "_".to_string(),
                                    };
                                    let pname = if p.dotdotdot {
                                        format!("...{}", base_name)
                                    } else if p.optional || p.initializer.is_some() {
                                        format!("?{}", base_name)
                                    } else {
                                        base_name
                                    };
                                    let pty = p
                                        .type_ann
                                        .as_ref()
                                        .map(|t| self.resolve_type_node(t))
                                        .unwrap_or(Type::Any);
                                    (pname, self.declared_optional_param_type(p, pty))
                                })
                                .collect();
                            let ret = cs
                                .return_type
                                .as_ref()
                                .map(|t| self.resolve_type_node(t))
                                .unwrap_or(Type::Any);
                            let type_param_constraints = cs
                                .type_params
                                .as_ref()
                                .map(|params| {
                                    params
                                        .iter()
                                        .map(|param| {
                                            param
                                                .constraint
                                                .as_ref()
                                                .map(|ty| self.resolve_type_node(ty))
                                        })
                                        .collect()
                                })
                                .unwrap_or_default();
                            let constructor = ConstructorType {
                                is_abstract: false,
                                params,
                                return_type: Arc::new(ret),
                                type_params,
                                type_param_constraints,
                                type_param_defaults: self
                                    .resolve_type_param_defaults(cs.type_params.as_deref()),
                            };
                            self.exit_type_resolution_type_params();
                            construct_sigs.push(constructor);
                        }
                        TypeMemberKind::IndexSig(is) => {
                            if let Some(first_param) = is.params.first() {
                                let key_ty = first_param
                                    .type_ann
                                    .as_ref()
                                    .map(|t| self.resolve_type_node(t))
                                    .unwrap_or(Type::String);
                                let val_ty = is
                                    .type_ann
                                    .as_ref()
                                    .map(|t| self.resolve_type_node(t))
                                    .unwrap_or(Type::Any);
                                // One slot: a string index signature covers
                                // every name, so it wins over a number one.
                                let keep_existing = index_sig.as_ref().is_some_and(
                                    |(existing_key, _): &(Arc<Type>, Arc<Type>)| {
                                        matches!(existing_key.as_ref(), Type::String)
                                            && !matches!(key_ty, Type::String)
                                    },
                                );
                                if !keep_existing {
                                    index_sig = Some((Arc::new(key_ty), Arc::new(val_ty)));
                                    if let PatKind::Ident(name) = &first_param.name.kind {
                                        index_sig_name = Some(name.to_string());
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
                let ty = Type::ObjectType(ObjectTypeInfo {
                    properties: props,
                    call_signatures: call_sigs,
                    construct_signatures: construct_sigs,
                    index_signature: index_sig,
                    index_signature_name: index_sig_name,
                    method_names,
                });
                self.record_type_literal_member_locations(&ty, members);
                ty
            }
            TypeNodeKind::Literal(lit) => match lit {
                LiteralTypeKind::Number(n) => Type::NumberLiteral(n.clone()),
                LiteralTypeKind::String(s) => {
                    Type::StringLiteral(self.cooked_property_literal(s, node.span))
                }
                LiteralTypeKind::Boolean(b) => Type::BooleanLiteral(*b),
                LiteralTypeKind::Null => Type::Null,
                LiteralTypeKind::BigInt(n) => {
                    Type::BigIntLiteral(TypeChecker::normalize_bigint_literal(n))
                }
                // `-16n` is a bigint literal type, `-1` a numeric one.
                LiteralTypeKind::Minus(n) if n.ends_with('n') => {
                    let magnitude = TypeChecker::normalize_bigint_literal(n);
                    Type::BigIntLiteral(if magnitude == "0n" {
                        magnitude
                    } else {
                        format!("-{magnitude}")
                    })
                }
                LiteralTypeKind::Minus(n) => Type::NumberLiteral(format!("-{}", n)),
            },
            TypeNodeKind::Conditional(cond) => {
                let raw = Type::Conditional {
                    check: Arc::new(self.resolve_type_node(&cond.check)),
                    extends: Arc::new(self.resolve_type_node(&cond.extends)),
                    true_type: Arc::new(self.resolve_type_node(&cond.true_type)),
                    false_type: Arc::new(self.resolve_type_node(&cond.false_type)),
                };
                // Eagerly collapse when both sides are concrete. When `check`
                // is still a TypeParameter (e.g. inside a generic alias body),
                // the evaluator returns the Conditional unchanged. The alias
                // registration pre-pass turns local-scoped `TypeReference(T,[])`
                // into `TypeParameter(T)` so this case is reliable.
                self.simplify_type(&raw)
            }
            TypeNodeKind::Mapped(mapped) => {
                let constraint = mapped
                    .type_param
                    .constraint
                    .as_ref()
                    .map(|c| self.resolve_type_node(c))
                    .unwrap_or(Type::Any);
                let template = mapped
                    .type_ann
                    .as_ref()
                    .map(|t| Arc::new(self.resolve_type_node(t)));
                // The `as` key-remapper (`[K in keyof T as NameType]`).
                let name_type = mapped
                    .name_type
                    .as_ref()
                    .map(|n| Arc::new(self.resolve_type_node(n)));
                let readonly_mod = mapped.readonly.map(|m| match m {
                    MappedModifier::Add | MappedModifier::None => MappedModifierKind::Add,
                    MappedModifier::Remove => MappedModifierKind::Remove,
                });
                let optional_mod = mapped.optional.map(|m| match m {
                    MappedModifier::Add | MappedModifier::None => MappedModifierKind::Add,
                    MappedModifier::Remove => MappedModifierKind::Remove,
                });
                Type::Mapped {
                    param: mapped.type_param.name.clone(),
                    constraint: Arc::new(constraint),
                    template,
                    name_type,
                    readonly_mod,
                    optional_mod,
                }
            }
            TypeNodeKind::IndexedAccess(obj, idx) => {
                let raw = Type::IndexedAccess(
                    Arc::new(self.resolve_type_node(obj)),
                    Arc::new(self.resolve_type_node(idx)),
                );
                let simplified = self.simplify_type(&raw);
                // Don't let an indexed access that can't be resolved YET collapse
                // to `Error` at declaration-load time. A member like
                // `output: $InferObjectOutput<Shape, Config["out"]>` is loaded
                // while `Config` is still an unbound type parameter, so
                // `Config["out"]` can't resolve — it must stay a DEFERRED
                // IndexedAccess so a later substitution + simplify can resolve it
                // against the concrete type argument. Collapsing to `Error` here
                // permanently poisons the member (e.g. it broke z.infer's
                // `$InferObjectOutput` composition, where the baked `Error`
                // corrupted the `& Config["out"]` intersection).
                if matches!(simplified, Type::Error) {
                    raw
                } else {
                    simplified
                }
            }
            TypeNodeKind::Keyof(inner) => Type::Keyof(Arc::new(self.resolve_type_node(inner))),
            TypeNodeKind::TypeQuery(expr) => {
                let mut name = self.expr_to_name(expr);
                // Mirror the ordinary type-reference rewrite above for
                // `typeof C` inside a named class expression. Keeping the raw
                // source name here would let an outer C capture the stored
                // static self type after the class-expression scope closes.
                if matches!(expr.kind, ExprKind::Ident(_)) {
                    if let Some(Type::TypeReference(value_name, value_args)) =
                        self.lookup_var(&name)
                    {
                        if value_args.is_empty() {
                            if let Some(identity) =
                                value_name.strip_prefix("typeof __class_expression_")
                            {
                                name = format!("__class_expression_{identity}");
                            }
                        }
                    }
                }
                // `typeof undefined` is the `undefined` type itself, which
                // widens to `any` without strictNullChecks.
                if name == "undefined" && matches!(expr.kind, ExprKind::Ident(_)) {
                    return if self.strict_null_checks {
                        Type::Undefined
                    } else {
                        Type::Any
                    };
                }
                // tsc resolves `typeof x` to the variable's type: a concrete
                // one displays and relates as itself (`{ foo: string; }`).
                // Function declarations and namespaces keep `typeof f` /
                // `typeof c` for display (their anonymous object types).
                let function_declaration = self.function_param_spans.contains_key(&name)
                    || self.overloads.contains_key(&name);
                if matches!(expr.kind, ExprKind::Ident(_)) && !self.namespace_paths.contains(&name) {
                    if let Some(resolved) = self.lookup_var(&name).filter(|resolved| {
                        Self::is_concrete_typeof_target(resolved)
                            && !(function_declaration && matches!(resolved, Type::Function(_)))
                    })
                    {
                        return resolved.clone();
                    }
                }
                Type::Typeof(name)
            }
            TypeNodeKind::This => Type::This,
            TypeNodeKind::Paren(inner) => self.resolve_type_node(inner),
            TypeNodeKind::Rest(inner) => Type::Rest(Arc::new(self.resolve_type_node(inner))),
            TypeNodeKind::Optional(inner) => {
                let ty = self.resolve_type_node(inner);
                Type::Union(vec![ty, Type::Undefined].into())
            }
            TypeNodeKind::JSDocNullable(inner) => match inner {
                Some(inner) => {
                    let ty = self.resolve_type_node(inner);
                    Type::Union(vec![ty, Type::Null].into())
                }
                None => Type::Any,
            },
            TypeNodeKind::Unique(inner) => self.resolve_type_node(inner),
            // `readonly T[]` relates like `T[]` here; the readonly wrapper is
            // not modelled through assignability yet.
            TypeNodeKind::Readonly(inner) => self.resolve_type_node(inner),
            TypeNodeKind::Infer(name, _) => Type::Infer(name.clone()),
            TypeNodeKind::TemplateLit(tl) => {
                let quasis: Vec<std::string::String> = tl
                    .quasis
                    .iter()
                    .map(|q| q.cooked.clone().unwrap_or_else(|| q.raw.clone()))
                    .collect();
                let types: Vec<Type> = tl.types.iter().map(|t| self.resolve_type_node(t)).collect();
                Self::evaluate_template_literal_type(quasis, types)
            }
            TypeNodeKind::ImportType(_) => Type::Any,
            // `x is T` returns boolean; an `asserts x` / `asserts x is T`
            // signature returns void (tsc).
            TypeNodeKind::Predicate(pred) => {
                if pred.asserts {
                    Type::Void
                } else {
                    Type::Boolean
                }
            }
            TypeNodeKind::TypeOperator(op, inner) => match op {
                TypeOperatorKind::Keyof => Type::Keyof(Arc::new(self.resolve_type_node(inner))),
                TypeOperatorKind::Unique => self.resolve_type_node(inner),
                TypeOperatorKind::Readonly => self.resolve_type_node(inner),
            },
            TypeNodeKind::NamedTupleMember(member) => self.resolve_type_node(&member.type_node),
        }
    }

    // -----------------------------------------------------------------------
    // Template literal type evaluation
    // -----------------------------------------------------------------------

    pub(crate) fn evaluate_template_literal_type(
        quasis: Vec<std::string::String>,
        types: Vec<Type>,
    ) -> Type {
        // Count against the per-statement instantiation budget so recursive
        // template-conditional types (e.g. `Join`, `SnakeToPascalCase`,
        // `HexColor` distributing 22^6 combinations) cannot expand without
        // bound. On exhaustion widen to `string` rather than exhaust memory.
        if crate::type_op_enter() {
            return Type::String;
        }
        let result = Self::evaluate_template_literal_type_inner(quasis, types);
        crate::type_op_exit();
        result
    }

    fn evaluate_template_literal_type_inner(
        quasis: Vec<std::string::String>,
        types: Vec<Type>,
    ) -> Type {
        if types.is_empty() {
            return Type::StringLiteral(quasis.concat());
        }
        let alternatives: Vec<Vec<Type>> = types
            .into_iter()
            .map(|ty| match ty {
                Type::Union(members) => members.iter().cloned().collect(),
                other => vec![other],
            })
            .collect();
        let all_literal = alternatives.iter().all(|alts| {
            alts.iter()
                .all(|t| Self::type_to_literal_string(t).is_some())
        });
        if all_literal {
            // The literal cross-product grows as the product of the union
            // sizes (`${A}${B}` over unions A,B yields |A|*|B| members).
            // tsc caps such unions at 100,000 members; past that we widen to
            // `string` rather than building millions of combinations and
            // exhausting memory (e.g. conformance templateLiteralTypes1).
            const MAX_TEMPLATE_LITERAL_UNION: u64 = 100_000;
            let within_cap = alternatives
                .iter()
                .try_fold(1u64, |acc, alts| {
                    acc.checked_mul((alts.len().max(1)) as u64)
                })
                .is_some_and(|n| n <= MAX_TEMPLATE_LITERAL_UNION);
            if !within_cap {
                return Type::String;
            }
            let mut results: Vec<std::string::String> = vec![quasis[0].clone()];
            for (i, alts) in alternatives.iter().enumerate() {
                let suffix = &quasis[i + 1];
                let mut new_results = Vec::new();
                for prefix in &results {
                    for alt in alts {
                        let lit = Self::type_to_literal_string(alt).unwrap();
                        new_results.push(format!("{}{}{}", prefix, lit, suffix));
                    }
                }
                results = new_results;
            }
            if results.len() == 1 {
                Type::StringLiteral(results.into_iter().next().unwrap())
            } else {
                let mut deduped: Vec<Type> = Vec::new();
                for r in results {
                    let t = Type::StringLiteral(r);
                    if !deduped.contains(&t) {
                        deduped.push(t);
                    }
                }
                Type::Union(deduped.into())
            }
        } else {
            let flat_types: Vec<Type> = alternatives
                .into_iter()
                .map(|alts| {
                    if alts.len() == 1 {
                        alts.into_iter().next().unwrap()
                    } else {
                        Type::Union(alts.into())
                    }
                })
                .collect();
            Type::TemplateLiteral {
                quasis: quasis.into(),
                types: flat_types.into(),
            }
        }
    }

    pub(crate) fn type_to_literal_string(ty: &Type) -> Option<std::string::String> {
        match ty {
            Type::StringLiteral(s) => Some(s.clone()),
            Type::NumberLiteral(n) => Some(n.clone()),
            Type::BooleanLiteral(b) => Some(b.to_string()),
            Type::BigIntLiteral(n) => Some(n.clone()),
            Type::Null => Some("null".into()),
            Type::Undefined => Some("undefined".into()),
            _ => None,
        }
    }

    pub(crate) fn matches_template_literal(
        literal: &str,
        quasis: &[std::string::String],
        types: &[Type],
    ) -> bool {
        Self::match_template_recursive(literal, quasis, types, 0)
    }

    pub(crate) fn match_template_recursive(
        remaining: &str,
        quasis: &[std::string::String],
        types: &[Type],
        idx: usize,
    ) -> bool {
        if idx >= quasis.len() {
            return remaining.is_empty();
        }
        let quasi = &quasis[idx];
        if !remaining.starts_with(quasi.as_str()) {
            return false;
        }
        let after_quasi = &remaining[quasi.len()..];
        if idx >= types.len() {
            return after_quasi.is_empty();
        }
        let ty = &types[idx];
        match ty {
            t if Self::type_to_literal_string(t).is_some() => {
                let lit = Self::type_to_literal_string(t).unwrap();
                if after_quasi.starts_with(lit.as_str()) {
                    Self::match_template_recursive(
                        &after_quasi[lit.len()..],
                        quasis,
                        types,
                        idx + 1,
                    )
                } else {
                    false
                }
            }
            Type::Union(members) => members.iter().any(|member| {
                let mut single_types = types.to_vec();
                single_types[idx] = member.clone();
                Self::match_template_recursive(after_quasi, quasis, &single_types, idx)
            }),
            Type::String => {
                for end in 0..=after_quasi.len() {
                    if Self::match_template_recursive(&after_quasi[end..], quasis, types, idx + 1) {
                        return true;
                    }
                }
                false
            }
            Type::Number => {
                for end in 0..=after_quasi.len() {
                    let candidate = &after_quasi[..end];
                    if !candidate.is_empty()
                        && candidate.parse::<f64>().is_ok()
                        && Self::match_template_recursive(
                            &after_quasi[end..],
                            quasis,
                            types,
                            idx + 1,
                        )
                    {
                        return true;
                    }
                }
                Self::match_template_recursive(after_quasi, quasis, types, idx + 1)
            }
            Type::Boolean => {
                for lit in &["true", "false"] {
                    if let Some(rest) = after_quasi.strip_prefix(lit) {
                        if Self::match_template_recursive(rest, quasis, types, idx + 1) {
                            return true;
                        }
                    }
                }
                false
            }
            _ => {
                for end in 0..=after_quasi.len() {
                    if Self::match_template_recursive(&after_quasi[end..], quasis, types, idx + 1) {
                        return true;
                    }
                }
                false
            }
        }
    }
    pub(crate) fn expr_to_name(&self, expr: &Expr) -> std::string::String {
        match &expr.kind {
            ExprKind::Ident(name) => name.to_string(),
            ExprKind::Member(mem) => {
                let obj = self.expr_to_name(&mem.object);
                format!("{}.{}", obj, mem.property)
            }
            _ => "<expr>".to_string(),
        }
    }

    pub(crate) fn canonical_numeric_property_name(raw: &str) -> std::string::String {
        let clean = raw.replace('_', "");
        let (negative, magnitude) = if let Some(rest) = clean.strip_prefix('-') {
            (true, rest)
        } else if let Some(rest) = clean.strip_prefix('+') {
            (false, rest)
        } else {
            (false, clean.as_str())
        };
        let magnitude_value = if let Some(digits) = magnitude
            .strip_prefix("0x")
            .or_else(|| magnitude.strip_prefix("0X"))
        {
            u128::from_str_radix(digits, 16)
                .ok()
                .map(|value| value as f64)
                .or_else(|| {
                    num_bigint::BigUint::parse_bytes(digits.as_bytes(), 16)
                        .and_then(|value| value.to_f64())
                })
        } else if let Some(digits) = magnitude
            .strip_prefix("0o")
            .or_else(|| magnitude.strip_prefix("0O"))
        {
            u128::from_str_radix(digits, 8)
                .ok()
                .map(|value| value as f64)
                .or_else(|| {
                    num_bigint::BigUint::parse_bytes(digits.as_bytes(), 8)
                        .and_then(|value| value.to_f64())
                })
        } else if let Some(digits) = magnitude
            .strip_prefix("0b")
            .or_else(|| magnitude.strip_prefix("0B"))
        {
            u128::from_str_radix(digits, 2)
                .ok()
                .map(|value| value as f64)
                .or_else(|| {
                    num_bigint::BigUint::parse_bytes(digits.as_bytes(), 2)
                        .and_then(|value| value.to_f64())
                })
        } else {
            magnitude.parse::<f64>().ok()
        };
        let Some(value) = magnitude_value.map(|value| if negative { -value } else { value }) else {
            return raw.to_string();
        };
        if value.is_infinite() {
            return if value.is_sign_negative() {
                "-Infinity".to_string()
            } else {
                "Infinity".to_string()
            };
        }
        if value == 0.0 {
            return "0".to_string();
        }

        let negative = value.is_sign_negative();
        let value = value.abs();
        let mut buffer = ryu::Buffer::new();
        let shortest = buffer.format_finite(value);
        let (mut digits, n): (String, i32) =
            if let Some((mantissa, exponent)) = shortest.split_once(['e', 'E']) {
                let exponent = exponent.parse::<i32>().unwrap_or(0);
                let decimal_pos = mantissa.find('.').unwrap_or(mantissa.len()) as i32;
                (mantissa.replace('.', ""), decimal_pos + exponent)
            } else if let Some((integer, fraction)) = shortest.split_once('.') {
                if integer != "0" {
                    (format!("{integer}{fraction}"), integer.len() as i32)
                } else {
                    let leading_zeros = fraction.bytes().take_while(|byte| *byte == b'0').count();
                    (
                        fraction[leading_zeros..].to_string(),
                        -(leading_zeros as i32),
                    )
                }
            } else {
                (shortest.to_string(), shortest.len() as i32)
            };
        while digits.len() > 1 && digits.ends_with('0') {
            digits.pop();
        }
        let k = digits.len() as i32;
        let rendered = if k <= n && n <= 21 {
            format!("{}{}", digits, "0".repeat((n - k) as usize))
        } else if 0 < n && n <= 21 {
            let split = n as usize;
            format!("{}.{}", &digits[..split], &digits[split..])
        } else if -6 < n && n <= 0 {
            format!("0.{}{}", "0".repeat((-n) as usize), digits)
        } else {
            let exponent = n - 1;
            let mantissa = if digits.len() == 1 {
                digits
            } else {
                format!("{}.{}", &digits[..1], &digits[1..])
            };
            if exponent >= 0 {
                format!("{mantissa}e+{exponent}")
            } else {
                format!("{mantissa}e{exponent}")
            }
        };
        if negative {
            format!("-{rendered}")
        } else {
            rendered
        }
    }

    pub(crate) fn qualified_property_type(&self, owner: Type, property_name: &str) -> Option<Type> {
        match owner {
            Type::Namespace(info) => info
                .exports
                .iter()
                .find(|(name, _)| name == property_name)
                .map(|(_, ty)| ty.clone()),
            Type::Module(info) => info
                .exports
                .iter()
                .find(|(name, _)| name == property_name)
                .map(|(_, ty)| ty.clone()),
            Type::ObjectType(info) => info
                .properties
                .iter()
                .find(|(name, _)| name == property_name)
                .map(|(_, ty)| ty.as_ref().clone()),
            Type::Intersection(members) => members
                .iter()
                .find_map(|member| self.qualified_property_type(member.clone(), property_name)),
            Type::Tuple(elements) => property_name
                .parse::<usize>()
                .ok()
                .and_then(|index| elements.get(index).cloned()),
            Type::Array(element) if property_name.parse::<usize>().is_ok() => {
                Some(Type::clone(&element))
            }
            Type::TypeReference(name, arguments)
                if matches!(name.as_str(), "Array" | "ReadonlyArray")
                    && arguments.len() == 1
                    && property_name.parse::<usize>().is_ok() =>
            {
                Some(arguments[0].clone())
            }
            Type::TypeReference(class_name, arguments) => {
                if !class_name.starts_with("typeof ")
                    && self.interface_info.contains_key(class_name.as_str())
                {
                    if let Some(Type::ObjectType(info)) =
                        self.resolve_type_reference_to_object(&class_name, &arguments)
                    {
                        if let Some((_, ty)) = info
                            .properties
                            .iter()
                            .find(|(name, _)| name == property_name)
                        {
                            return Some(ty.as_ref().clone());
                        }
                    }
                }
                let mut current = class_name
                    .strip_prefix("typeof ")
                    .unwrap_or(&class_name)
                    .to_string();
                if let Some(members) = self.enum_info.get(current.as_str()) {
                    return members
                        .iter()
                        .find(|(name, _)| name == property_name)
                        .map(|(_, ty)| ty.clone());
                }
                let mut seen = rustc_hash::FxHashSet::default();
                loop {
                    if !seen.insert(current.clone()) {
                        return None;
                    }
                    let info = self.class_info.get(current.as_str())?;
                    if let Some((_, ty)) = info
                        .static_properties
                        .iter()
                        .find(|(name, _)| name == property_name)
                    {
                        return Some(ty.clone());
                    }
                    let Some(base) = &info.extends else {
                        return None;
                    };
                    current = base.clone();
                }
            }
            _ => None,
        }
    }

    fn qualified_value_type(&self, expr: &Expr) -> Option<Type> {
        match &expr.kind {
            ExprKind::Ident(name) => self.lookup_var(name).cloned(),
            ExprKind::Member(member) => {
                let owner = self.qualified_value_type(&member.object)?;
                let property_span = Span::new(
                    expr.span.end.saturating_sub(member.property.len() as u32),
                    expr.span.end,
                );
                let property_name = self.cooked_identifier_name(&member.property, property_span);
                self.qualified_property_type(owner, &property_name)
            }
            ExprKind::ElemAccess(access) => {
                let owner = self.qualified_value_type(&access.object)?;
                let property_name = self.computed_property_name(&access.index)?;
                self.qualified_property_type(owner, &property_name)
            }
            ExprKind::Paren(inner) | ExprKind::NonNull(inner) => self.qualified_value_type(inner),
            ExprKind::Satisfies(satisfies) => self.qualified_value_type(&satisfies.expr),
            ExprKind::As(assertion) if Self::is_const_assertion_expr(expr) => {
                self.qualified_value_type(&assertion.expr)
            }
            ExprKind::TypeAssertion(assertion) if Self::is_const_assertion_expr(expr) => {
                self.qualified_value_type(&assertion.expr)
            }
            _ => None,
        }
    }

    pub(crate) fn qualified_name_type(&self, name: &str) -> Option<Type> {
        let mut parts = name.split('.');
        let first = parts.next()?;
        let mut ty = self.lookup_var(first).cloned()?;
        for property in parts {
            ty = self.qualified_property_type(ty, property)?;
        }
        Some(ty)
    }

    pub(crate) fn computed_property_name(&self, expr: &Expr) -> Option<std::string::String> {
        match &expr.kind {
            ExprKind::Paren(inner) | ExprKind::NonNull(inner) => {
                return self.computed_property_name(inner);
            }
            ExprKind::Satisfies(satisfies) => {
                return self.computed_property_name(&satisfies.expr);
            }
            ExprKind::As(assertion) => {
                if Self::is_const_assertion_expr(expr) {
                    return self.computed_property_name(&assertion.expr);
                }
                return match self.resolve_type_node(&assertion.type_node) {
                    Type::StringLiteral(value) => Some(value),
                    Type::NumberLiteral(value) => {
                        Some(Self::canonical_numeric_property_name(&value))
                    }
                    Type::UniqueSymbol(identity) => {
                        Some(Self::unique_symbol_property_key(&identity))
                    }
                    _ => None,
                };
            }
            ExprKind::TypeAssertion(assertion) => {
                if Self::is_const_assertion_expr(expr) {
                    return self.computed_property_name(&assertion.expr);
                }
                return match self.resolve_type_node(&assertion.type_node) {
                    Type::StringLiteral(value) => Some(value),
                    Type::NumberLiteral(value) => {
                        Some(Self::canonical_numeric_property_name(&value))
                    }
                    Type::UniqueSymbol(identity) => {
                        Some(Self::unique_symbol_property_key(&identity))
                    }
                    _ => None,
                };
            }
            _ => {}
        }
        match &expr.kind {
            ExprKind::Member(member)
                if !self.file_shadows_global_symbol
                    && Self::is_well_known_symbol_name(&member.property)
                    && matches!(&member.object.kind, ExprKind::Ident(name) if name == "Symbol") =>
            {
                Some(format!("[Symbol.{}]", member.property))
            }
            ExprKind::StrLit(value) | ExprKind::NoSubstTemplate(value) => {
                Some(self.cooked_property_literal(value, expr.span))
            }
            ExprKind::NumLit(value) => Some(Self::canonical_numeric_property_name(value)),
            ExprKind::Unary(unary) if matches!(unary.op, UnaryOp::Pos | UnaryOp::Neg) => {
                let ExprKind::NumLit(value) = &unary.argument.kind else {
                    return None;
                };
                let signed = if unary.op == UnaryOp::Neg {
                    format!("-{value}")
                } else {
                    value.to_string()
                };
                Some(Self::canonical_numeric_property_name(&signed))
            }
            ExprKind::Ident(name) => match self.lookup_var(name) {
                Some(Type::StringLiteral(value)) => Some(value.clone()),
                Some(Type::NumberLiteral(value)) => {
                    Some(Self::canonical_numeric_property_name(value))
                }
                Some(Type::UniqueSymbol(identity)) => {
                    Some(Self::unique_symbol_property_key(identity))
                }
                _ => None,
            },
            ExprKind::Member(_) | ExprKind::ElemAccess(_) => {
                match self.qualified_value_type(expr) {
                    Some(Type::StringLiteral(value)) => Some(value),
                    Some(Type::NumberLiteral(value)) => {
                        Some(Self::canonical_numeric_property_name(&value))
                    }
                    Some(Type::UniqueSymbol(identity)) => {
                        Some(Self::unique_symbol_property_key(&identity))
                    }
                    Some(Type::Typeof(target)) => match self.qualified_name_type(&target) {
                        Some(Type::UniqueSymbol(identity)) => {
                            Some(Self::unique_symbol_property_key(&identity))
                        }
                        _ => None,
                    },
                    _ => None,
                }
            }
            _ => None,
        }
    }

    pub(crate) fn unique_symbol_property_key(identity: &str) -> std::string::String {
        if let Some(name) = identity
            .split('\0')
            .next()
            .and_then(|name| name.strip_prefix("SymbolConstructor."))
            .filter(|name| Self::is_well_known_symbol_name(name))
        {
            return format!("[Symbol.{name}]");
        }
        format!("[unique:{identity}]")
    }

    pub(super) fn is_well_known_symbol_name(name: &str) -> bool {
        matches!(
            name,
            "asyncDispose"
                | "asyncIterator"
                | "dispose"
                | "hasInstance"
                | "isConcatSpreadable"
                | "iterator"
                | "match"
                | "matchAll"
                | "metadata"
                | "replace"
                | "search"
                | "species"
                | "split"
                | "toPrimitive"
                | "toStringTag"
                | "unscopables"
        )
    }

    pub(crate) fn unique_symbol_display_name(identity: &str) -> &str {
        identity.split('\0').next().unwrap_or(identity)
    }

    pub(crate) fn cooked_property_literal(&self, fallback: &str, span: Span) -> String {
        let Some(source) = self.current_source.as_deref() else {
            return fallback.to_string();
        };
        let Some(token_text) = source.get(span.start as usize..span.end as usize) else {
            return fallback.to_string();
        };
        let mut scanner = tsc_rs_scanner::TsScanner::new(token_text);
        if matches!(
            scanner.scan(),
            tsc_rs_scanner::TokenKind::StringLiteral
                | tsc_rs_scanner::TokenKind::NoSubstitutionTemplate
        ) {
            scanner.token_value().to_string()
        } else {
            fallback.to_string()
        }
    }

    pub(crate) fn cooked_identifier_name(&self, fallback: &str, span: Span) -> String {
        if !fallback.contains('\\') {
            return fallback.to_string();
        }
        let Some(source) = self.current_source.as_deref() else {
            return fallback.to_string();
        };
        let Some(token_text) = source.get(span.start as usize..span.end as usize) else {
            return fallback.to_string();
        };
        let mut scanner = tsc_rs_scanner::TsScanner::new(token_text);
        scanner.scan();
        if scanner.text_pos() == token_text.len() {
            scanner.token_value().to_string()
        } else {
            fallback.to_string()
        }
    }

    pub(crate) fn prop_name_to_string(&self, name: &PropName) -> std::string::String {
        match name {
            PropName::Ident(s, span) => self.cooked_identifier_name(s, *span),
            PropName::String(s, span) => self.cooked_property_literal(s, *span),
            PropName::Number(s, _) => Self::canonical_numeric_property_name(s),
            PropName::Private(s, _) => format!("#{}", s),
            PropName::Computed(expr, _) => self
                .computed_property_name(expr)
                .unwrap_or_else(|| "[computed]".to_string()),
        }
    }

    // -----------------------------------------------------------------------
    // Function type resolution
    // -----------------------------------------------------------------------

    /// Declared constraints of a signature's type parameters, aligned with
    /// its `type_params` (`None` for an unconstrained one).
    pub(crate) fn resolve_type_param_constraints(
        &self,
        type_params: Option<&[TypeParam]>,
    ) -> Vec<Option<Type>> {
        let Some(type_params) = type_params else {
            return Vec::new();
        };
        type_params
            .iter()
            .map(|tp| {
                tp.constraint
                    .as_ref()
                    .map(|constraint| self.resolve_type_node(constraint))
            })
            .collect()
    }

    pub(crate) fn resolve_type_param_defaults(
        &self,
        type_params: Option<&[TypeParam]>,
    ) -> Vec<Option<Type>> {
        let Some(type_params) = type_params else {
            return Vec::new();
        };
        let names: std::collections::HashSet<_> =
            type_params.iter().map(|tp| tp.name.as_str()).collect();
        type_params
            .iter()
            .map(|tp| {
                tp.default.as_ref().map(|default| {
                    let resolved = self.resolve_type_node(default);
                    Self::rewrite_type_param_refs(&resolved, &names)
                })
            })
            .collect()
    }

    pub(crate) fn resolve_fn_type(&self, fn_decl: &FnDecl) -> FunctionType {
        let params = fn_decl
            .params
            .iter()
            .map(|p| {
                let base_name = match &p.name.kind {
                    PatKind::Ident(n) => n.to_string(),
                    _ => "_".to_string(),
                };
                // Encode optional/rest in the name for display_string:
                //   "?name" = optional param, "...name" = rest param
                let pname = if p.dotdotdot {
                    format!("...{}", base_name)
                } else if p.optional || p.initializer.is_some() {
                    format!("?{}", base_name)
                } else {
                    base_name
                };
                let pty = p
                    .type_ann
                    .as_ref()
                    .map(|t| self.resolve_type_node(t))
                    .unwrap_or_else(|| {
                        // Infer type from initializer, widening literals
                        p.initializer
                            .as_ref()
                            .map(|init| widen_literal(self.infer_expr_type(init)))
                            .unwrap_or(Type::Any)
                    });
                (pname, self.declared_optional_param_type(p, pty))
            })
            .collect();
        let ret = if let Some(ref rt) = fn_decl.return_type {
            self.resolve_type_node(rt)
        } else if let Some(ref body) = fn_decl.body {
            let inferred = self.infer_return_type_from_stmts(body);
            // An async function ALWAYS returns a Promise. When the return type
            // is inferred (no annotation), wrap the body-completion type in
            // `Promise<…>` — otherwise a bare `return;` / fall-through leaves
            // the call result typed `undefined`/`void`, so `f().catch(…)` and
            // `await f()` are wrongly seen as possibly-undefined.
            if fn_decl.is_generator {
                // Generator bodies complete with `Generator`/`AsyncGenerator`,
                // never the bare completion type (an async generator is NOT a
                // `Promise`).
                let owner = if fn_decl.is_async {
                    "AsyncGenerator"
                } else {
                    "Generator"
                };
                Type::TypeReference(owner.into(), vec![Type::Any, Type::Any, Type::Any].into())
            } else if fn_decl.is_async {
                Self::wrap_async_inferred_return(inferred)
            } else {
                inferred
            }
        } else {
            Type::Any
        };
        let type_predicate = fn_decl
            .return_type
            .as_ref()
            .and_then(|rt| self.extract_type_predicate(rt));
        let type_params = fn_decl
            .type_params
            .as_ref()
            .map(|tps| tps.iter().map(|tp| tp.name.clone()).collect())
            .unwrap_or_default();
        let type_param_defaults = self.resolve_type_param_defaults(fn_decl.type_params.as_deref());
        let type_param_constraints =
            self.resolve_type_param_constraints(fn_decl.type_params.as_deref());
        FunctionType {
            type_param_constraints,
            params,
            return_type: Arc::new(ret),
            type_params,
            type_param_defaults,
            type_predicate,
        }
    }

    /// Wrap an async function's INFERRED body-completion type in `Promise<…>`.
    /// If the body already completes with a `Promise<T>` (e.g. `return
    /// somePromise`), async flattening keeps it a single `Promise<T>` rather
    /// than double-wrapping.
    pub(crate) fn wrap_async_inferred_return(inferred: Type) -> Type {
        let already_promise = matches!(
            &inferred,
            Type::TypeReference(name, _)
                if name.rsplit('.').next() == Some("Promise")
        );
        if already_promise {
            inferred
        } else {
            Type::TypeReference("Promise".into(), vec![inferred].into())
        }
    }

    /// Extract a type predicate from a return type node (if it's a predicate type).
    pub(crate) fn extract_type_predicate(&self, type_node: &TypeNode) -> Option<TypePredicate> {
        if let TypeNodeKind::Predicate(pred) = &type_node.kind {
            let target_type = pred
                .type_ann
                .as_ref()
                .map(|t| self.resolve_type_node(t))
                .unwrap_or(Type::Any);
            Some(TypePredicate {
                param_name: pred.param_name.clone(),
                target_type: Arc::new(target_type),
                is_asserts: pred.asserts,
            })
        } else {
            None
        }
    }

    /// Resolve an `OverloadSignature` (from the symbol table) into a `FunctionType`.
    pub(crate) fn resolve_overload_signature(
        &self,
        sig: &tsc_rs_symbols::OverloadSignature,
    ) -> FunctionType {
        let params = sig
            .params
            .iter()
            .map(|p| {
                let pty = p
                    .type_ann
                    .as_ref()
                    .map(|t| self.resolve_type_node(t))
                    .unwrap_or(Type::Any);
                let pname = if p.rest {
                    format!("...{}", p.name)
                } else if p.optional {
                    format!("?{}", p.name)
                } else {
                    p.name.clone()
                };
                (pname, pty)
            })
            .collect();
        let ret = sig
            .return_type
            .as_ref()
            .map(|t| self.resolve_type_node(t))
            .unwrap_or(Type::Any);
        let names: std::collections::HashSet<_> =
            sig.type_params.iter().map(String::as_str).collect();
        let type_param_defaults = sig
            .type_param_defaults
            .iter()
            .map(|default| {
                default.as_ref().map(|node| {
                    let resolved = self.resolve_type_node(node);
                    Self::rewrite_type_param_refs(&resolved, &names)
                })
            })
            .collect();
        FunctionType {
            type_param_constraints: Vec::new(),
            params,
            return_type: Arc::new(ret),
            type_params: sig.type_params.clone(),
            type_param_defaults,
            type_predicate: None,
        }
    }

    /// Infer the return type of a function from its return statements.
    pub(crate) fn infer_return_type_from_stmts(&self, stmts: &[Stmt]) -> Type {
        let mut return_types = Vec::new();
        self.collect_return_types(stmts, &mut return_types);
        // No returns, or only bare `return;` statements: void.
        if return_types.is_empty() || return_types.iter().all(|t| matches!(t, Type::Undefined)) {
            Type::Void
        } else {
            let mut widened: Vec<_> = return_types
                .into_iter()
                .map(|t| self.widen_type(&t))
                .collect();
            // Without strictNullChecks null/undefined are not part of the
            // inferred type; a function returning only those returns `any`.
            if !self.strict_null_checks {
                let all_nullish = widened
                    .iter()
                    .all(|t| matches!(t, Type::Null | Type::Undefined));
                if all_nullish {
                    return Type::Any;
                }
                widened.retain(|t| !matches!(t, Type::Null | Type::Undefined));
            }
            Type::flatten_union(widened)
        }
    }

    /// Walk statements to collect return expression types.
    pub(crate) fn collect_return_types(&self, stmts: &[Stmt], out: &mut Vec<Type>) {
        for stmt in stmts {
            self.collect_return_types_from_stmt(stmt, out);
        }
    }

    pub(crate) fn collect_return_types_from_stmt(&self, stmt: &Stmt, out: &mut Vec<Type>) {
        match &stmt.kind {
            StmtKind::Return(Some(ref expr)) => {
                let ty = self.infer_expr_type(expr);
                out.push(self.widen_fresh_return_expr_type(expr, ty));
            }
            StmtKind::Return(None) => {
                out.push(Type::Undefined);
            }
            StmtKind::Block(stmts) => {
                self.collect_return_types(stmts, out);
            }
            StmtKind::If(if_stmt) => {
                self.collect_return_types_from_stmt(&if_stmt.consequent, out);
                if let Some(ref alt) = if_stmt.alternate {
                    self.collect_return_types_from_stmt(alt, out);
                }
            }
            // Try / Switch / loop bodies can `return` too. Without these
            // arms `infer_return_type_from_block` flagged an IIFE body
            // like `try { return x } finally { ... }` as returning `void`,
            // tripping TS2345 wherever the IIFE result was passed to a
            // typed parameter (~4 sites in apps/app's shared-context.ts
            // and similar).
            StmtKind::Try(try_stmt) => {
                self.collect_return_types(&try_stmt.block, out);
                if let Some(ref handler) = try_stmt.handler {
                    self.collect_return_types(&handler.body, out);
                }
                if let Some(ref finalizer) = try_stmt.finalizer {
                    self.collect_return_types(finalizer, out);
                }
            }
            StmtKind::Switch(switch_stmt) => {
                for case in &switch_stmt.cases {
                    self.collect_return_types(&case.consequent, out);
                }
            }
            StmtKind::While(w) => {
                self.collect_return_types_from_stmt(&w.body, out);
            }
            StmtKind::DoWhile(dw) => {
                self.collect_return_types_from_stmt(&dw.body, out);
            }
            StmtKind::For(f) => {
                self.collect_return_types_from_stmt(&f.body, out);
            }
            StmtKind::ForIn(f) => {
                self.collect_return_types_from_stmt(&f.body, out);
            }
            StmtKind::ForOf(f) => {
                self.collect_return_types_from_stmt(&f.body, out);
            }
            _ => {}
        }
    }

    /// Infer the return type from a function/arrow body block that has
    /// already been fully checked (statements walked). Looks at return
    /// statements to build a union of their expression types.
    pub(crate) fn infer_return_type_from_block(&self, stmts: &[Stmt]) -> Type {
        let mut return_types = Vec::new();
        self.collect_return_types(stmts, &mut return_types);
        // tsc (mayReturnNever): a function expression, arrow or object
        // literal method with no return statements whose end point is
        // unreachable (`{ throw ... }`) returns `never`.
        if return_types.is_empty() && stmts.iter().any(|stmt| Self::is_definite_terminator(stmt)) {
            return Type::Never;
        }
        // No returns, or only bare `return;` statements: void.
        if return_types.is_empty() || return_types.iter().all(|t| matches!(t, Type::Undefined)) {
            Type::Void
        } else {
            let mut widened: Vec<_> = return_types
                .into_iter()
                .map(|t| self.widen_type(&t))
                .collect();
            // Without strictNullChecks null/undefined are not part of the
            // inferred type; a function returning only those returns `any`.
            if !self.strict_null_checks {
                let all_nullish = widened
                    .iter()
                    .all(|t| matches!(t, Type::Null | Type::Undefined));
                if all_nullish {
                    return Type::Any;
                }
                widened.retain(|t| !matches!(t, Type::Null | Type::Undefined));
            }
            Type::flatten_union(widened)
        }
    }

    // -----------------------------------------------------------------------
    // Expression type inference (without full checking)
    // -----------------------------------------------------------------------

    pub(crate) fn infer_const_asserted_expr(&self, expr: &Expr) -> Type {
        match &expr.kind {
            ExprKind::Paren(inner) => self.infer_const_asserted_expr(inner),
            ExprKind::ArrayLit(elements) => Type::Tuple(
                elements
                    .iter()
                    .map(|element| {
                        element
                            .as_ref()
                            .map(|element| self.infer_const_asserted_expr(element))
                            .unwrap_or(Type::Undefined)
                    })
                    .collect(),
            ),
            ExprKind::ObjectLit(properties) => {
                let Type::ObjectType(mut info) = self.infer_expr_type(expr) else {
                    return Self::freeze_as_const(&self.infer_expr_type(expr));
                };
                for property in properties {
                    let ObjLitProp::Property(property) = property else {
                        continue;
                    };
                    let name = self.prop_name_to_string(&property.key);
                    if let Some((_, ty)) = info
                        .properties
                        .iter_mut()
                        .rev()
                        .find(|(property_name, _)| property_name == &name)
                    {
                        *ty = Arc::new(self.infer_const_asserted_expr(&property.value));
                    }
                }
                Type::ObjectType(info)
            }
            ExprKind::As(assertion) if Self::is_const_assertion_expr(expr) => {
                self.infer_const_asserted_expr(&assertion.expr)
            }
            ExprKind::TypeAssertion(assertion) if Self::is_const_assertion_expr(expr) => {
                self.infer_const_asserted_expr(&assertion.expr)
            }
            _ => Self::freeze_as_const(&self.infer_expr_type(expr)),
        }
    }

    pub(crate) fn infer_expr_type(&self, expr: &Expr) -> Type {
        match &expr.kind {
            ExprKind::NumLit(n) => Type::NumberLiteral(n.to_string()),
            ExprKind::BigIntLit(n) => Type::BigIntLiteral(n.to_string()),
            ExprKind::StrLit(s) => Type::StringLiteral(self.cooked_property_literal(s, expr.span)),
            ExprKind::BoolLit(b) => Type::BooleanLiteral(*b),
            ExprKind::NullLit => Type::Null,
            ExprKind::NoSubstTemplate(s) => {
                Type::StringLiteral(self.cooked_property_literal(s, expr.span))
            }
            ExprKind::Template(_) => Type::String,
            ExprKind::ArrayLit(elems) => {
                let mut elem_types = Vec::new();
                for elem in elems.iter().flatten() {
                    let inferred = self.infer_expr_type(elem);
                    let preserve_narrow =
                        Self::is_const_assertion_expr(elem) || !Self::is_fresh_literal_expr(elem);
                    let ty = if preserve_narrow {
                        inferred
                    } else {
                        self.widen_array_elem(&inferred)
                    };
                    if !elem_types.iter().any(|t: &Type| t == &ty) {
                        elem_types.push(ty);
                    }
                }
                // tsc orders nullish members last in inferred element unions.
                elem_types.sort_by_key(|t| matches!(t, Type::Null | Type::Undefined));
                let elem_ty = if elem_types.is_empty() {
                    Type::Any
                } else if elem_types.len() == 1 {
                    elem_types.into_iter().next().unwrap()
                } else if let Some(bct) = self.best_common_type(&elem_types) {
                    bct
                } else {
                    Type::Union(elem_types.into())
                };
                Type::Array(Arc::new(elem_ty))
            }
            ExprKind::ObjectLit(props) => {
                let mut method_names: Vec<std::string::String> = Vec::new();
                let mut properties = Vec::new();
                for prop in props {
                    match prop {
                        ObjLitProp::Property(p) => {
                            let key = self.prop_name_to_string(&p.key);
                            let val_ty = self.infer_expr_type(&p.value);
                            properties.push((key, Arc::new(val_ty)));
                        }
                        ObjLitProp::Shorthand(name, _) => {
                            let ty = self.lookup_var(name).cloned().unwrap_or(Type::Any);
                            properties.push((name.to_string(), Arc::new(ty)));
                        }
                        ObjLitProp::Spread(e, _) => {
                            // Mirror the check-pass behavior so that
                            // infer_return_type / inject paths don't lose
                            // spread properties from `{ ...state, x: 1 }`.
                            // Without this, reducer-style returns inferred
                            // an object type missing every property
                            // contributed by `state`, tripping downstream
                            // TS2322 / TS2339 against the declared shape.
                            let spread_ty = self.infer_expr_type(e);
                            let _ = self
                                .merge_spread_into_object_properties(&spread_ty, &mut properties);
                        }
                        ObjLitProp::Method(m) => {
                            let name = self.prop_name_to_string(&m.name);
                            method_names.push(name.clone());
                            let params: Vec<_> = m
                                .params
                                .iter()
                                .map(|p| {
                                    let base_name = match &p.name.kind {
                                        PatKind::Ident(n) => n.to_string(),
                                        _ => "_".to_string(),
                                    };
                                    let pname = if p.dotdotdot {
                                        format!("...{}", base_name)
                                    } else if p.optional || p.initializer.is_some() {
                                        format!("?{}", base_name)
                                    } else {
                                        base_name
                                    };
                                    let pty = p
                                        .type_ann
                                        .as_ref()
                                        .map(|t| self.resolve_type_node(t))
                                        .unwrap_or(Type::Any);
                                    (pname, self.declared_optional_param_type(p, pty))
                                })
                                .collect();
                            let ret = m
                                .return_type
                                .as_ref()
                                .map(|t| self.resolve_type_node(t))
                                .unwrap_or_else(|| self.infer_return_type_from_stmts(&m.body));
                            properties.push((
                                name,
                                Arc::new(Type::Function(FunctionType {
                                    type_param_constraints: Vec::new(),
                                    params,
                                    return_type: Arc::new(ret),
                                    type_params: Vec::new(),
                                    type_param_defaults: Vec::new(),
                                    type_predicate: None,
                                })),
                            ));
                        }
                        ObjLitProp::Get(acc) => {
                            let name = self.prop_name_to_string(&acc.name);
                            let ty = acc
                                .return_type
                                .as_ref()
                                .map(|t| self.resolve_type_node(t))
                                .unwrap_or_else(|| self.infer_return_type_from_stmts(&acc.body));
                            properties.push((name, Arc::new(ty)));
                        }
                        _ => {}
                    }
                }
                Type::ObjectType(ObjectTypeInfo {
                    properties,
                    call_signatures: Vec::new(),
                    construct_signatures: Vec::new(),
                    index_signature: None,
                    index_signature_name: None,
                    method_names,
                })
            }
            ExprKind::Ident(name) => self.lookup_var(name).cloned().unwrap_or(Type::Any),
            ExprKind::FnExpr(fn_decl) => Type::Function(self.resolve_fn_type(fn_decl)),
            ExprKind::Arrow(arrow) => {
                let params = arrow
                    .params
                    .iter()
                    .map(|p| {
                        let base_name = match &p.name.kind {
                            PatKind::Ident(n) => n.to_string(),
                            _ => "_".to_string(),
                        };
                        let pname = if p.dotdotdot {
                            format!("...{}", base_name)
                        } else if p.optional || p.initializer.is_some() {
                            format!("?{}", base_name)
                        } else {
                            base_name
                        };
                        let pty = p
                            .type_ann
                            .as_ref()
                            .map(|t| self.resolve_type_node(t))
                            .unwrap_or_else(|| {
                                p.initializer
                                    .as_ref()
                                    .map(|init| widen_literal(self.infer_expr_type(init)))
                                    .unwrap_or(Type::Any)
                            });
                        (pname, pty)
                    })
                    .collect();
                let ret = if let Some(ref rt) = arrow.return_type {
                    self.resolve_type_node(rt)
                } else {
                    let inferred = match &arrow.body {
                        tsc_rs_ast::ArrowBody::Block(stmts) => {
                            self.infer_return_type_from_stmts(stmts)
                        }
                        tsc_rs_ast::ArrowBody::Expr(expr) => self.infer_expr_type(expr),
                    };
                    // An async arrow always returns a Promise — same rule as
                    // the function-declaration path. Without the wrap,
                    // `async () => { await x; }` typed as `() => void` and
                    // failed assignment to `() => Promise<void>`.
                    if arrow.is_async {
                        Self::wrap_async_inferred_return(inferred)
                    } else {
                        inferred
                    }
                };
                let type_params = arrow
                    .type_params
                    .as_ref()
                    .map(|tps| tps.iter().map(|tp| tp.name.clone()).collect())
                    .unwrap_or_default();
                Type::Function(FunctionType {
                    type_param_constraints: Vec::new(),
                    params,
                    return_type: Arc::new(ret),
                    type_params,
                    type_param_defaults: Vec::new(),
                    type_predicate: None,
                })
            }
            ExprKind::Paren(inner) => self.infer_expr_type(inner),
            ExprKind::As(a) => {
                // Detect `as const`: parser encodes it as Reference("const")
                let is_const_assertion = matches!(&a.type_node.kind,
                    TypeNodeKind::Reference(type_ref) if matches!(&type_ref.name.kind,
                        ExprKind::Ident(n) if n == "const"
                    )
                );
                if is_const_assertion {
                    self.infer_const_asserted_expr(&a.expr)
                } else {
                    self.resolve_type_node(&a.type_node)
                }
            }
            ExprKind::TypeAssertion(ta) => {
                if Self::is_const_assertion_expr(expr) {
                    self.infer_const_asserted_expr(&ta.expr)
                } else {
                    self.resolve_type_node(&ta.type_node)
                }
            }
            ExprKind::Cond(cond) => {
                let t = self.infer_expr_type(&cond.consequent);
                let f = self.infer_expr_type(&cond.alternate);
                Self::reduce_empty_array_union(Type::flatten_union(vec![t, f]))
            }
            ExprKind::Binary(bin) => match bin.op {
                BinaryOp::Add => {
                    let left = self.infer_expr_type(&bin.left);
                    let right = self.infer_expr_type(&bin.right);
                    if matches!(
                        self.widen_type(&left),
                        Type::String | Type::StringLiteral(_)
                    ) || matches!(
                        self.widen_type(&right),
                        Type::String | Type::StringLiteral(_)
                    ) {
                        Type::String
                    } else {
                        Type::Number
                    }
                }
                BinaryOp::Sub
                | BinaryOp::Mul
                | BinaryOp::Div
                | BinaryOp::Mod
                | BinaryOp::Exp
                | BinaryOp::BitAnd
                | BinaryOp::BitOr
                | BinaryOp::BitXor
                | BinaryOp::Shl
                | BinaryOp::Shr
                | BinaryOp::UShr => Type::Number,
                BinaryOp::Eq
                | BinaryOp::Ne
                | BinaryOp::StrictEq
                | BinaryOp::StrictNe
                | BinaryOp::Lt
                | BinaryOp::Le
                | BinaryOp::Gt
                | BinaryOp::Ge
                | BinaryOp::InstanceOf
                | BinaryOp::In => Type::Boolean,
                BinaryOp::LogAnd => self.infer_expr_type(&bin.right),
                BinaryOp::LogOr | BinaryOp::NullCoal => {
                    let left = self.infer_expr_type(&bin.left);
                    let right = self.infer_expr_type(&bin.right);
                    Type::flatten_union(vec![left, right])
                }
            },
            ExprKind::Unary(un) => match (&un.op, &un.argument.kind) {
                (UnaryOp::Pos, ExprKind::NumLit(value)) => {
                    Type::NumberLiteral(TypeChecker::normalize_numeric_name(value))
                }
                (UnaryOp::Neg, ExprKind::NumLit(value)) => {
                    let magnitude = TypeChecker::normalize_numeric_name(value);
                    Type::NumberLiteral(match magnitude.strip_prefix('-') {
                        Some(rest) => rest.to_string(),
                        None => format!("-{magnitude}"),
                    })
                }
                (UnaryOp::Neg, ExprKind::BigIntLit(value)) => {
                    let magnitude = TypeChecker::normalize_bigint_literal(value);
                    Type::BigIntLiteral(if magnitude == "0n" {
                        magnitude
                    } else {
                        format!("-{magnitude}")
                    })
                }
                (UnaryOp::Neg | UnaryOp::Pos | UnaryOp::BitNot, _) => Type::Number,
                (UnaryOp::LogNot, _) => Type::Boolean,
                (UnaryOp::Typeof, _) => TypeChecker::typeof_result_type(),
                (UnaryOp::Void, _) => Type::Undefined,
                (UnaryOp::Delete, _) => Type::Boolean,
            },
            ExprKind::Update(update) => {
                let operand = self.infer_expr_type(&update.argument);
                if matches!(operand, Type::BigInt | Type::BigIntLiteral(_)) {
                    Type::BigInt
                } else {
                    Type::Number
                }
            }
            ExprKind::Typeof(_) => TypeChecker::typeof_result_type(),
            ExprKind::Void(_) => Type::Undefined,
            ExprKind::Delete(_) => Type::Boolean,
            ExprKind::Comma(exprs) => exprs
                .last()
                .map(|e| self.infer_expr_type(e))
                .unwrap_or(Type::Undefined),
            ExprKind::RegexpLit(_) => {
                Type::TypeReference("RegExp".to_string(), Arc::from([] as [Type; 0]))
            }
            ExprKind::Await(inner) => {
                let ty = self.infer_expr_type(inner);
                if let Type::TypeReference(ref name, ref args) = ty {
                    if name == "Promise" && args.len() == 1 {
                        return args[0].clone();
                    }
                }
                ty
            }
            ExprKind::Call(call) => {
                // For named function calls, look up the function variable
                if let ExprKind::Ident(name) = &call.callee.kind {
                    if let Some(ty) = self.lookup_var(name) {
                        if let Type::Function(ft) = ty {
                            // Substitute generic type params if present
                            if !ft.type_params.is_empty() {
                                let type_param_map = if let Some(ref ta) = call.type_args {
                                    let mut map = std::collections::HashMap::new();
                                    for (tp_name, type_arg_node) in
                                        ft.type_params.iter().zip(ta.iter())
                                    {
                                        map.insert(
                                            tp_name.clone(),
                                            self.resolve_type_node(type_arg_node),
                                        );
                                    }
                                    map
                                } else {
                                    // Widen literal types before generic inference
                                    // (TypeScript infers `number` from `1`, not literal `1`)
                                    let arg_types: Vec<Type> = call
                                        .args
                                        .iter()
                                        .map(|a| {
                                            Self::widen_nested_literals(&self.infer_expr_type(a))
                                        })
                                        .collect();
                                    Self::infer_type_arguments(
                                        &arg_types,
                                        &ft.params,
                                        &ft.type_params,
                                    )
                                };
                                return Self::substitute(&ft.return_type, &type_param_map);
                            }
                            return Type::clone(&ft.return_type);
                        }
                    }
                }

                // Special-case: Array(), Boolean(), String(), Number() as function calls
                if let ExprKind::Ident(name) = &call.callee.kind {
                    match name.as_str() {
                        "Array" => {
                            // Array<T>() or Array(items...) — infer element type
                            if let Some(ref ta) = call.type_args {
                                if let Some(first) = ta.first() {
                                    return Type::Array(Arc::new(self.resolve_type_node(first)));
                                }
                            }
                            // Infer from args: Array("s") → string[]
                            if !call.args.is_empty() {
                                // If single numeric arg, it's array length → any[]
                                if call.args.len() == 1 {
                                    let arg_ty = self.infer_expr_type(&call.args[0]);
                                    if matches!(arg_ty, Type::Number | Type::NumberLiteral(_)) {
                                        return Type::Array(Arc::new(Type::Any));
                                    }
                                }
                                let elem_types: Vec<Type> = call
                                    .args
                                    .iter()
                                    .map(|a| Self::widen_nested_literals(&self.infer_expr_type(a)))
                                    .collect();
                                let elem = if elem_types.len() == 1 {
                                    elem_types.into_iter().next().unwrap()
                                } else {
                                    Type::flatten_union(elem_types)
                                };
                                return Type::Array(Arc::new(elem));
                            }
                            return Type::Array(Arc::new(Type::Any));
                        }
                        "Boolean" => return Type::Boolean,
                        "String" if !call.args.is_empty() => return Type::String,
                        "Number" if !call.args.is_empty() => return Type::Number,
                        "Symbol" => return Type::Symbol,
                        _ => {}
                    }
                }

                // Special-case: static method calls (Array.from, Object.keys, etc.)
                if let ExprKind::Member(mem) = &call.callee.kind {
                    if let ExprKind::Ident(obj_name) = &mem.object.kind {
                        match (obj_name.as_str(), mem.property.as_str()) {
                            ("Array", "from") | ("Array", "of") => {
                                // Array.from(iterable) — infer element type from first arg
                                if let Some(arg) = call.args.first() {
                                    let arg_ty = self.infer_expr_type(arg);
                                    if let Type::Array(elem) = arg_ty {
                                        return Type::Array(elem);
                                    }
                                }
                                return Type::Array(Arc::new(Type::Any));
                            }
                            ("Object", "keys") => return Type::Array(Arc::new(Type::String)),
                            ("Object", "values") => return Type::Array(Arc::new(Type::Any)),
                            ("Symbol", "for") => return Type::Symbol,
                            ("Object", "entries") => {
                                return Type::Array(Arc::new(Type::Tuple(
                                    vec![Type::String, Type::Any].into(),
                                )));
                            }
                            _ => {}
                        }
                    }
                }

                // Special-case: Array method calls that preserve/transform element types
                if let ExprKind::Member(mem) = &call.callee.kind {
                    let obj_ty = self.infer_expr_type(&mem.object);
                    if let Type::Array(ref elem) = obj_ty {
                        match mem.property.as_str() {
                            "map" | "flatMap" => {
                                // map(fn): infer callback return type from first arg
                                if let Some(cb) = call.args.first() {
                                    let cb_ret = self.infer_callback_return_type(cb, elem);
                                    return Type::Array(Arc::new(cb_ret));
                                }
                                return Type::Array(Arc::new(Type::Any));
                            }
                            "filter" => {
                                // A type-guard predicate (`x is S`) narrows the
                                // result to `S[]`; otherwise the array type is
                                // unchanged.
                                if let Some(cb) = call.args.first() {
                                    if let Some(narrowed) = self.callback_type_guard_target(cb) {
                                        return Type::Array(Arc::new(narrowed));
                                    }
                                }
                                return obj_ty.clone();
                            }
                            "slice" | "concat" | "reverse" | "sort" | "flat" | "splice" => {
                                return obj_ty.clone();
                            }
                            "find" => {
                                // A type-guard predicate (`x is S`) narrows the
                                // element to `S | undefined`.
                                if let Some(cb) = call.args.first() {
                                    if let Some(narrowed) = self.callback_type_guard_target(cb) {
                                        return Type::flatten_union(vec![
                                            narrowed,
                                            Type::Undefined,
                                        ]);
                                    }
                                }
                                return Type::flatten_union(vec![
                                    Type::clone(&elem),
                                    Type::Undefined,
                                ]);
                            }
                            "reduce" => {
                                // reduce(fn, init): return type = init type or accumulator
                                if call.args.len() >= 2 {
                                    return self.infer_expr_type(&call.args[1]);
                                }
                                return Type::clone(&elem);
                            }
                            "pop" | "shift" => {
                                return Type::flatten_union(vec![
                                    Type::clone(&elem),
                                    Type::Undefined,
                                ]);
                            }
                            "push" | "unshift" => return Type::Number,
                            "indexOf" | "lastIndexOf" | "findIndex" => return Type::Number,
                            "includes" | "every" | "some" => return Type::Boolean,
                            "join" => return Type::String,
                            "forEach" => return Type::Void,
                            _ => {}
                        }
                    }
                }

                // For member calls (obj.method()), resolve the method type
                let callee_ty = if let ExprKind::Member(ref mem) = call.callee.kind {
                    let obj_ty = self.infer_expr_type(&mem.object);
                    self.resolve_member_on_type(&obj_ty, &mem.property)
                } else {
                    self.infer_expr_type(&call.callee)
                };
                match callee_ty {
                    Type::Function(ft) => {
                        // Substitute generic type params if present
                        if !ft.type_params.is_empty() {
                            let type_param_map = if let Some(ref ta) = call.type_args {
                                let mut map = std::collections::HashMap::new();
                                for (tp_name, type_arg_node) in ft.type_params.iter().zip(ta.iter())
                                {
                                    map.insert(
                                        tp_name.clone(),
                                        self.resolve_type_node(type_arg_node),
                                    );
                                }
                                map
                            } else {
                                let arg_types: Vec<Type> = call
                                    .args
                                    .iter()
                                    .map(|a| Self::widen_nested_literals(&self.infer_expr_type(a)))
                                    .collect();
                                Self::infer_type_arguments(&arg_types, &ft.params, &ft.type_params)
                            };
                            Self::substitute(&ft.return_type, &type_param_map)
                        } else {
                            Type::clone(&ft.return_type)
                        }
                    }
                    // Intersection-of-Functions encodes an overloaded
                    // callable (see TypeChecker::check_call_against_fn_type
                    // for the full machinery). Here we run a lightweight
                    // overload pick: try each constituent Function in
                    // order and return the first that arity-matches the
                    // call. Per-arg type checking is skipped at this
                    // pre-check layer; the heavyweight check refines
                    // later.
                    Type::Intersection(members) => {
                        let arg_types: Vec<Type> = call
                            .args
                            .iter()
                            .map(|a| Self::widen_nested_literals(&self.infer_expr_type(a)))
                            .collect();
                        for member in members.iter() {
                            let Type::Function(ft) = member else {
                                continue;
                            };
                            let min_params = ft
                                .params
                                .iter()
                                .filter(|(n, _)| !n.starts_with('?') && !n.starts_with("..."))
                                .count();
                            let has_rest = ft.params.iter().any(|(n, _)| n.starts_with("..."));
                            let max_params = if has_rest {
                                usize::MAX
                            } else {
                                ft.params.len()
                            };
                            if arg_types.len() < min_params || arg_types.len() > max_params {
                                continue;
                            }
                            return if !ft.type_params.is_empty() {
                                let inferred = Self::infer_type_arguments(
                                    &arg_types,
                                    &ft.params,
                                    &ft.type_params,
                                );
                                Self::substitute(&ft.return_type, &inferred)
                            } else {
                                Type::clone(&ft.return_type)
                            };
                        }
                        Type::Any
                    }
                    // TypeReference that maps to an interface with call signatures
                    Type::TypeReference(ref name, _) => {
                        let instance = self.get_class_instance_type(name);
                        if let Type::ObjectType(ref info) = instance {
                            if !info.call_signatures.is_empty() {
                                let arg_types: Vec<Type> = call
                                    .args
                                    .iter()
                                    .map(|a| Self::widen_nested_literals(&self.infer_expr_type(a)))
                                    .collect();
                                // Try each call signature, pick best match
                                for sig in &info.call_signatures {
                                    if sig.params.len() >= arg_types.len() || sig.params.is_empty()
                                    {
                                        if !sig.type_params.is_empty() {
                                            let inferred = Self::infer_type_arguments(
                                                &arg_types,
                                                &sig.params,
                                                &sig.type_params,
                                            );
                                            return Self::substitute(&sig.return_type, &inferred);
                                        }
                                        return Type::clone(&sig.return_type);
                                    }
                                }
                                // Fallback to first sig
                                let sig = &info.call_signatures[0];
                                if !sig.type_params.is_empty() {
                                    let inferred = Self::infer_type_arguments(
                                        &arg_types,
                                        &sig.params,
                                        &sig.type_params,
                                    );
                                    Self::substitute(&sig.return_type, &inferred)
                                } else {
                                    Type::clone(&sig.return_type)
                                }
                            } else {
                                Type::Any
                            }
                        } else {
                            Type::Any
                        }
                    }
                    // A callee typed as an inline object type literal carrying
                    // call signatures (e.g. `const f: { (x: string): string;
                    // (x: number): number }`) — mirror the interface-with-call-
                    // signatures path above so the call returns the signature's
                    // result instead of falling through to `any`.
                    Type::ObjectType(ref info) if !info.call_signatures.is_empty() => {
                        let arg_types: Vec<Type> = call
                            .args
                            .iter()
                            .map(|a| Self::widen_nested_literals(&self.infer_expr_type(a)))
                            .collect();
                        for sig in &info.call_signatures {
                            if sig.params.len() >= arg_types.len() || sig.params.is_empty() {
                                if !sig.type_params.is_empty() {
                                    let inferred = Self::infer_type_arguments(
                                        &arg_types,
                                        &sig.params,
                                        &sig.type_params,
                                    );
                                    return Self::substitute(&sig.return_type, &inferred);
                                }
                                return Type::clone(&sig.return_type);
                            }
                        }
                        let sig = &info.call_signatures[0];
                        if !sig.type_params.is_empty() {
                            let inferred = Self::infer_type_arguments(
                                &arg_types,
                                &sig.params,
                                &sig.type_params,
                            );
                            Self::substitute(&sig.return_type, &inferred)
                        } else {
                            Type::clone(&sig.return_type)
                        }
                    }
                    _ => Type::Any,
                }
            }
            ExprKind::New(new_expr) => {
                if let ExprKind::Ident(ref name) = new_expr.callee.kind {
                    // Special-case: new Array(...) → infer element type
                    if name == "Array" {
                        if let Some(ref ta) = new_expr.type_args {
                            if let Some(first) = ta.first() {
                                return Type::Array(Arc::new(self.resolve_type_node(first)));
                            }
                        }
                        // Infer from args: new Array("s") → string[]
                        if let Some(ref args) = new_expr.args {
                            if !args.is_empty() {
                                // Single numeric arg = array length → any[]
                                if args.len() == 1 {
                                    let arg_ty = self.infer_expr_type(&args[0]);
                                    if matches!(arg_ty, Type::Number | Type::NumberLiteral(_)) {
                                        return Type::Array(Arc::new(Type::Any));
                                    }
                                    return Type::Array(Arc::new(Self::widen_nested_literals(
                                        &arg_ty,
                                    )));
                                }
                                let elem_types: Vec<Type> = args
                                    .iter()
                                    .map(|a| Self::widen_nested_literals(&self.infer_expr_type(a)))
                                    .collect();
                                let elem = if elem_types.len() == 1 {
                                    elem_types.into_iter().next().unwrap()
                                } else {
                                    Type::flatten_union(elem_types)
                                };
                                return Type::Array(Arc::new(elem));
                            }
                        }
                        return Type::Array(Arc::new(Type::Any));
                    }
                    // A class declared inside a namespace is registered under its
                    // qualified name (`N.Q`); inside that namespace `new Q(...)`
                    // names it bare.
                    let qualified_key: Option<std::string::String> =
                        if self.class_info.contains_key(name.as_str()) {
                            None
                        } else {
                            let mut candidates = self.class_info.keys().filter(|key| {
                                key.rsplit_once('.').is_some_and(|(_, tail)| tail == name)
                            });
                            match (candidates.next(), candidates.next()) {
                                (Some(only), None) => Some(only.clone()),
                                _ => None,
                            }
                        };
                    let lookup_name: &str = qualified_key.as_deref().unwrap_or(name.as_str());
                    // Preserve type arguments: new Map<string, number>() -> Map<string, number>
                    let type_args: Vec<Type> = if let Some(ref args) = new_expr.type_args {
                        args.iter().map(|a| self.resolve_type_node(a)).collect()
                    } else {
                        // Infer type args from constructor arguments
                        let info_opt = self.class_info.get(lookup_name);
                        if let Some(info) = info_opt {
                            if !info.type_params.is_empty() && !info.constructor_params.is_empty() {
                                let ctor_params: Vec<(std::string::String, Type)> = info
                                    .constructor_params
                                    .iter()
                                    .map(|(n, t, _, _)| (n.clone(), t.clone()))
                                    .collect();
                                let type_params = info.type_params.clone();
                                let call_args: Vec<Type> = new_expr
                                    .args
                                    .as_ref()
                                    .map(|args| {
                                        args.iter()
                                            .map(|a| {
                                                Self::widen_nested_literals(
                                                    &self.infer_expr_type(a),
                                                )
                                            })
                                            .collect()
                                    })
                                    .unwrap_or_default();
                                let inferred = Self::infer_type_arguments(
                                    &call_args,
                                    &ctor_params,
                                    &type_params,
                                );
                                type_params
                                    .iter()
                                    .map(|tp| inferred.get(tp).cloned().unwrap_or(Type::Unknown))
                                    .collect()
                            } else {
                                vec![Type::Unknown; info.type_params.len()]
                            }
                        } else {
                            Vec::new()
                        }
                    };
                    // Always return TypeReference — preserves the class name for display
                    // (get_class_instance_type expands to ObjectType, losing the name)
                    Type::TypeReference(name.to_string(), type_args.into())
                } else {
                    Type::Any
                }
            }
            ExprKind::Member(mem) => {
                let obj_ty = self.infer_expr_type(&mem.object);
                match &obj_ty {
                    Type::ObjectType(info) => {
                        info.properties
                            .iter()
                            .find(|(n, _)| n == &mem.property)
                            .map(|(_, t)| Type::clone(&t))
                            .or_else(|| {
                                // Fall back to index signature if property not found
                                info.index_signature.as_ref().map(|(_, v)| Type::clone(&v))
                            })
                            .unwrap_or(Type::Any)
                    }
                    Type::TypeReference(name, type_args) => {
                        // Check if this is an enum member access (e.g. Colors.Cornflower)
                        if let Some(members) = self.enum_info.get(name.as_str()) {
                            if members.iter().any(|(n, _)| n == &mem.property) {
                                return Type::TypeReference(
                                    name.clone(),
                                    Arc::from([] as [Type; 0]),
                                );
                            }
                        }
                        // Build substitution map from class type params → type args
                        let subst_map: Option<
                            std::collections::HashMap<std::string::String, Type>,
                        > = {
                            let type_params = self
                                .class_info
                                .get(name.as_str())
                                .map(|info| &info.type_params)
                                .or_else(|| {
                                    self.interface_info
                                        .get(name.as_str())
                                        .map(|info| &info.type_params)
                                });
                            type_params.and_then(|tps| {
                                if tps.is_empty() || type_args.is_empty() {
                                    None
                                } else {
                                    let mut map = std::collections::HashMap::new();
                                    for (tp, ta) in tps.iter().zip(type_args.iter()) {
                                        map.insert(tp.clone(), ta.clone());
                                    }
                                    Some(map)
                                }
                            })
                        };
                        // Look up property in class/interface info
                        let instance_ty = self.get_class_instance_type(name);
                        let raw_ty = if let Type::ObjectType(ref info) = instance_ty {
                            info.properties
                                .iter()
                                .find(|(n, _)| n == &mem.property)
                                .map(|(_, t)| Type::clone(&t))
                                .unwrap_or_else(|| {
                                    self.builtins
                                        .lookup_instance_property(name, &mem.property)
                                        .unwrap_or(Type::Any)
                                })
                        } else {
                            self.builtins
                                .lookup_instance_property(name, &mem.property)
                                .unwrap_or(Type::Any)
                        };
                        // Substitute class type parameters if available
                        if let Some(ref map) = subst_map {
                            Self::substitute(&raw_ty, map)
                        } else {
                            raw_ty
                        }
                    }
                    Type::String | Type::StringLiteral(_) => self
                        .builtins
                        .lookup_instance_property("String", &mem.property)
                        .unwrap_or(Type::Any),
                    Type::Number | Type::NumberLiteral(_) => self
                        .builtins
                        .lookup_instance_property("Number", &mem.property)
                        .unwrap_or(Type::Any),
                    Type::Array(elem) => self
                        .builtins
                        .lookup_array_method_typed(elem, &mem.property)
                        .or_else(|| {
                            self.builtins
                                .lookup_instance_property("Array", &mem.property)
                        })
                        .unwrap_or(Type::Any),
                    _ => Type::Any,
                }
            }
            ExprKind::ElemAccess(ea) => {
                let obj_ty = self.infer_expr_type(&ea.object);
                match &obj_ty {
                    Type::Array(elem) => Type::clone(&elem),
                    Type::Tuple(elems) => {
                        // Try to get index from literal
                        if let ExprKind::NumLit(n) = &ea.index.kind {
                            let idx = n.parse::<usize>().unwrap_or(0);
                            return elems.get(idx).cloned().unwrap_or(Type::Any);
                        }
                        Type::Any
                    }
                    _ => Type::Any,
                }
            }
            ExprKind::Assign(assign) => self.infer_expr_type(&assign.right),
            ExprKind::NonNull(inner) => {
                let ty = self.infer_expr_type(inner);
                self.remove_null_undefined(&ty)
            }
            ExprKind::Spread(inner) => {
                let ty = self.infer_expr_type(inner);
                match &ty {
                    Type::Array(element) => Type::clone(element),
                    Type::Tuple(elements) if !elements.is_empty() => {
                        Type::flatten_union(elements.iter().cloned().collect())
                    }
                    Type::TypeReference(name, args) => match (name.as_str(), args.as_ref()) {
                        (
                            "Set"
                            | "ReadonlySet"
                            | "Iterable"
                            | "IterableIterator"
                            | "Generator"
                            | "AsyncIterable"
                            | "AsyncIterableIterator"
                            | "AsyncGenerator"
                            | "ReadonlyArray"
                            | "Array",
                            [element, ..],
                        ) => element.clone(),
                        ("Map" | "ReadonlyMap" | "WeakMap", [key, value, ..]) => {
                            Type::Tuple(Arc::from([key.clone(), value.clone()]))
                        }
                        (
                            "Set"
                            | "ReadonlySet"
                            | "Iterable"
                            | "IterableIterator"
                            | "Generator"
                            | "AsyncIterable"
                            | "AsyncIterableIterator"
                            | "AsyncGenerator"
                            | "ReadonlyArray"
                            | "Array"
                            | "Map"
                            | "ReadonlyMap"
                            | "WeakMap",
                            [],
                        ) => Type::Any,
                        _ => ty.clone(),
                    },
                    Type::String | Type::StringLiteral(_) => Type::String,
                    _ => ty,
                }
            }
            _ => Type::Any,
        }
    }

    /// Infer the return type of a callback expression (arrow or function).
    /// `elem_type` is the element type of the array the callback operates on.
    /// If `callback` is a function whose signature is a type guard (`x is S`),
    /// return the narrowed target type `S`. Used by `Array.prototype.find` /
    /// `filter` to narrow their result (the overloads
    /// `find<S extends T>(p: (v: T) => v is S): S | undefined` and
    /// `filter<S extends T>(p: (v: T) => v is S): S[]`). Returns None for plain
    /// boolean-returning callbacks and `asserts` predicates.
    pub(crate) fn callback_type_guard_target(&self, callback: &Expr) -> Option<Type> {
        let cb_ty = self.infer_expr_type(callback);
        if let Type::Function(ft) = &cb_ty {
            if let Some(pred) = &ft.type_predicate {
                if !pred.is_asserts {
                    return Some(Type::clone(&pred.target_type));
                }
            }
        }
        None
    }

    pub(crate) fn infer_callback_return_type(&self, callback: &Expr, elem_type: &Type) -> Type {
        match &callback.kind {
            ExprKind::Arrow(arrow) => {
                // Check return type annotation first
                if let Some(ref ret_ann) = arrow.return_type {
                    return self.resolve_type_node(ret_ann);
                }
                // Infer from body expression (single-expression arrows)
                match &arrow.body {
                    tsc_rs_ast::ArrowBody::Expr(body_expr) => {
                        // Simple heuristic: if body is a binary op on the param, infer type
                        match &body_expr.kind {
                            ExprKind::Binary(bin) => match bin.op {
                                BinaryOp::Add => {
                                    let left = self.infer_expr_type(&bin.left);
                                    let right = self.infer_expr_type(&bin.right);
                                    if matches!(
                                        self.widen_type(&left),
                                        Type::String | Type::StringLiteral(_)
                                    ) || matches!(
                                        self.widen_type(&right),
                                        Type::String | Type::StringLiteral(_)
                                    ) {
                                        Type::String
                                    } else {
                                        Type::Number
                                    }
                                }
                                BinaryOp::Sub
                                | BinaryOp::Mul
                                | BinaryOp::Div
                                | BinaryOp::Mod
                                | BinaryOp::Exp => Type::Number,
                                BinaryOp::Lt
                                | BinaryOp::Le
                                | BinaryOp::Gt
                                | BinaryOp::Ge
                                | BinaryOp::Eq
                                | BinaryOp::Ne
                                | BinaryOp::StrictEq
                                | BinaryOp::StrictNe => Type::Boolean,
                                _ => Type::Any,
                            },
                            ExprKind::Call(inner_call) => {
                                // e.g., n => String(n) — infer from the call
                                if let ExprKind::Ident(name) = &inner_call.callee.kind {
                                    match name.as_str() {
                                        "String" => return Type::String,
                                        "Number" => return Type::Number,
                                        "Boolean" => return Type::Boolean,
                                        _ => {
                                            if let Some(ty) = self.lookup_var(name) {
                                                if let Type::Function(ft) = ty {
                                                    return Type::clone(&ft.return_type);
                                                }
                                            }
                                        }
                                    }
                                }
                                Type::Any
                            }
                            ExprKind::Member(_) => {
                                // e.g., n => n.toString() — infer member type
                                self.infer_expr_type(body_expr)
                            }
                            ExprKind::Ident(_) => {
                                // Identity callback: x => x — returns element type
                                elem_type.clone()
                            }
                            _ => self.infer_expr_type(body_expr),
                        }
                    }
                    tsc_rs_ast::ArrowBody::Block(_) => {
                        // Block body — would need return statement analysis
                        Type::Any
                    }
                }
            }
            ExprKind::FnExpr(_) => Type::Any,
            ExprKind::Ident(name) => {
                // Passing a named function: nums.map(String) etc.
                match name.as_str() {
                    "String" => Type::String,
                    "Number" => Type::Number,
                    "Boolean" => Type::Boolean,
                    _ => {
                        if let Some(ty) = self.lookup_var(name) {
                            if let Type::Function(ft) = ty {
                                return Type::clone(&ft.return_type);
                            }
                        }
                        Type::Any
                    }
                }
            }
            // Satisfies preserves the expression type (not the asserted type)
            ExprKind::Satisfies(s) => self.infer_expr_type(&s.expr),
            // `this` in a class context
            ExprKind::This => Type::This,
            // void x evaluates to undefined
            ExprKind::Unary(u) if u.op == UnaryOp::Void => Type::Undefined,
            // Logical NOT returns boolean
            ExprKind::Unary(u) if u.op == UnaryOp::LogNot => Type::Boolean,
            // typeof x returns the canonical typeof-label union (8 string lits)
            ExprKind::Typeof(_) => TypeChecker::typeof_result_type(),
            _ => Type::Any,
        }
    }
}
