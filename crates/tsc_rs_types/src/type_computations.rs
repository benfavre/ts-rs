//! Type-level computation functions: substitution, conditional types, mapped types, keyof, and indexed access.

use std::sync::Arc;

use super::*;

/// Convenience: check a source file and return results.
pub fn check(file: &SourceFile, symbols: &SymbolTable) -> TypeCheckOutput {
    TypeChecker::new().check(file, symbols)
}

/// Check a source file with compiler options.
pub fn check_with_options(
    file: &SourceFile,
    symbols: &SymbolTable,
    options: &CompilerOptions,
) -> TypeCheckOutput {
    TypeChecker::new().check_with_options(file, symbols, options)
}

// ===========================================================================

// ===========================================================================
// Mapped & conditional type evaluation
// ===========================================================================

/// Substitute type arguments into a type, replacing `TypeParameter(name)` with
/// the corresponding type from `type_args`.
pub fn substitute(ty: &Type, type_args: &HashMap<std::string::String, Type>) -> Type {
    let exceeded = type_op_enter();
    if exceeded {
        return ty.clone();
    }
    let result = substitute_inner(ty, type_args);
    type_op_exit();
    result
}

pub(crate) fn substitute_inner(ty: &Type, type_args: &HashMap<std::string::String, Type>) -> Type {
    match ty {
        Type::TypeParameter(name) => type_args.get(name).cloned().unwrap_or_else(|| ty.clone()),
        Type::Infer(name) => type_args.get(name).cloned().unwrap_or_else(|| ty.clone()),
        Type::Union(members) => {
            let subst: Vec<_> = members.iter().map(|m| substitute(m, type_args)).collect();
            Type::flatten_union(subst)
        }
        Type::Intersection(members) => {
            let subst: Vec<_> = members.iter().map(|m| substitute(m, type_args)).collect();
            Type::Intersection(subst.into())
        }
        Type::Array(elem) => Type::Array(Arc::new(substitute(elem, type_args))),
        Type::Tuple(elems) => Type::Tuple(elems.iter().map(|e| substitute(e, type_args)).collect()),
        Type::Function(ft) => {
            let params = ft
                .params
                .iter()
                .map(|(n, t)| (n.clone(), substitute(t, type_args)))
                .collect();
            let ret = substitute(&ft.return_type, type_args);
            let type_predicate = ft.type_predicate.as_ref().map(|pred| TypePredicate {
                param_name: pred.param_name.clone(),
                target_type: Arc::new(substitute(&pred.target_type, type_args)),
                is_asserts: pred.is_asserts,
            });
            Type::Function(FunctionType {
                type_param_constraints: ft
                    .type_param_constraints
                    .iter()
                    .map(|default| default.as_ref().map(|ty| substitute(ty, type_args)))
                    .collect(),
                params,
                return_type: Arc::new(ret),
                type_params: ft.type_params.clone(),
                type_param_defaults: ft
                    .type_param_defaults
                    .iter()
                    .map(|default| default.as_ref().map(|ty| substitute(ty, type_args)))
                    .collect(),
                type_predicate,
            })
        }
        Type::Constructor(constructor) => {
            let mut inner_args = type_args.clone();
            for parameter in &constructor.type_params {
                inner_args.remove(parameter);
            }
            Type::Constructor(ConstructorType {
                is_abstract: constructor.is_abstract,
                params: constructor
                    .params
                    .iter()
                    .map(|(name, ty)| (name.clone(), substitute(ty, &inner_args)))
                    .collect(),
                return_type: Arc::new(substitute(&constructor.return_type, &inner_args)),
                type_params: constructor.type_params.clone(),
                type_param_constraints: constructor
                    .type_param_constraints
                    .iter()
                    .map(|constraint| constraint.as_ref().map(|ty| substitute(ty, &inner_args)))
                    .collect(),
                type_param_defaults: constructor
                    .type_param_defaults
                    .iter()
                    .map(|default| default.as_ref().map(|ty| substitute(ty, &inner_args)))
                    .collect(),
            })
        }
        Type::ObjectType(info) => {
            let props = info
                .properties
                .iter()
                .map(|(n, t)| (n.clone(), Arc::new(substitute(t, type_args))))
                .collect();
            let call_sigs = info
                .call_signatures
                .iter()
                .map(|cs| FunctionType {
                    type_param_constraints: cs
                        .type_param_constraints
                        .iter()
                        .map(|default| default.as_ref().map(|ty| substitute(ty, type_args)))
                        .collect(),
                    params: cs
                        .params
                        .iter()
                        .map(|(n, t)| (n.clone(), substitute(t, type_args)))
                        .collect(),
                    return_type: Arc::new(substitute(&cs.return_type, type_args)),
                    type_params: cs.type_params.clone(),
                    type_param_defaults: cs
                        .type_param_defaults
                        .iter()
                        .map(|default| default.as_ref().map(|ty| substitute(ty, type_args)))
                        .collect(),
                    type_predicate: cs.type_predicate.as_ref().map(|pred| TypePredicate {
                        param_name: pred.param_name.clone(),
                        target_type: Arc::new(substitute(&pred.target_type, type_args)),
                        is_asserts: pred.is_asserts,
                    }),
                })
                .collect();
            let construct_sigs = info
                .construct_signatures
                .iter()
                .map(
                    |cs| match substitute(&Type::Constructor(cs.clone()), type_args) {
                        Type::Constructor(signature) => signature,
                        _ => unreachable!("constructor substitution changed its kind"),
                    },
                )
                .collect();
            let index_sig = info.index_signature.as_ref().map(|(k, v)| {
                (
                    Arc::new(substitute(k, type_args)),
                    Arc::new(substitute(v, type_args)),
                )
            });
            Type::ObjectType(ObjectTypeInfo {
                properties: props,
                call_signatures: call_sigs,
                construct_signatures: construct_sigs,
                index_signature: index_sig,
                index_signature_name: None,
                method_names: Vec::new(),
            })
        }
        Type::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => Type::Conditional {
            check: Arc::new(substitute(check, type_args)),
            extends: Arc::new(substitute(extends, type_args)),
            true_type: Arc::new(substitute(true_type, type_args)),
            false_type: Arc::new(substitute(false_type, type_args)),
        },
        Type::Mapped {
            param,
            constraint,
            template,
            name_type,
            readonly_mod,
            optional_mod,
        } => Type::Mapped {
            param: param.clone(),
            constraint: Arc::new(substitute(constraint, type_args)),
            template: template
                .as_ref()
                .map(|t| Arc::new(substitute(t, type_args))),
            name_type: name_type
                .as_ref()
                .map(|n| Arc::new(substitute(n, type_args))),
            readonly_mod: *readonly_mod,
            optional_mod: *optional_mod,
        },
        Type::IndexedAccess(obj, idx) => Type::IndexedAccess(
            Arc::new(substitute(obj, type_args)),
            Arc::new(substitute(idx, type_args)),
        ),
        Type::Keyof(inner) => Type::Keyof(Arc::new(substitute(inner, type_args))),
        Type::Optional(inner) => Type::Optional(Arc::new(substitute(inner, type_args))),
        Type::Readonly(inner) => Type::Readonly(Arc::new(substitute(inner, type_args))),
        Type::Rest(inner) => Type::Rest(Arc::new(substitute(inner, type_args))),
        Type::Instance(inner) => Type::Instance(Arc::new(substitute(inner, type_args))),
        Type::TemplateLiteral { quasis, types } => Type::TemplateLiteral {
            quasis: quasis.clone(),
            types: types.iter().map(|t| substitute(t, type_args)).collect(),
        },
        Type::TypeReference(name, args) => {
            // A zero-arg TypeReference whose name matches a type
            // parameter in scope should resolve to the bound type.
            // Used for cross-namespace decls where the resolver hasn't
            // converted `T` into a `TypeParameter` yet (zod's
            // `Mapped { [k in keyof T as ...]: T[k]["_zod"]["output"] }`
            // uses `T` as a zero-arg TypeReference; without this
            // fallback substitute leaves the inner key access pointing
            // at `TypeRef("k", [])` and the mapped folds to `any`.
            if args.is_empty() {
                if let Some(bound) = type_args.get(name.as_str()) {
                    return bound.clone();
                }
            }
            let subst_args: Vec<_> = args.iter().map(|a| substitute(a, type_args)).collect();
            Type::TypeReference(name.clone(), subst_args.into())
        }
        _ => ty.clone(),
    }
}

/// Compute `keyof T`: return a union of string literal types for each property name.
pub fn keyof_type(ty: &Type) -> Type {
    match ty {
        Type::ObjectType(info) => {
            let keys: Vec<Type> = info
                .properties
                .iter()
                .map(|(name, _)| Type::StringLiteral(name.clone()))
                .collect();
            if keys.is_empty() {
                Type::Never
            } else {
                Type::flatten_union(keys)
            }
        }
        Type::Union(members) => {
            // keyof (A | B) = (keyof A) & (keyof B)
            let keyofs: Vec<Type> = members.iter().map(keyof_type).collect();
            if keyofs.len() == 1 {
                keyofs.into_iter().next().unwrap()
            } else {
                Type::Intersection(keyofs.into())
            }
        }
        Type::Intersection(members) => {
            // keyof (A & B) = (keyof A) | (keyof B)
            let keyofs: Vec<Type> = members.iter().map(keyof_type).collect();
            Type::flatten_union(keyofs)
        }
        Type::Any => Type::Union(vec![Type::String, Type::Number, Type::Symbol].into()),
        _ => Type::Never,
    }
}

/// Look up a property type by index type.
pub fn indexed_access_type(object_ty: &Type, index_ty: &Type) -> Type {
    match (object_ty, index_ty) {
        (Type::ObjectType(info), Type::StringLiteral(key)) => info
            .properties
            .iter()
            .find(|(n, _)| n == key)
            .map(|(_, t)| Type::clone(&t))
            .unwrap_or(Type::Error),
        (Type::ObjectType(_), Type::Union(members)) => {
            let results: Vec<Type> = members
                .iter()
                .map(|m| indexed_access_type(object_ty, m))
                .filter(|t| !matches!(t, Type::Error))
                .collect();
            Type::flatten_union(results)
        }
        (Type::Array(elem), Type::Number) | (Type::Array(elem), Type::NumberLiteral(_)) => {
            Type::clone(&elem)
        }
        (Type::Tuple(elems), Type::NumberLiteral(n)) => {
            if let Ok(idx) = n.parse::<usize>() {
                elems.get(idx).cloned().unwrap_or(Type::Error)
            } else {
                Type::Error
            }
        }
        (Type::Tuple(elems), Type::Number) => Type::flatten_union(elems.iter().cloned().collect()),
        (Type::ObjectType(info), Type::String) => {
            if let Some((_, val_ty)) = &info.index_signature {
                Type::clone(&val_ty)
            } else {
                Type::Error
            }
        }
        _ => Type::Error,
    }
}

/// Required (non-optional, non-rest) parameter count of an encoded param list.
fn param_required_count(params: &[(std::string::String, Type)]) -> usize {
    params
        .iter()
        .filter(|(n, _)| !n.starts_with('?') && !n.starts_with("...") && n != "this")
        .count()
}

/// The type accepted at positional slot `index`; a rest parameter covers
/// every remaining slot through its element type.
fn param_slot(params: &[(std::string::String, Type)], index: usize) -> Option<Type> {
    let rest_element = |ty: &Type| match ty {
        Type::Array(elem) => Type::clone(elem),
        other => other.clone(),
    };
    let mut position = 0;
    for (name, ty) in params {
        if name == "this" {
            continue;
        }
        if name.starts_with("...") {
            return match ty {
                Type::Array(elem) => Some(Type::clone(elem)),
                Type::Tuple(elems) => match elems.get(index - position) {
                    Some(Type::Rest(inner)) => Some(rest_element(inner)),
                    Some(other) => Some(other.clone()),
                    None => match elems.last() {
                        Some(Type::Rest(inner)) => Some(rest_element(inner)),
                        _ => None,
                    },
                },
                other => Some(other.clone()),
            };
        }
        if position == index {
            return Some(ty.clone());
        }
        position += 1;
    }
    None
}

pub(crate) fn is_type_assignable(source: &Type, target: &Type) -> bool {
    if source == target {
        return true;
    }
    if matches!(source, Type::Any | Type::Error) || matches!(target, Type::Any | Type::Error) {
        return true;
    }
    if matches!(source, Type::Never) {
        return true;
    }
    if matches!(target, Type::Unknown) {
        return true;
    }
    match (source, target) {
        (Type::NumberLiteral(_), Type::Number) => true,
        (Type::StringLiteral(_), Type::String) => true,
        (Type::BooleanLiteral(_), Type::Boolean) => true,
        (Type::BigIntLiteral(_), Type::BigInt) => true,
        (Type::Null, Type::Undefined) | (Type::Undefined, Type::Null) => false,
        _ => {
            // The lowercase `object` type matches any non-primitive. Needed
            // for `T extends object & { ... }` patterns (lib.es5 Awaited)
            // where intersection-target assignability fails without it.
            if matches!(target, Type::Object) {
                return matches!(
                    source,
                    Type::ObjectType(_)
                        | Type::Function(_)
                        | Type::Constructor(_)
                        | Type::Array(_)
                        | Type::Tuple(_)
                );
            }
            if let Type::Union(members) = target {
                return members.iter().any(|m| is_type_assignable(source, m));
            }
            if let Type::Union(members) = source {
                return members.iter().all(|m| is_type_assignable(m, target));
            }
            // Intersection target: every member must accept.
            if let Type::Intersection(members) = target {
                return members.iter().all(|m| is_type_assignable(source, m));
            }
            // Intersection source: any constituent satisfying the target
            // is enough — an `A & B` value satisfies both.
            if let Type::Intersection(members) = source {
                return members.iter().any(|m| is_type_assignable(m, target));
            }
            if let (Type::ObjectType(src), Type::ObjectType(tgt)) = (source, target) {
                for (name, tgt_ty) in &tgt.properties {
                    if let Some((_, src_ty)) = src.properties.iter().find(|(n, _)| n == name) {
                        if !is_type_assignable(src_ty, tgt_ty) {
                            return false;
                        }
                    } else {
                        return false;
                    }
                }
                return true;
            }
            if let (Type::Function(src_ft), Type::Function(tgt_ft)) = (source, target) {
                // A source requiring more parameters than the target provides
                // is not assignable, unless the target has a rest parameter.
                let tgt_has_rest = tgt_ft.params.iter().any(|(n, _)| n.starts_with("..."));
                let tgt_capacity = tgt_ft.params.iter().filter(|(n, _)| n != "this").count();
                if !tgt_has_rest && param_required_count(&src_ft.params) > tgt_capacity {
                    return false;
                }
                let compared = src_ft.params.len().max(tgt_ft.params.len());
                for index in 0..compared {
                    let Some(sp) = param_slot(&src_ft.params, index) else {
                        continue;
                    };
                    let Some(tp) = param_slot(&tgt_ft.params, index) else {
                        break;
                    };
                    if !is_type_assignable(&tp, &sp) {
                        return false;
                    }
                }
                return is_type_assignable(&src_ft.return_type, &tgt_ft.return_type);
            }
            if let (Type::Constructor(source), Type::Constructor(target)) = (source, target) {
                if source.is_abstract && !target.is_abstract {
                    return false;
                }
                if source.type_params.is_empty() && !target.type_params.is_empty() {
                    return false;
                }
                let normalized;
                let (source, target) = if !source.type_params.is_empty()
                    && !target.type_params.is_empty()
                {
                    if source.type_params.len() != target.type_params.len() {
                        return false;
                    }
                    let canonical = (0..source.type_params.len())
                        .map(|index| Type::TypeParameter(format!("__ctor{index}")))
                        .collect::<Vec<_>>();
                    let source_normalized = crate::alpha_normalize_constructor(source, &canonical);
                    let target_normalized = crate::alpha_normalize_constructor(target, &canonical);
                    for index in 0..canonical.len() {
                        let source_constraint = source_normalized
                            .type_param_constraints
                            .get(index)
                            .and_then(|constraint| constraint.as_ref())
                            .cloned()
                            .unwrap_or(Type::Unknown);
                        let target_constraint = target_normalized
                            .type_param_constraints
                            .get(index)
                            .and_then(|constraint| constraint.as_ref())
                            .cloned()
                            .unwrap_or(Type::Unknown);
                        if !is_type_assignable(&target_constraint, &source_constraint) {
                            return false;
                        }
                    }
                    normalized = (source_normalized, target_normalized);
                    (&normalized.0, &normalized.1)
                } else {
                    (source, target)
                };
                let source_required = crate::constructor_required_count(source);
                let target_has_rest = target.params.iter().any(|(n, _)| n.starts_with("..."));
                let target_capacity = target.params.iter().filter(|(n, _)| n != "this").count();
                if !target_has_rest && source_required > target_capacity {
                    return false;
                }
                let compared = crate::constructor_max_count(target)
                    .unwrap_or(target.params.len())
                    .max(crate::constructor_max_count(source).unwrap_or(source.params.len()));
                for index in 0..compared {
                    let Some(source_parameter) = crate::constructor_parameter_at(source, index)
                    else {
                        continue;
                    };
                    let Some(target_parameter) = crate::constructor_parameter_at(target, index)
                    else {
                        break;
                    };
                    if !is_type_assignable(&target_parameter, &source_parameter) {
                        return false;
                    }
                }
                return is_type_assignable(&source.return_type, &target.return_type);
            }
            if let (Type::Array(s), Type::Array(t)) = (source, target) {
                return is_type_assignable(s, t);
            }
            false
        }
    }
}

pub(crate) fn try_infer_from_extends(
    check_type: &Type,
    extends_pattern: &Type,
) -> Option<HashMap<std::string::String, Type>> {
    let mut bindings = HashMap::new();
    if do_infer_match(check_type, extends_pattern, &mut bindings) {
        Some(bindings)
    } else {
        None
    }
}

pub(crate) fn do_infer_match(
    source: &Type,
    pattern: &Type,
    bindings: &mut HashMap<std::string::String, Type>,
) -> bool {
    // Source-side Union: when matching against an infer-bearing pattern,
    // try each union member separately. Succeed and use bindings from
    // the first member that matches. Concrete case: Awaited's inner
    // `F extends ((value: infer V, ...): any) ? ...` where F was
    // bound to `Union(Function, Undefined, Null)` in the outer step.
    // Without this, the Function member never gets a chance to extract V.
    // Skip when the pattern is itself a Union/Intersection — those
    // need to see the full source shape for distributive matching.
    if !matches!(pattern, Type::Union(_) | Type::Intersection(_)) {
        if let Type::Union(src_members) = source {
            for member in src_members.iter() {
                let mut tentative = bindings.clone();
                if do_infer_match(member, pattern, &mut tentative) {
                    *bindings = tentative;
                    return true;
                }
            }
            return false;
        }
    }

    match pattern {
        Type::Infer(name) => {
            bindings.insert(name.clone(), source.clone());
            true
        }
        Type::Function(pat_ft) => {
            if let Type::Function(src_ft) = source {
                if !do_infer_match(&src_ft.return_type, &pat_ft.return_type, bindings) {
                    return false;
                }
                if pat_ft.params.len() == 1 && matches!(pat_ft.params[0].1, Type::Infer(_)) {
                    if let Type::Infer(ref pname) = pat_ft.params[0].1 {
                        let param_types: Vec<Type> = src_ft
                            .params
                            .iter()
                            .map(|(name, ty)| {
                                if name.starts_with('?') {
                                    Type::Optional(Arc::new(ty.clone()))
                                } else if name.starts_with("...") {
                                    Type::Rest(Arc::new(ty.clone()))
                                } else {
                                    ty.clone()
                                }
                            })
                            .collect();
                        bindings.insert(pname.clone(), Type::Tuple(param_types.into()));
                    }
                    return true;
                }
                for ((_, sp), (_, pp)) in src_ft.params.iter().zip(pat_ft.params.iter()) {
                    if !do_infer_match(sp, pp, bindings) {
                        return false;
                    }
                }
                true
            } else {
                false
            }
        }
        Type::Constructor(pattern_constructor) => {
            let Type::Constructor(source_constructor) = source else {
                return false;
            };
            if source_constructor.is_abstract && !pattern_constructor.is_abstract {
                return false;
            }
            if !do_infer_match(
                &source_constructor.return_type,
                &pattern_constructor.return_type,
                bindings,
            ) {
                return false;
            }
            if pattern_constructor.params.len() == 1
                && pattern_constructor.params[0].0.starts_with("...")
                && matches!(pattern_constructor.params[0].1, Type::Infer(_))
            {
                if let Type::Infer(ref name) = pattern_constructor.params[0].1 {
                    bindings.insert(
                        name.clone(),
                        Type::Tuple(
                            source_constructor
                                .params
                                .iter()
                                .map(|(parameter, ty)| {
                                    if parameter.starts_with('?') {
                                        Type::Optional(Arc::new(ty.clone()))
                                    } else if parameter.starts_with("...") {
                                        Type::Rest(Arc::new(ty.clone()))
                                    } else {
                                        ty.clone()
                                    }
                                })
                                .collect(),
                        ),
                    );
                }
                return true;
            }
            for ((_, source_parameter), (_, pattern_parameter)) in source_constructor
                .params
                .iter()
                .zip(pattern_constructor.params.iter())
            {
                if !do_infer_match(source_parameter, pattern_parameter, bindings) {
                    return false;
                }
            }
            true
        }
        Type::Array(pat_elem) => {
            if let Type::Array(src_elem) = source {
                do_infer_match(src_elem, pat_elem, bindings)
            } else {
                false
            }
        }
        Type::ObjectType(pat_info) => {
            if let Type::ObjectType(src_info) = source {
                for (name, pat_ty) in &pat_info.properties {
                    if let Some((_, src_ty)) = src_info.properties.iter().find(|(n, _)| n == name) {
                        if !do_infer_match(src_ty, pat_ty, bindings) {
                            return false;
                        }
                    } else {
                        return false;
                    }
                }
                true
            } else {
                false
            }
        }
        // `Promise<infer V>` matching `Promise<{a:number}>` — extract V.
        // Same generic name + same arg count, then recurse per arg.
        // Without this, infer never binds inside generic patterns and
        // the conditional always falls to its FALSE branch (or stays
        // unsimplified). This is the bug that makes TRPC's recursive
        // `Awaited<T>` simplification fail: every step's
        // `R extends Promise<infer V> ? ... : R` couldn't extract V,
        // so the whole chain stays opaque.
        Type::TypeReference(pat_name, pat_args) => {
            if let Type::TypeReference(src_name, src_args) = source {
                if src_name != pat_name || src_args.len() != pat_args.len() {
                    return false;
                }
                for (sp, pp) in src_args.iter().zip(pat_args.iter()) {
                    if !do_infer_match(sp, pp, bindings) {
                        return false;
                    }
                }
                true
            } else {
                false
            }
        }
        Type::Tuple(pat_elems) => {
            if let Type::Tuple(src_elems) = source {
                if src_elems.len() != pat_elems.len() {
                    return false;
                }
                for (sp, pp) in src_elems.iter().zip(pat_elems.iter()) {
                    if !do_infer_match(sp, pp, bindings) {
                        return false;
                    }
                }
                true
            } else {
                false
            }
        }
        // Intersection pattern: every member must match the source. Needed
        // for `T extends object & { then(infer F, ...): any }` from
        // lib.es5 `Awaited<T>`. Without this, infer F never binds because
        // the matcher falls back to `is_type_assignable` which can't
        // decompose the intersection.
        Type::Intersection(pat_members) => {
            for pat_member in pat_members.iter() {
                if !do_infer_match(source, pat_member, bindings) {
                    return false;
                }
            }
            true
        }
        _ => is_type_assignable(source, pattern),
    }
}

pub(crate) fn contains_infer(ty: &Type) -> bool {
    match ty {
        Type::Infer(_) => true,
        Type::Function(ft) => {
            ft.params.iter().any(|(_, t)| contains_infer(t)) || contains_infer(&ft.return_type)
        }
        Type::Constructor(constructor) => {
            constructor.params.iter().any(|(_, ty)| contains_infer(ty))
                || contains_infer(&constructor.return_type)
        }
        Type::Array(elem) => contains_infer(elem),
        Type::Tuple(elems) => elems.iter().any(contains_infer),
        Type::Union(members) | Type::Intersection(members) => members.iter().any(contains_infer),
        Type::ObjectType(info) => {
            info.properties.iter().any(|(_, t)| contains_infer(t))
                || info.call_signatures.iter().any(|signature| {
                    signature.params.iter().any(|(_, ty)| contains_infer(ty))
                        || contains_infer(&signature.return_type)
                })
                || info.construct_signatures.iter().any(|signature| {
                    signature.params.iter().any(|(_, ty)| contains_infer(ty))
                        || contains_infer(&signature.return_type)
                })
        }
        // `Promise<infer V>` / `Array<infer T>` etc. — without recursing
        // into generic args the conditional evaluator missed every
        // generic-extraction pattern and fell to the false branch. This
        // is the root cause of TRPC's `Awaited<T>` chain (and any other
        // `T extends SomeGeneric<infer X> ? X : ...`) staying opaque.
        Type::TypeReference(_, args) => args.iter().any(contains_infer),
        Type::IndexedAccess(obj, idx) => contains_infer(obj) || contains_infer(idx),
        Type::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            contains_infer(check)
                || contains_infer(extends)
                || contains_infer(true_type)
                || contains_infer(false_type)
        }
        _ => false,
    }
}

/// Evaluate a conditional type with substitution.
pub fn evaluate_conditional_type(
    check: &Type,
    extends: &Type,
    true_type: &Type,
    false_type: &Type,
    type_args: &HashMap<std::string::String, Type>,
) -> Type {
    let exceeded = type_op_enter();
    if exceeded {
        return Type::Any;
    }
    let result = evaluate_conditional_type_inner(check, extends, true_type, false_type, type_args);
    type_op_exit();
    result
}

pub(crate) fn evaluate_conditional_type_inner(
    check: &Type,
    extends: &Type,
    true_type: &Type,
    false_type: &Type,
    type_args: &HashMap<std::string::String, Type>,
) -> Type {
    // If check is a type parameter, we may need to distribute over a union.
    // We need the original type parameter name for per-member substitution.
    let check_param_name = match check {
        Type::TypeParameter(name) => Some(name.clone()),
        _ => None,
    };

    let check_subst = substitute(check, type_args);
    let extends_subst = substitute(extends, type_args);

    if matches!(check_subst, Type::TypeParameter(_)) {
        let true_subst = substitute(true_type, type_args);
        let false_subst = substitute(false_type, type_args);
        return Type::Conditional {
            check: Arc::new(check_subst),
            extends: Arc::new(extends_subst),
            true_type: Arc::new(true_subst),
            false_type: Arc::new(false_subst),
        };
    }

    // Distribution: if check was a type parameter that resolved to a union,
    // distribute the conditional over each member, substituting the original
    // type parameter name with each member in true/false branches.
    if let (Type::Union(members), Some(ref param_name)) = (&check_subst, &check_param_name) {
        let results: Vec<Type> = members
            .iter()
            .map(|member| {
                let mut per_member_args = type_args.clone();
                per_member_args.insert(param_name.clone(), member.clone());
                let ext = substitute(extends, &per_member_args);
                let tt = substitute(true_type, &per_member_args);
                let ft = substitute(false_type, &per_member_args);
                eval_single_conditional(member, &ext, &tt, &ft)
            })
            .collect();
        return Type::flatten_union(results);
    }

    let true_subst = substitute(true_type, type_args);
    let false_subst = substitute(false_type, type_args);
    eval_single_conditional(&check_subst, &extends_subst, &true_subst, &false_subst)
}

pub(crate) fn eval_single_conditional(
    check: &Type,
    extends: &Type,
    true_type: &Type,
    false_type: &Type,
) -> Type {
    if matches!(check, Type::Never) {
        return Type::Never;
    }
    if contains_infer(extends) {
        if let Some(infer_bindings) = try_infer_from_extends(check, extends) {
            substitute(true_type, &infer_bindings)
        } else {
            substitute(false_type, &HashMap::new())
        }
    } else if is_type_assignable(check, extends) {
        true_type.clone()
    } else if contains_value_query(check) || contains_value_query(extends) {
        // A `typeof value` the static evaluator cannot expand: "not
        // assignable" is not a decision, so keep the conditional deferred.
        Type::Conditional {
            check: Arc::new(check.clone()),
            extends: Arc::new(extends.clone()),
            true_type: Arc::new(true_type.clone()),
            false_type: Arc::new(false_type.clone()),
        }
    } else {
        false_type.clone()
    }
}

/// Whether `ty` still contains an unexpanded `typeof value` query.
fn contains_value_query(ty: &Type) -> bool {
    match ty {
        Type::Typeof(_) => true,
        Type::Union(members) | Type::Intersection(members) => {
            members.iter().any(contains_value_query)
        }
        Type::TypeReference(_, args) => args.iter().any(contains_value_query),
        Type::Array(inner) => contains_value_query(inner),
        Type::Tuple(elements) => elements.iter().any(contains_value_query),
        Type::Keyof(inner) => contains_value_query(inner),
        Type::IndexedAccess(object, index) => {
            contains_value_query(object) || contains_value_query(index)
        }
        _ => false,
    }
}

/// Evaluate a mapped type with concrete type arguments.
#[allow(clippy::too_many_arguments)]
pub fn evaluate_mapped_type(
    param: &str,
    constraint: &Type,
    template: Option<&Type>,
    name_type: Option<&Type>,
    readonly_mod: Option<MappedModifierKind>,
    optional_mod: Option<MappedModifierKind>,
    type_args: &HashMap<std::string::String, Type>,
) -> Type {
    let exceeded = type_op_enter();
    if exceeded {
        return Type::Any;
    }
    let result = evaluate_mapped_type_inner(
        param,
        constraint,
        template,
        name_type,
        readonly_mod,
        optional_mod,
        type_args,
    );
    type_op_exit();
    result
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn evaluate_mapped_type_inner(
    param: &str,
    constraint: &Type,
    template: Option<&Type>,
    name_type: Option<&Type>,
    readonly_mod: Option<MappedModifierKind>,
    optional_mod: Option<MappedModifierKind>,
    type_args: &HashMap<std::string::String, Type>,
) -> Type {
    let constraint_subst = substitute(constraint, type_args);
    let keys = resolve_constraint_keys(&constraint_subst, type_args);

    let mut properties = Vec::new();
    for key in &keys {
        let key_name = match key {
            Type::StringLiteral(s) => s.clone(),
            _ => continue,
        };

        let mut local_args = type_args.clone();
        local_args.insert(param.to_string(), key.clone());

        // The `as` key-remapper: evaluate `NameType` with the key bound.
        // A key whose remapped name is `never` is DROPPED; a string-literal
        // result RENAMES the key; anything else keeps the original name.
        let out_name = if let Some(nt) = name_type {
            let remapped = resolve_type_level_computation(&substitute(nt, &local_args), type_args);
            match remapped {
                Type::Never => continue,
                Type::StringLiteral(s) => s,
                _ => key_name.clone(),
            }
        } else {
            key_name.clone()
        };

        let prop_ty = if let Some(tmpl) = template {
            let subst = substitute(tmpl, &local_args);
            resolve_type_level_computation(&subst, type_args)
        } else {
            Type::Any
        };

        // Optional modifier: `+?`/`?` wraps the value as Optional (the
        // property may be absent); `-?` strips an Optional/undefined so the
        // property becomes required. When there's no modifier the value's
        // own Optional-ness (e.g. a homomorphic `[K in keyof T]: T[K]` over
        // an object with optional props) is preserved by resolve/indexed
        // access, so we leave it alone.
        let prop_ty = match optional_mod {
            Some(MappedModifierKind::Add) => match &prop_ty {
                Type::Optional(_) => prop_ty,
                _ => Type::Optional(Arc::new(prop_ty)),
            },
            Some(MappedModifierKind::Remove) => match prop_ty {
                Type::Optional(inner) => Type::clone(&inner),
                Type::Union(members) => {
                    let filtered: Vec<Type> = members
                        .iter()
                        .filter(|m| !matches!(m, Type::Undefined))
                        .cloned()
                        .collect();
                    Type::flatten_union(filtered)
                }
                other => other,
            },
            None => prop_ty,
        };

        let _ = readonly_mod;

        properties.push((out_name, Arc::new(prop_ty)));
    }

    Type::ObjectType(ObjectTypeInfo {
        properties,
        call_signatures: Vec::new(),
        construct_signatures: Vec::new(),
        index_signature: None,
        index_signature_name: None,
        method_names: Vec::new(),
    })
}

pub(crate) fn resolve_constraint_keys(
    constraint: &Type,
    type_args: &HashMap<std::string::String, Type>,
) -> Vec<Type> {
    match constraint {
        Type::Union(members) => members.iter().cloned().collect(),
        Type::StringLiteral(_) => vec![constraint.clone()],
        Type::Keyof(inner) => {
            let resolved = substitute(inner, type_args);
            let resolved = resolve_type_level_computation(&resolved, type_args);
            match keyof_type(&resolved) {
                Type::Union(members) => members.iter().cloned().collect(),
                single @ Type::StringLiteral(_) => vec![single],
                Type::Never => vec![],
                other => vec![other],
            }
        }
        Type::TypeParameter(name) => {
            if let Some(resolved) = type_args.get(name) {
                resolve_constraint_keys(resolved, type_args)
            } else {
                vec![]
            }
        }
        _ => vec![],
    }
}

/// Resolve type-level computations like IndexedAccess and Keyof.
pub fn resolve_type_level_computation(
    ty: &Type,
    type_args: &HashMap<std::string::String, Type>,
) -> Type {
    let exceeded = type_op_enter();
    if exceeded {
        return Type::Any;
    }
    let result = resolve_type_level_computation_inner(ty, type_args);
    type_op_exit();
    result
}

pub(crate) fn resolve_type_level_computation_inner(
    ty: &Type,
    type_args: &HashMap<std::string::String, Type>,
) -> Type {
    match ty {
        Type::IndexedAccess(obj, idx) => {
            let obj_resolved = resolve_type_level_computation(obj, type_args);
            let idx_resolved = resolve_type_level_computation(idx, type_args);
            indexed_access_type(&obj_resolved, &idx_resolved)
        }
        Type::Keyof(inner) => {
            let resolved = resolve_type_level_computation(inner, type_args);
            keyof_type(&resolved)
        }
        Type::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => evaluate_conditional_type(check, extends, true_type, false_type, type_args),
        Type::Mapped {
            param,
            constraint,
            template,
            name_type,
            readonly_mod,
            optional_mod,
        } => evaluate_mapped_type(
            param,
            constraint,
            template.as_deref(),
            name_type.as_deref(),
            *readonly_mod,
            *optional_mod,
            type_args,
        ),
        Type::TypeParameter(name) => type_args.get(name).cloned().unwrap_or_else(|| ty.clone()),
        Type::Union(members) => {
            let resolved: Vec<_> = members
                .iter()
                .map(|m| resolve_type_level_computation(m, type_args))
                .collect();
            Type::flatten_union(resolved)
        }
        Type::Intersection(members) => {
            let resolved: Vec<_> = members
                .iter()
                .map(|m| resolve_type_level_computation(m, type_args))
                .collect();
            Type::Intersection(resolved.into())
        }
        Type::Constructor(constructor) => {
            let mut inner_args = type_args.clone();
            for parameter in &constructor.type_params {
                inner_args.remove(parameter);
            }
            Type::Constructor(ConstructorType {
                is_abstract: constructor.is_abstract,
                params: constructor
                    .params
                    .iter()
                    .map(|(name, ty)| {
                        (
                            name.clone(),
                            resolve_type_level_computation(ty, &inner_args),
                        )
                    })
                    .collect(),
                return_type: Arc::new(resolve_type_level_computation(
                    &constructor.return_type,
                    &inner_args,
                )),
                type_params: constructor.type_params.clone(),
                type_param_constraints: constructor
                    .type_param_constraints
                    .iter()
                    .map(|constraint| {
                        constraint
                            .as_ref()
                            .map(|ty| resolve_type_level_computation(ty, &inner_args))
                    })
                    .collect(),
                type_param_defaults: constructor
                    .type_param_defaults
                    .iter()
                    .map(|default| {
                        default
                            .as_ref()
                            .map(|ty| resolve_type_level_computation(ty, &inner_args))
                    })
                    .collect(),
            })
        }
        _ => ty.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ObjectTypeInfo;
    use std::sync::Arc;

    #[test]
    fn infer_match_type_reference() {
        // Promise<{a: number}> extends Promise<infer V> → V = {a: number}
        let source = Type::TypeReference(
            "Promise".into(),
            vec![Type::ObjectType(ObjectTypeInfo {
                properties: vec![("a".into(), Arc::new(Type::Number))],
                call_signatures: vec![],
                construct_signatures: vec![],
                index_signature: None,
                index_signature_name: None,
                method_names: Vec::new(),
            })]
            .into(),
        );
        let pattern = Type::TypeReference("Promise".into(), vec![Type::Infer("V".into())].into());
        let bindings = try_infer_from_extends(&source, &pattern)
            .expect("Promise<T> extends Promise<infer V> should match");
        match bindings.get("V") {
            Some(Type::ObjectType(info)) => {
                assert_eq!(info.properties.len(), 1);
                assert_eq!(info.properties[0].0, "a");
            }
            other => panic!("V should bind to ObjectType, got {:?}", other),
        }
    }

    #[test]
    fn infer_match_type_reference_no_match_different_name() {
        // Array<X> doesn't match Promise<infer V>
        let source = Type::TypeReference("Array".into(), vec![Type::String].into());
        let pattern = Type::TypeReference("Promise".into(), vec![Type::Infer("V".into())].into());
        assert!(try_infer_from_extends(&source, &pattern).is_none());
    }

    #[test]
    fn evaluate_conditional_promise_extract() {
        // Conditional: Promise<{a:number}> extends Promise<infer V> ? V : never
        let check = Type::TypeReference(
            "Promise".into(),
            vec![Type::ObjectType(ObjectTypeInfo {
                properties: vec![("a".into(), Arc::new(Type::Number))],
                call_signatures: vec![],
                construct_signatures: vec![],
                index_signature: None,
                index_signature_name: None,
                method_names: Vec::new(),
            })]
            .into(),
        );
        let extends = Type::TypeReference("Promise".into(), vec![Type::Infer("V".into())].into());
        let true_type = Type::TypeParameter("V".into());
        let false_type = Type::Never;
        let result = eval_single_conditional(&check, &extends, &true_type, &false_type);
        match result {
            Type::ObjectType(info) => {
                assert_eq!(info.properties.len(), 1);
                assert_eq!(info.properties[0].0, "a");
            }
            other => panic!("expected ObjectType, got {:?}", other),
        }
    }

    #[test]
    fn infer_constructor_parameter_tuple() {
        let source = Type::Constructor(ConstructorType {
            is_abstract: false,
            params: vec![
                ("value".into(), Type::String),
                ("?count".into(), Type::Number),
            ],
            return_type: Arc::new(Type::Object),
            type_params: Vec::new(),
            type_param_constraints: Vec::new(),
            type_param_defaults: Vec::new(),
        });
        let pattern = Type::Constructor(ConstructorType {
            is_abstract: true,
            params: vec![("...args".into(), Type::Infer("P".into()))],
            return_type: Arc::new(Type::Any),
            type_params: Vec::new(),
            type_param_constraints: Vec::new(),
            type_param_defaults: Vec::new(),
        });
        let bindings = try_infer_from_extends(&source, &pattern)
            .expect("constructor parameter tuple should be inferred");
        assert_eq!(
            bindings.get("P"),
            Some(&Type::Tuple(
                vec![Type::String, Type::Optional(Arc::new(Type::Number)),].into()
            ))
        );
    }
}
