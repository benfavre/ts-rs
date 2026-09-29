//! Type operations: conditional/mapped type expansion, keyof resolution,
//! indexed access resolution, and union/intersection simplification.
//!
//! Ported from stc's type analysis passes. These are pure functions operating
//! on the `Type` enum defined in this crate.

use std::sync::Arc;

use std::collections::HashMap;

use crate::{FunctionType, MappedModifierKind, ObjectTypeInfo, Type};

// ---------------------------------------------------------------------------
// Extends result
// ---------------------------------------------------------------------------

/// Result of an `extends` check between two types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtendsResult {
    True,
    False,
    /// Cannot be determined statically (e.g. unresolved type parameters).
    Unknown,
}

// ---------------------------------------------------------------------------
// Substitution helper
// ---------------------------------------------------------------------------

/// Replace every occurrence of `TypeParameter(name)` with `replacement`,
/// recursing through all `Type` variants.
pub fn substitute(ty: &Type, name: &str, replacement: &Type) -> Type {
    // Depth + cumulative-instantiation guard: this single-parameter substitute
    // is the chokepoint for exponential tuple growth (`[...T, ...T]` inserts the
    // replacement at every TypeParameter occurrence, doubling size each level).
    if crate::type_op_enter() {
        return ty.clone();
    }
    let result = substitute_inner(ty, name, replacement);
    crate::type_op_exit();
    result
}

fn substitute_inner(ty: &Type, name: &str, replacement: &Type) -> Type {
    match ty {
        Type::TypeParameter(n) if n == name => replacement.clone(),

        // Primitives / leaves -- no children to recurse into
        Type::Any
        | Type::Unknown
        | Type::Number
        | Type::String
        | Type::Boolean
        | Type::Void
        | Type::Undefined
        | Type::Null
        | Type::Never
        | Type::Object
        | Type::Symbol
        | Type::BigInt
        | Type::NumberLiteral(_)
        | Type::StringLiteral(_)
        | Type::BooleanLiteral(_)
        | Type::BigIntLiteral(_)
        | Type::This
        | Type::TypeParameter(_)
        | Type::Typeof(_)
        | Type::Infer(_)
        | Type::UniqueSymbol(_)
        | Type::Error => ty.clone(),

        // Compound -- recurse
        Type::Array(inner) => Type::Array(Arc::new(substitute(inner, name, replacement))),

        Type::Tuple(elems) => Type::Tuple(
            elems
                .iter()
                .map(|e| substitute(e, name, replacement))
                .collect(),
        ),

        Type::Union(members) => Type::Union(
            members
                .iter()
                .map(|m| substitute(m, name, replacement))
                .collect(),
        ),

        Type::Intersection(members) => Type::Intersection(
            members
                .iter()
                .map(|m| substitute(m, name, replacement))
                .collect(),
        ),

        Type::Function(ft) => Type::Function(FunctionType {
            type_param_constraints: ft
                .type_param_constraints
                .iter()
                .map(|default| default.as_ref().map(|ty| substitute(ty, name, replacement)))
                .collect(),
            params: ft
                .params
                .iter()
                .map(|(n, t)| (n.clone(), substitute(t, name, replacement)))
                .collect(),
            return_type: Arc::new(substitute(&ft.return_type, name, replacement)),
            type_params: ft.type_params.clone(),
            type_param_defaults: ft
                .type_param_defaults
                .iter()
                .map(|default| default.as_ref().map(|ty| substitute(ty, name, replacement)))
                .collect(),
            type_predicate: ft.type_predicate.clone(),
        }),

        Type::ObjectType(obj) => Type::ObjectType(substitute_object(obj, name, replacement)),

        Type::TypeReference(ref_name, args) if args.is_empty() && ref_name == name => {
            replacement.clone()
        }
        Type::TypeReference(ref_name, args) => Type::TypeReference(
            ref_name.clone(),
            args.iter()
                .map(|a| substitute(a, name, replacement))
                .collect(),
        ),

        Type::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => Type::Conditional {
            check: Arc::new(substitute(check, name, replacement)),
            extends: Arc::new(substitute(extends, name, replacement)),
            true_type: Arc::new(substitute(true_type, name, replacement)),
            false_type: Arc::new(substitute(false_type, name, replacement)),
        },

        Type::Mapped {
            param,
            constraint,
            template,
            name_type,
            readonly_mod,
            optional_mod,
        } => {
            // If the mapped param shadows the substitution target, don't recurse into template
            if param == name {
                ty.clone()
            } else {
                Type::Mapped {
                    param: param.clone(),
                    constraint: Arc::new(substitute(constraint, name, replacement)),
                    template: template
                        .as_ref()
                        .map(|t| Arc::new(substitute(t, name, replacement))),
                    name_type: name_type
                        .as_ref()
                        .map(|n| Arc::new(substitute(n, name, replacement))),
                    readonly_mod: *readonly_mod,
                    optional_mod: *optional_mod,
                }
            }
        }

        Type::IndexedAccess(obj, idx) => Type::IndexedAccess(
            Arc::new(substitute(obj, name, replacement)),
            Arc::new(substitute(idx, name, replacement)),
        ),

        Type::Keyof(inner) => Type::Keyof(Arc::new(substitute(inner, name, replacement))),

        Type::TemplateLiteral { quasis, types } => Type::TemplateLiteral {
            quasis: quasis.clone(),
            types: types
                .iter()
                .map(|t| substitute(t, name, replacement))
                .collect(),
        },

        Type::Constructor(ct) if ct.type_params.iter().any(|parameter| parameter == name) => {
            ty.clone()
        }
        Type::Constructor(ct) => Type::Constructor(crate::ConstructorType {
            is_abstract: ct.is_abstract,
            params: ct
                .params
                .iter()
                .map(|(n, t)| (n.clone(), substitute(t, name, replacement)))
                .collect(),
            return_type: Arc::new(substitute(&ct.return_type, name, replacement)),
            type_params: ct.type_params.clone(),
            type_param_constraints: ct
                .type_param_constraints
                .iter()
                .map(|constraint| {
                    constraint
                        .as_ref()
                        .map(|ty| substitute(ty, name, replacement))
                })
                .collect(),
            type_param_defaults: ct
                .type_param_defaults
                .iter()
                .map(|default| default.as_ref().map(|ty| substitute(ty, name, replacement)))
                .collect(),
        }),

        Type::Rest(inner) => Type::Rest(Arc::new(substitute(inner, name, replacement))),
        Type::Optional(inner) => Type::Optional(Arc::new(substitute(inner, name, replacement))),
        Type::Readonly(inner) => Type::Readonly(Arc::new(substitute(inner, name, replacement))),
        Type::Instance(inner) => Type::Instance(Arc::new(substitute(inner, name, replacement))),

        Type::Import {
            module,
            name: import_name,
            type_args,
        } => Type::Import {
            module: module.clone(),
            name: import_name.clone(),
            type_args: type_args
                .iter()
                .map(|a| substitute(a, name, replacement))
                .collect(),
        },

        Type::Predicate(pred) => Type::Predicate(crate::TypePredicate {
            param_name: pred.param_name.clone(),
            target_type: Arc::new(substitute(&pred.target_type, name, replacement)),
            is_asserts: pred.is_asserts,
        }),

        Type::Namespace(ns) => Type::Namespace(crate::NamespaceType {
            name: ns.name.clone(),
            exports: ns
                .exports
                .iter()
                .map(|(n, t)| (n.clone(), substitute(t, name, replacement)))
                .collect(),
            readonly_exports: ns.readonly_exports.clone(),
        }),

        Type::Module(m) => Type::Module(crate::ModuleType {
            name: m.name.clone(),
            exports: m
                .exports
                .iter()
                .map(|(n, t)| (n.clone(), substitute(t, name, replacement)))
                .collect(),
        }),

        Type::EnumType(e) => Type::EnumType(crate::EnumTypeInfo {
            name: e.name.clone(),
            members: e
                .members
                .iter()
                .map(|(n, t)| (n.clone(), substitute(t, name, replacement)))
                .collect(),
            is_const: e.is_const,
        }),

        Type::EnumVariant {
            enum_name,
            variant_name,
            value,
        } => Type::EnumVariant {
            enum_name: enum_name.clone(),
            variant_name: variant_name.clone(),
            value: value
                .as_ref()
                .map(|v| Arc::new(substitute(v, name, replacement))),
        },

        Type::StringMapping { kind, inner } => Type::StringMapping {
            kind: *kind,
            inner: Arc::new(substitute(inner, name, replacement)),
        },
    }
}

fn substitute_object(obj: &ObjectTypeInfo, name: &str, replacement: &Type) -> ObjectTypeInfo {
    ObjectTypeInfo {
        properties: obj
            .properties
            .iter()
            .map(|(n, t)| (n.clone(), Arc::new(substitute(t, name, replacement))))
            .collect(),
        call_signatures: obj
            .call_signatures
            .iter()
            .map(|ft| FunctionType {
                type_param_constraints: ft
                    .type_param_constraints
                    .iter()
                    .map(|default| default.as_ref().map(|ty| substitute(ty, name, replacement)))
                    .collect(),
                params: ft
                    .params
                    .iter()
                    .map(|(n, t)| (n.clone(), substitute(t, name, replacement)))
                    .collect(),
                return_type: Arc::new(substitute(&ft.return_type, name, replacement)),
                type_params: ft.type_params.clone(),
                type_param_defaults: ft
                    .type_param_defaults
                    .iter()
                    .map(|default| default.as_ref().map(|ty| substitute(ty, name, replacement)))
                    .collect(),
                type_predicate: ft.type_predicate.clone(),
            })
            .collect(),
        construct_signatures: obj
            .construct_signatures
            .iter()
            .map(|constructor| {
                match substitute(&Type::Constructor(constructor.clone()), name, replacement) {
                    Type::Constructor(signature) => signature,
                    _ => unreachable!("constructor substitution changed its kind"),
                }
            })
            .collect(),
        index_signature: obj.index_signature.as_ref().map(|(k, v)| {
            (
                Arc::new(substitute(k, name, replacement)),
                Arc::new(substitute(v, name, replacement)),
            )
        }),
        index_signature_name: obj.index_signature_name.clone(),
        method_names: obj.method_names.clone(),
    }
}

/// Apply substitution from a map of type parameters to their replacements.
pub fn substitute_all(ty: &Type, params: &HashMap<String, Type>) -> Type {
    let mut result = ty.clone();
    for (name, replacement) in params {
        result = substitute(&result, name, replacement);
    }
    result
}

// ---------------------------------------------------------------------------
// Extends check (simple)
// ---------------------------------------------------------------------------

/// Determine whether `check` extends `target`.
///
/// This is a simplified check that handles the common cases used in
/// conditional type resolution. Returns `Unknown` for anything non-trivial
/// (e.g. unresolved type parameters, complex generics).
pub fn is_extends(check: &Type, target: &Type) -> ExtendsResult {
    // Identical types always extend each other.
    if check == target {
        return ExtendsResult::True;
    }

    // `never` extends everything.
    if matches!(check, Type::Never) {
        return ExtendsResult::True;
    }

    // Everything extends `any` and `unknown`.
    if matches!(target, Type::Any | Type::Unknown) {
        return ExtendsResult::True;
    }

    // `any` extends everything except `never`.
    if matches!(check, Type::Any) {
        if matches!(target, Type::Never) {
            return ExtendsResult::False;
        }
        // `any` conditionals are special -- they distribute to both branches.
        return ExtendsResult::Unknown;
    }

    // Nothing (other than never/any handled above) extends `never`.
    if matches!(target, Type::Never) {
        return ExtendsResult::False;
    }

    // Literal types extend their base type.
    match (check, target) {
        (Type::StringLiteral(_), Type::String) => return ExtendsResult::True,
        (Type::NumberLiteral(_), Type::Number) => return ExtendsResult::True,
        (Type::BooleanLiteral(_), Type::Boolean) => return ExtendsResult::True,
        (Type::BigIntLiteral(_), Type::BigInt) => return ExtendsResult::True,
        _ => {}
    }

    // Null/Undefined extend themselves only (already covered by equality check).
    if matches!(check, Type::Null) && !matches!(target, Type::Null | Type::Any | Type::Unknown) {
        return ExtendsResult::False;
    }
    if matches!(check, Type::Undefined)
        && !matches!(
            target,
            Type::Undefined | Type::Void | Type::Any | Type::Unknown
        )
    {
        return ExtendsResult::False;
    }

    // Void extends only void, any, unknown (any/unknown already handled).
    if matches!(check, Type::Void) && !matches!(target, Type::Void) {
        return ExtendsResult::False;
    }

    // Primitive vs. primitive mismatches.
    if is_primitive(check) && is_primitive(target) && check != target {
        return ExtendsResult::False;
    }

    // Union check: all members must extend target.
    if let Type::Union(members) = check {
        let mut all_true = true;
        let mut any_unknown = false;
        for member in members.iter() {
            match is_extends(member, target) {
                ExtendsResult::True => {}
                ExtendsResult::False => return ExtendsResult::False,
                ExtendsResult::Unknown => {
                    all_true = false;
                    any_unknown = true;
                }
            }
        }
        if all_true {
            return ExtendsResult::True;
        }
        if any_unknown {
            return ExtendsResult::Unknown;
        }
    }

    // Intersection check: check extends target if any member of intersection extends target.
    if let Type::Intersection(members) = check {
        for member in members.iter() {
            if is_extends(member, target) == ExtendsResult::True {
                return ExtendsResult::True;
            }
        }
    }

    // Target union: check extends union if check extends any member.
    if let Type::Union(members) = target {
        for member in members.iter() {
            if is_extends(check, member) == ExtendsResult::True {
                return ExtendsResult::True;
            }
        }
        return ExtendsResult::False;
    }

    // Unresolved type parameters.
    if matches!(check, Type::TypeParameter(_)) || matches!(target, Type::TypeParameter(_)) {
        return ExtendsResult::Unknown;
    }

    // Array extends Array if element extends element.
    if let (Type::Array(a), Type::Array(b)) = (check, target) {
        return is_extends(a, b);
    }

    // Tuple extends Array.
    if let (Type::Tuple(_), Type::Array(elem)) = (check, target) {
        // Simplified: tuple extends Array<T> if all tuple elements extend T.
        if let Type::Tuple(elems) = check {
            let mut all = true;
            for e in elems.iter() {
                if is_extends(e, elem) != ExtendsResult::True {
                    all = false;
                    break;
                }
            }
            if all {
                return ExtendsResult::True;
            }
        }
    }

    ExtendsResult::Unknown
}

fn is_primitive(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Number
            | Type::String
            | Type::Boolean
            | Type::Void
            | Type::Undefined
            | Type::Null
            | Type::Symbol
            | Type::BigInt
            | Type::Never
    )
}

// ---------------------------------------------------------------------------
// Conditional type expansion
// ---------------------------------------------------------------------------

/// Expand a conditional type `check extends extends_ty ? true_type : false_type`.
///
/// If `check` is a naked type parameter that is a union, the conditional
/// distributes over the union members per TypeScript semantics.
///
/// `type_params` maps type parameter names to their concrete types (if known).
pub fn expand_conditional(
    check: &Type,
    extends_ty: &Type,
    true_type: &Type,
    false_type: &Type,
    type_params: &HashMap<String, Type>,
) -> Type {
    // Budget guard: recursive conditional types (e.g. `Join`, `SnakeToPascalCase`,
    // `Trim` in templateLiteralTypes1) distribute over unions and re-enter here;
    // count them against the per-statement instantiation budget so they cannot
    // expand without bound.
    if crate::type_op_enter() {
        return Type::Any;
    }
    let result = expand_conditional_inner(check, extends_ty, true_type, false_type, type_params);
    crate::type_op_exit();
    result
}

fn expand_conditional_inner(
    check: &Type,
    extends_ty: &Type,
    true_type: &Type,
    false_type: &Type,
    type_params: &HashMap<String, Type>,
) -> Type {
    let check = if type_params.is_empty() {
        check.clone()
    } else {
        substitute_all(check, type_params)
    };
    let extends_ty = substitute_all(extends_ty, type_params);

    // The BRANCHES are substituted lazily — only the one actually taken.
    // Substituting both eagerly expands a self-referential branch
    // (`type Trim<S> = S extends ` ${infer T}` ? Trim<T> : …`) on every step,
    // which is exponential and exhausted tens of GB before any budget tripped.
    // tsc evaluates the check first and instantiates only the selected branch.
    let take = |t: &Type| -> Type {
        if type_params.is_empty() {
            t.clone()
        } else {
            substitute_all(t, type_params)
        }
    };

    // Distribution: if the check type is a union, distribute conditional over members.
    if let Type::Union(members) = &check {
        let results: Vec<Type> = members
            .iter()
            .map(|member| match is_extends(member, &extends_ty) {
                ExtendsResult::True => take(true_type),
                ExtendsResult::False => take(false_type),
                ExtendsResult::Unknown => Type::Conditional {
                    check: Arc::new(member.clone()),
                    extends: Arc::new(extends_ty.clone()),
                    true_type: Arc::new(take(true_type)),
                    false_type: Arc::new(take(false_type)),
                },
            })
            .collect();
        return simplify_union(results);
    }

    match is_extends(&check, &extends_ty) {
        ExtendsResult::True => take(true_type),
        ExtendsResult::False => take(false_type),
        ExtendsResult::Unknown => Type::Conditional {
            check: Arc::new(check),
            extends: Arc::new(extends_ty),
            true_type: Arc::new(take(true_type)),
            false_type: Arc::new(take(false_type)),
        },
    }
}

fn expand_conditional_single(
    check: &Type,
    extends_ty: &Type,
    true_type: &Type,
    false_type: &Type,
) -> Type {
    match is_extends(check, extends_ty) {
        ExtendsResult::True => true_type.clone(),
        ExtendsResult::False => false_type.clone(),
        ExtendsResult::Unknown => Type::Conditional {
            check: Arc::new(check.clone()),
            extends: Arc::new(extends_ty.clone()),
            true_type: Arc::new(true_type.clone()),
            false_type: Arc::new(false_type.clone()),
        },
    }
}

// ---------------------------------------------------------------------------
// Mapped type expansion
// ---------------------------------------------------------------------------

/// Expand a mapped type `{ [P in Constraint]: Template }`.
///
/// Resolves the constraint to a set of keys, then builds an `ObjectType`
/// by substituting `param` with each key in the template.
///
/// Returns the original `Mapped` type if keys cannot be statically resolved.
#[allow(clippy::too_many_arguments)]
pub fn expand_mapped(
    param: &str,
    constraint: &Type,
    template: Option<&Type>,
    name_type: Option<&Type>,
    readonly_mod: Option<MappedModifierKind>,
    optional_mod: Option<MappedModifierKind>,
    type_params: &HashMap<String, Type>,
) -> Type {
    let constraint = substitute_all(constraint, type_params);

    let keys = resolve_keys(&constraint);

    let keys = match keys {
        Some(k) => k,
        None => {
            // Cannot resolve keys statically -- return as-is.
            return Type::Mapped {
                param: param.to_string(),
                constraint: Arc::new(constraint),
                template: template.map(|t| Arc::new(substitute_all(t, type_params))),
                name_type: name_type.map(|n| Arc::new(substitute_all(n, type_params))),
                readonly_mod,
                optional_mod,
            };
        }
    };

    let template = match template {
        Some(t) => substitute_all(t, type_params),
        None => Type::Any,
    };

    let properties: Vec<(String, Arc<Type>)> = keys
        .into_iter()
        .map(|key| {
            let prop_type = substitute(&template, param, &Type::StringLiteral(key.clone()));
            (key, Arc::new(prop_type))
        })
        .collect();

    // Apply readonly/optional modifiers to the resulting ObjectType.
    // For now we produce a plain ObjectType; modifier handling is noted
    // but has no structural effect since ObjectTypeInfo doesn't track them per-property.
    let _ = readonly_mod;
    let _ = optional_mod;

    Type::ObjectType(ObjectTypeInfo {
        properties,
        call_signatures: Vec::new(),
        construct_signatures: Vec::new(),
        index_signature: None,
        index_signature_name: None,
        method_names: Vec::new(),
    })
}

/// Try to extract a list of string keys from a type (for mapped type expansion).
fn resolve_keys(ty: &Type) -> Option<Vec<String>> {
    match ty {
        Type::StringLiteral(s) => Some(vec![s.clone()]),

        Type::Union(members) => {
            let mut keys = Vec::new();
            for member in members.iter() {
                match resolve_keys(member) {
                    Some(k) => keys.extend(k),
                    None => return None,
                }
            }
            Some(keys)
        }

        Type::Keyof(inner) => resolve_keyof_keys(inner),

        Type::Never => Some(Vec::new()),

        _ => None,
    }
}

/// Extract property name keys for `keyof` resolution (used internally by `resolve_keys`).
fn resolve_keyof_keys(ty: &Type) -> Option<Vec<String>> {
    match ty {
        Type::ObjectType(obj) => Some(obj.properties.iter().map(|(n, _)| n.clone()).collect()),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Keyof resolution
// ---------------------------------------------------------------------------

/// Resolve `keyof T` to a concrete type.
///
/// - `ObjectType` -> union of string literal property names
/// - `Union`      -> intersection of keys (only common keys)
/// - `Intersection` -> union of keys (all keys)
/// - Other        -> `Keyof(ty)` unchanged
pub fn resolve_keyof(ty: &Type) -> Type {
    match ty {
        Type::ObjectType(obj) => {
            let keys: Vec<Type> = obj
                .properties
                .iter()
                .map(|(name, _)| Type::StringLiteral(name.clone()))
                .collect();
            simplify_union(keys)
        }

        Type::Union(members) => {
            // keyof (A | B) = (keyof A) & (keyof B) -- only common keys.
            if members.is_empty() {
                return Type::Never;
            }
            let key_sets: Vec<Option<Vec<String>>> = members
                .iter()
                .map(|m| match m {
                    Type::ObjectType(obj) => {
                        Some(obj.properties.iter().map(|(n, _)| n.clone()).collect())
                    }
                    _ => None,
                })
                .collect();

            // If any member can't be resolved, fall back to Keyof.
            if key_sets.iter().any(|s| s.is_none()) {
                return Type::Keyof(Arc::new(ty.clone()));
            }

            let mut sets: Vec<Vec<String>> = key_sets.into_iter().map(|s| s.unwrap()).collect();

            if sets.is_empty() {
                return Type::Never;
            }

            // Intersect: keep only keys present in all sets.
            let first = sets.remove(0);
            let common: Vec<String> = first
                .into_iter()
                .filter(|k| sets.iter().all(|s| s.contains(k)))
                .collect();

            let keys: Vec<Type> = common.into_iter().map(Type::StringLiteral).collect();
            simplify_union(keys)
        }

        Type::Intersection(members) => {
            // keyof (A & B) = (keyof A) | (keyof B) -- all keys.
            let mut all_keys: Vec<Type> = Vec::new();
            for member in members.iter() {
                match resolve_keyof(member) {
                    Type::Union(keys) => all_keys.extend(keys.iter().cloned()),
                    Type::StringLiteral(s) => all_keys.push(Type::StringLiteral(s)),
                    Type::Never => {}
                    _other => {
                        // Unresolvable member -- fall back.
                        return Type::Keyof(Arc::new(ty.clone()));
                    }
                }
            }
            simplify_union(all_keys)
        }

        Type::Array(_) | Type::Tuple(_) => {
            // keyof array/tuple includes numeric index + array methods.
            // For a simplified model, return `string | number`.
            Type::Union(vec![Type::String, Type::Number].into())
        }

        Type::Any => Type::Union(vec![Type::String, Type::Number, Type::Symbol].into()),

        Type::Never => Type::Never,

        _ => Type::Keyof(Arc::new(ty.clone())),
    }
}

// ---------------------------------------------------------------------------
// Indexed access resolution
// ---------------------------------------------------------------------------

/// Resolve `T[K]` (indexed access type).
///
/// - `ObjectType[StringLiteral]` -> property type
/// - `Array[Number]`             -> element type
/// - `Tuple[NumberLiteral]`      -> specific element type
/// - `Union` index               -> union of results
pub fn resolve_indexed_access(object_type: &Type, index_type: &Type) -> Type {
    // Union index: T[A | B] = T[A] | T[B]
    if let Type::Union(members) = index_type {
        let results: Vec<Type> = members
            .iter()
            .map(|m| resolve_indexed_access(object_type, m))
            .collect();
        return simplify_union(results);
    }

    // Union object: (A | B)[K] = A[K] | B[K]
    if let Type::Union(members) = object_type {
        let results: Vec<Type> = members
            .iter()
            .map(|m| resolve_indexed_access(m, index_type))
            .collect();
        return simplify_union(results);
    }

    // Intersection object: (A & B & …)[K] = the intersection of each
    // member's own `[K]`, skipping members that don't carry the key
    // (they resolve to `Error`). This is what lets a folded
    // `$InferObjectOutput` — a `RequiredPart & OptionalPart & Extra`
    // intersection of concrete objects — answer the outer `Prettify`'s
    // per-key `X[K]` value lookups. Without it the access stays a
    // deferred IndexedAccess and the whole shape reads as lenient.
    if let Type::Intersection(members) = object_type {
        let mut hits: Vec<Type> = Vec::new();
        let mut had_unresolved = false;
        for m in members.iter() {
            let r = resolve_indexed_access(m, index_type);
            match r {
                Type::Error | Type::Never => {}
                // A member that itself couldn't resolve the access keeps
                // the whole thing deferred — don't fabricate a narrower
                // answer from the members that did resolve.
                Type::IndexedAccess(_, _) => had_unresolved = true,
                other => hits.push(other),
            }
        }
        if !had_unresolved {
            match hits.len() {
                0 => return Type::Error,
                1 => return hits.into_iter().next().unwrap(),
                _ => return Type::Intersection(hits.into()),
            }
        }
    }

    match (object_type, index_type) {
        // ObjectType + StringLiteral -> property lookup
        (Type::ObjectType(obj), Type::StringLiteral(key)) => {
            for (name, ty) in &obj.properties {
                if name == key {
                    return Type::clone(&ty);
                }
            }
            // Check index signature
            if let Some((key_type, value_type)) = &obj.index_signature {
                if is_extends(index_type, key_type) == ExtendsResult::True {
                    return Type::clone(&value_type);
                }
            }
            Type::Error
        }

        // ObjectType + Number -> index signature lookup
        (Type::ObjectType(obj), Type::Number) => {
            if let Some((key_type, value_type)) = &obj.index_signature {
                if matches!(key_type.as_ref(), Type::Number | Type::String) {
                    return Type::clone(&value_type);
                }
            }
            Type::Error
        }

        // Array + Number -> element type
        (Type::Array(elem), Type::Number | Type::NumberLiteral(_)) => Type::clone(&elem),

        // Tuple + NumberLiteral -> specific element
        (Type::Tuple(elems), Type::NumberLiteral(idx_str)) => {
            if let Ok(idx) = idx_str.parse::<usize>() {
                if idx < elems.len() {
                    return elems[idx].clone();
                }
            }
            Type::Error
        }

        // Tuple + Number -> union of all element types
        (Type::Tuple(elems), Type::Number) => simplify_union(elems.iter().cloned().collect()),

        // Keyof index with matching object
        (Type::ObjectType(obj), Type::Keyof(inner)) if *inner.as_ref() == *object_type => {
            // T[keyof T] = union of all property types
            let types: Vec<Type> = obj
                .properties
                .iter()
                .map(|(_, t)| Type::clone(&t))
                .collect();
            simplify_union(types)
        }

        // Any indexed by anything returns Any
        (Type::Any, _) => Type::Any,

        _ => Type::IndexedAccess(Arc::new(object_type.clone()), Arc::new(index_type.clone())),
    }
}

// ---------------------------------------------------------------------------
// Union simplification
// ---------------------------------------------------------------------------

/// Simplify a union type:
/// - Flatten nested unions
/// - Remove duplicates
/// - Remove `Never`
/// - Single element -> unwrap
pub fn simplify_union(types: Vec<Type>) -> Type {
    let mut flattened = Vec::new();
    flatten_union(types, &mut flattened);

    // Remove Never
    flattened.retain(|t| !matches!(t, Type::Never));

    // Remove duplicates while preserving order
    dedup_types(&mut flattened);

    // If any member is `any`, the whole union is `any`.
    if flattened.iter().any(|t| matches!(t, Type::Any)) {
        return Type::Any;
    }

    match flattened.len() {
        0 => Type::Never,
        1 => flattened.into_iter().next().unwrap(),
        _ => Type::Union(flattened.into()),
    }
}

fn flatten_union(types: Vec<Type>, out: &mut Vec<Type>) {
    for ty in types.into_iter() {
        match ty {
            Type::Union(members) => flatten_union(members.iter().cloned().collect(), out),
            other => out.push(other),
        }
    }
}

// ---------------------------------------------------------------------------
// Intersection simplification
// ---------------------------------------------------------------------------

/// Simplify an intersection type:
/// - Flatten nested intersections
/// - Remove duplicates
/// - `Never` in any position -> `Never`
pub fn simplify_intersection(types: Vec<Type>) -> Type {
    let mut flattened = Vec::new();
    flatten_intersection(types, &mut flattened);

    // Never absorbs everything.
    if flattened.iter().any(|t| matches!(t, Type::Never)) {
        return Type::Never;
    }

    // `unknown` is the identity for intersection.
    flattened.retain(|t| !matches!(t, Type::Unknown));

    // If any member is `any`, the whole intersection is `any` (TypeScript semantics).
    if flattened.iter().any(|t| matches!(t, Type::Any)) {
        return Type::Any;
    }

    // Remove duplicates while preserving order
    dedup_types(&mut flattened);

    match flattened.len() {
        0 => Type::Unknown,
        1 => flattened.into_iter().next().unwrap(),
        _ => Type::Intersection(flattened.into()),
    }
}

fn flatten_intersection(types: Vec<Type>, out: &mut Vec<Type>) {
    for ty in types.into_iter() {
        match ty {
            Type::Intersection(members) => {
                flatten_intersection(members.iter().cloned().collect(), out)
            }
            other => out.push(other),
        }
    }
}

// ---------------------------------------------------------------------------
// Dedup helper
// ---------------------------------------------------------------------------

fn dedup_types(types: &mut Vec<Type>) {
    let mut seen = Vec::new();
    types.retain(|t| {
        if seen.contains(t) {
            false
        } else {
            seen.push(t.clone());
            true
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- substitute tests --

    #[test]
    fn substitute_simple() {
        let ty = Type::TypeParameter("T".into());
        let result = substitute(&ty, "T", &Type::Number);
        assert_eq!(result, Type::Number);
    }

    #[test]
    fn substitute_in_array() {
        let ty = Type::Array(Arc::new(Type::TypeParameter("T".into())));
        let result = substitute(&ty, "T", &Type::String);
        assert_eq!(result, Type::Array(Arc::new(Type::String)));
    }

    #[test]
    fn substitute_no_match() {
        let ty = Type::TypeParameter("U".into());
        let result = substitute(&ty, "T", &Type::Number);
        assert_eq!(result, Type::TypeParameter("U".into()));
    }

    #[test]
    fn substitute_shadowed_mapped() {
        // Mapped type with param "T" should not substitute inside template
        let ty = Type::Mapped {
            param: "T".into(),
            constraint: Arc::new(Type::String),
            template: Some(Arc::new(Type::TypeParameter("T".into()))),
            name_type: None,
            readonly_mod: None,
            optional_mod: None,
        };
        let result = substitute(&ty, "T", &Type::Number);
        assert_eq!(result, ty); // unchanged
    }

    // -- is_extends tests --

    #[test]
    fn extends_same_type() {
        assert_eq!(
            is_extends(&Type::Number, &Type::Number),
            ExtendsResult::True
        );
    }

    #[test]
    fn extends_never_extends_anything() {
        assert_eq!(is_extends(&Type::Never, &Type::String), ExtendsResult::True);
    }

    #[test]
    fn extends_anything_extends_any() {
        assert_eq!(is_extends(&Type::String, &Type::Any), ExtendsResult::True);
    }

    #[test]
    fn extends_literal_extends_base() {
        assert_eq!(
            is_extends(&Type::StringLiteral("hello".into()), &Type::String),
            ExtendsResult::True
        );
        assert_eq!(
            is_extends(&Type::NumberLiteral("42".into()), &Type::Number),
            ExtendsResult::True
        );
    }

    #[test]
    fn extends_primitive_mismatch() {
        assert_eq!(
            is_extends(&Type::String, &Type::Number),
            ExtendsResult::False
        );
    }

    #[test]
    fn extends_union_all_extend() {
        let check = Type::Union(
            vec![
                Type::StringLiteral("a".into()),
                Type::StringLiteral("b".into()),
            ]
            .into(),
        );
        assert_eq!(is_extends(&check, &Type::String), ExtendsResult::True);
    }

    #[test]
    fn extends_union_partial_fail() {
        let check = Type::Union(vec![Type::String, Type::Number].into());
        assert_eq!(is_extends(&check, &Type::String), ExtendsResult::False);
    }

    #[test]
    fn extends_type_param_unknown() {
        assert_eq!(
            is_extends(&Type::TypeParameter("T".into()), &Type::String),
            ExtendsResult::Unknown
        );
    }

    // -- expand_conditional tests --

    #[test]
    fn conditional_true_branch() {
        let result = expand_conditional(
            &Type::String,
            &Type::String,
            &Type::BooleanLiteral(true),
            &Type::BooleanLiteral(false),
            &HashMap::new(),
        );
        assert_eq!(result, Type::BooleanLiteral(true));
    }

    #[test]
    fn conditional_false_branch() {
        let result = expand_conditional(
            &Type::Number,
            &Type::String,
            &Type::BooleanLiteral(true),
            &Type::BooleanLiteral(false),
            &HashMap::new(),
        );
        assert_eq!(result, Type::BooleanLiteral(false));
    }

    #[test]
    fn conditional_distributes_over_union() {
        // Exclude<string | number, string> should give number
        let result = expand_conditional(
            &Type::Union(vec![Type::String, Type::Number].into()),
            &Type::String,
            &Type::Never,                     // true branch: never (excluded)
            &Type::TypeParameter("T".into()), // placeholder, but check type is substituted
            &HashMap::new(),
        );
        // string extends string -> Never, number !extends string -> TypeParameter("T")
        // But since we don't substitute the check type into branches, let's check
        // that distribution works: we get simplify_union([Never, TypeParameter("T")])
        // = TypeParameter("T")
        assert_eq!(result, Type::TypeParameter("T".into()));
    }

    // -- expand_mapped tests --

    #[test]
    fn mapped_simple_object() {
        // { [P in "a" | "b"]: number } -> { a: number, b: number }
        let result = expand_mapped(
            "P",
            &Type::Union(
                vec![
                    Type::StringLiteral("a".into()),
                    Type::StringLiteral("b".into()),
                ]
                .into(),
            ),
            Some(&Type::Number),
            None,
            None,
            None,
            &HashMap::new(),
        );
        match &result {
            Type::ObjectType(obj) => {
                assert_eq!(obj.properties.len(), 2);
                assert_eq!(obj.properties[0].0, "a");
                assert_eq!(*obj.properties[0].1, Type::Number);
                assert_eq!(obj.properties[1].0, "b");
                assert_eq!(*obj.properties[1].1, Type::Number);
            }
            _ => panic!("Expected ObjectType, got {:?}", result),
        }
    }

    #[test]
    fn mapped_unresolvable_returns_mapped() {
        let result = expand_mapped(
            "P",
            &Type::TypeParameter("T".into()),
            Some(&Type::Number),
            None,
            None,
            None,
            &HashMap::new(),
        );
        assert!(matches!(result, Type::Mapped { .. }));
    }

    // -- as-clause key-remapping + optional modifier (zod $InferObjectOutput) --

    #[test]
    fn mapped_as_clause_drops_never_keys() {
        // { [K in "a" | "b" as K extends "a" ? never : K]: number } -> { b: number }
        use crate::type_computations::evaluate_mapped_type;
        let constraint = Type::Union(
            vec![
                Type::StringLiteral("a".into()),
                Type::StringLiteral("b".into()),
            ]
            .into(),
        );
        let name_type = Type::Conditional {
            check: Arc::new(Type::TypeParameter("K".into())),
            extends: Arc::new(Type::StringLiteral("a".into())),
            true_type: Arc::new(Type::Never),
            false_type: Arc::new(Type::TypeParameter("K".into())),
        };
        let result = evaluate_mapped_type(
            "K",
            &constraint,
            Some(&Type::Number),
            Some(&name_type),
            None,
            None,
            &HashMap::new(),
        );
        match &result {
            Type::ObjectType(obj) => {
                assert_eq!(obj.properties.len(), 1, "only b survives: {:?}", obj);
                assert_eq!(obj.properties[0].0, "b");
            }
            _ => panic!("expected ObjectType, got {:?}", result),
        }
    }

    #[test]
    fn mapped_optional_modifier_wraps_values() {
        // { [K in "a" as K]?: number } -> { a?: number } (Optional value)
        use crate::type_computations::evaluate_mapped_type;
        let result = evaluate_mapped_type(
            "K",
            &Type::StringLiteral("a".into()),
            Some(&Type::Number),
            None,
            None,
            Some(MappedModifierKind::Add),
            &HashMap::new(),
        );
        match &result {
            Type::ObjectType(obj) => {
                assert_eq!(obj.properties.len(), 1);
                assert!(
                    matches!(obj.properties[0].1.as_ref(), Type::Optional(_)),
                    "value should be Optional: {:?}",
                    obj.properties[0].1
                );
            }
            _ => panic!("expected ObjectType, got {:?}", result),
        }
    }

    // -- resolve_keyof tests --

    #[test]
    fn keyof_object_type() {
        let obj = Type::ObjectType(ObjectTypeInfo {
            properties: vec![
                ("x".into(), Arc::new(Type::Number)),
                ("y".into(), Arc::new(Type::String)),
            ],
            call_signatures: vec![],
            construct_signatures: vec![],
            index_signature: None,
            index_signature_name: None,
            method_names: Vec::new(),
        });
        let result = resolve_keyof(&obj);
        assert_eq!(
            result,
            Type::Union(
                vec![
                    Type::StringLiteral("x".into()),
                    Type::StringLiteral("y".into()),
                ]
                .into()
            )
        );
    }

    #[test]
    fn keyof_never() {
        assert_eq!(resolve_keyof(&Type::Never), Type::Never);
    }

    // -- resolve_indexed_access tests --

    #[test]
    fn indexed_access_object_property() {
        let obj = Type::ObjectType(ObjectTypeInfo {
            properties: vec![
                ("name".into(), Arc::new(Type::String)),
                ("age".into(), Arc::new(Type::Number)),
            ],
            call_signatures: vec![],
            construct_signatures: vec![],
            index_signature: None,
            index_signature_name: None,
            method_names: Vec::new(),
        });
        let result = resolve_indexed_access(&obj, &Type::StringLiteral("name".into()));
        assert_eq!(result, Type::String);
    }

    #[test]
    fn indexed_access_array_number() {
        let arr = Type::Array(Arc::new(Type::String));
        let result = resolve_indexed_access(&arr, &Type::Number);
        assert_eq!(result, Type::String);
    }

    #[test]
    fn indexed_access_tuple() {
        let tuple = Type::Tuple(vec![Type::String, Type::Number, Type::Boolean].into());
        let result = resolve_indexed_access(&tuple, &Type::NumberLiteral("1".into()));
        assert_eq!(result, Type::Number);
    }

    #[test]
    fn indexed_access_union_index() {
        let obj = Type::ObjectType(ObjectTypeInfo {
            properties: vec![
                ("a".into(), Arc::new(Type::String)),
                ("b".into(), Arc::new(Type::Number)),
            ],
            call_signatures: vec![],
            construct_signatures: vec![],
            index_signature: None,
            index_signature_name: None,
            method_names: Vec::new(),
        });
        let idx = Type::Union(
            vec![
                Type::StringLiteral("a".into()),
                Type::StringLiteral("b".into()),
            ]
            .into(),
        );
        let result = resolve_indexed_access(&obj, &idx);
        assert_eq!(result, Type::Union(vec![Type::String, Type::Number].into()));
    }

    // -- simplify_union tests --

    #[test]
    fn union_flatten_and_dedup() {
        let types = vec![
            Type::Union(vec![Type::String, Type::Number].into()),
            Type::String,
            Type::Never,
        ];
        let result = simplify_union(types);
        assert_eq!(result, Type::Union(vec![Type::String, Type::Number].into()));
    }

    #[test]
    fn union_single_element() {
        let result = simplify_union(vec![Type::Never, Type::String, Type::Never]);
        assert_eq!(result, Type::String);
    }

    #[test]
    fn union_all_never() {
        let result = simplify_union(vec![Type::Never, Type::Never]);
        assert_eq!(result, Type::Never);
    }

    // -- simplify_intersection tests --

    #[test]
    fn intersection_never_absorbs() {
        let result = simplify_intersection(vec![Type::String, Type::Never]);
        assert_eq!(result, Type::Never);
    }

    #[test]
    fn intersection_flatten_and_dedup() {
        let types = vec![
            Type::Intersection(vec![Type::String, Type::Number].into()),
            Type::String,
        ];
        let result = simplify_intersection(types);
        assert_eq!(
            result,
            Type::Intersection(vec![Type::String, Type::Number].into())
        );
    }

    #[test]
    fn intersection_unknown_identity() {
        let result = simplify_intersection(vec![Type::String, Type::Unknown]);
        assert_eq!(result, Type::String);
    }
}
