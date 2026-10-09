//! Structural subtyping / assignability checks.
//!
//! This module determines whether a *source* type is assignable to a *target*
//! type following TypeScript's structural type system rules. The entry point is
//! [`is_assignable`] for a simple boolean check or [`assign_with`] for a
//! detailed result with error information.

#![allow(clippy::large_enum_variant)]

use std::sync::Arc;

use std::collections::HashSet;

use crate::Type;

// ---------------------------------------------------------------------------
// Options & errors
// ---------------------------------------------------------------------------

/// Flags that control assignability behaviour.
#[derive(Debug, Clone, Copy, Default)]
pub struct AssignOpts {
    /// When `true`, extra properties on the source object are tolerated.
    pub allow_excess_properties: bool,
    /// When `true`, missing properties on the source object are tolerated.
    pub allow_missing_fields: bool,
    /// Enables cast-like assignability (e.g. `string` assignable to a string
    /// literal target).
    pub for_castability: bool,
    /// When `true`, a concrete type may be assigned to a type parameter.
    pub allow_assignment_to_param: bool,
    /// Require enum identity rather than structural compatibility.
    pub strict_enum: bool,
    /// Enable variance-aware checking for generic types.
    pub check_variance: bool,
}

/// Errors produced by the assignability check.
#[derive(Debug, Clone)]
pub enum AssignError {
    /// The two types are simply not assignable.
    NotAssignable { target: Type, source: Type },
    /// A required property is missing on the source.
    MissingProperty {
        property_name: String,
        target: Type,
        source: Type,
    },
    /// A property exists on both sides but its type is not assignable.
    PropertyMismatch {
        property_name: String,
        inner: Box<AssignError>,
    },
    /// Multiple errors accumulated (e.g. from union / object checking).
    Multiple(Vec<AssignError>),
}

pub type AssignResult = Result<(), AssignError>;

// ---------------------------------------------------------------------------
// Cycle-detection set  (target, source) pairs already being checked.
// ---------------------------------------------------------------------------

type DejaVu<'a> = &'a mut HashSet<(u64, u64)>;

/// Cheap identity hash for cycle detection – we only need a fingerprint, not
/// cryptographic strength.
fn type_fingerprint(ty: &Type) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    ty.hash(&mut h);
    h.finish()
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Returns `true` when `source` is assignable to `target` under the default
/// options.
pub fn is_assignable(target: &Type, source: &Type) -> bool {
    let mut dejavu = HashSet::new();
    assign_with(target, source, AssignOpts::default(), &mut dejavu).is_ok()
}

/// If `source` fails to assign to `target` *solely* because it is missing one
/// or more required properties of `target` (no property-type mismatches, index
/// signature, or call/construct-signature incompatibilities), return those
/// property names in target order. Returns `None` when the types are assignable
/// or the failure has any other cause — the caller should then use the generic
/// TS2322 "not assignable" error. Runs under default options, matching the
/// boolean [`is_assignable`] the diagnostic sites gate on.
pub fn pure_missing_properties(target: &Type, source: &Type) -> Option<Vec<String>> {
    let mut dejavu = HashSet::new();
    let err = assign_with(target, source, AssignOpts::default(), &mut dejavu).err()?;
    let mut names = Vec::new();
    if collect_missing_only(&err, &mut names) && !names.is_empty() {
        Some(names)
    } else {
        None
    }
}

fn collect_missing_only(err: &AssignError, out: &mut Vec<String>) -> bool {
    match err {
        AssignError::MissingProperty { property_name, .. } => {
            out.push(property_name.clone());
            true
        }
        AssignError::Multiple(errs) => errs.iter().all(|e| collect_missing_only(e, out)),
        // NotAssignable or PropertyMismatch → not a pure missing-property failure.
        _ => false,
    }
}

/// Full assignability check with options and cycle detection.
pub fn assign_with(
    target: &Type,
    source: &Type,
    opts: AssignOpts,
    dejavu: DejaVu<'_>,
) -> AssignResult {
    // Cycle detection: if we are already checking this pair, assume OK.
    let pair = (type_fingerprint(target), type_fingerprint(source));
    if !dejavu.insert(pair) {
        return Ok(());
    }
    let res = assign_inner(target, source, opts, dejavu);
    dejavu.remove(&pair);
    res
}

// ---------------------------------------------------------------------------
// Core dispatcher
// ---------------------------------------------------------------------------

/// Primitive (non-object) source types.
pub(crate) fn is_primitive_source(ty: &Type) -> bool {
    matches!(
        ty,
        Type::String
            | Type::Number
            | Type::Boolean
            | Type::BigInt
            | Type::Symbol
            | Type::UniqueSymbol(_)
            | Type::StringLiteral(_)
            | Type::NumberLiteral(_)
            | Type::BooleanLiteral(_)
            | Type::BigIntLiteral(_)
            | Type::TemplateLiteral { .. }
    )
}

/// An object type whose every member is optional (tsc "weak type").
pub(crate) fn is_weak_object(ty: &Type) -> bool {
    match ty {
        Type::ObjectType(info) => {
            !info.properties.is_empty()
                && info.call_signatures.is_empty()
                && info.construct_signatures.is_empty()
                && info.index_signature.is_none()
                && info
                    .properties
                    .iter()
                    .all(|(_, property)| matches!(property.as_ref(), Type::Optional(_)))
        }
        _ => false,
    }
}

fn assign_inner(
    target: &Type,
    source: &Type,
    opts: AssignOpts,
    dejavu: DejaVu<'_>,
) -> AssignResult {
    // ---- Trivial / identical --------------------------------------------
    if target == source {
        return Ok(());
    }

    // ---- Target-side special cases --------------------------------------

    // `any` and `unknown` accept everything.
    match target {
        Type::Any | Type::Unknown => return Ok(()),
        Type::Error => return Ok(()),
        _ => {}
    }

    // `never` target only accepts `never`.
    if matches!(target, Type::Never) {
        return if matches!(source, Type::Never) {
            Ok(())
        } else {
            Err(not_assignable(target, source))
        };
    }

    // ---- Source-side special cases ---------------------------------------

    // `any` source is assignable to everything.
    if matches!(source, Type::Any) {
        return Ok(());
    }

    // `never` source is assignable to everything.
    if matches!(source, Type::Never) {
        return Ok(());
    }

    // `error` source (unresolved) – be lenient.
    if matches!(source, Type::Error) {
        return Ok(());
    }

    // ---- Wrapper unwrapping (before structural checks) ------------------

    // Readonly is transparent for assignability.
    if let Type::Readonly(inner) = target {
        return assign_with(inner, source, opts, dejavu);
    }
    if let Type::Readonly(inner) = source {
        return assign_with(target, inner, opts, dejavu);
    }

    // Optional target accepts `undefined` and the inner type.
    if let Type::Optional(inner) = target {
        if matches!(source, Type::Undefined) {
            return Ok(());
        }
        return assign_with(inner, source, opts, dejavu);
    }
    if let Type::Optional(inner) = source {
        return assign_with(target, inner, opts, dejavu);
    }

    // Rest wrapper.
    if let Type::Rest(inner) = target {
        if let Type::Array(elem) = inner.as_ref() {
            return assign_with(elem, source, opts, dejavu);
        }
        return assign_with(inner, source, opts, dejavu);
    }
    if let Type::Rest(inner) = source {
        if let Type::Array(elem) = inner.as_ref() {
            return assign_with(target, elem, opts, dejavu);
        }
        return assign_with(target, inner, opts, dejavu);
    }

    // Instance wrapper.
    if let Type::Instance(inner) = target {
        return assign_with(inner, source, opts, dejavu);
    }
    if let Type::Instance(inner) = source {
        return assign_with(target, inner, opts, dejavu);
    }

    // Conditional source: both branches must be assignable.
    if let Type::Conditional {
        true_type,
        false_type,
        ..
    } = source
    {
        let r1 = assign_with(target, true_type, opts, dejavu);
        let r2 = assign_with(target, false_type, opts, dejavu);
        return match (r1, r2) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(e), _) | (_, Err(e)) => Err(e),
        };
    }

    // Conditional target: source must be assignable to at least one branch.
    if let Type::Conditional {
        true_type,
        false_type,
        ..
    } = target
    {
        let r1 = assign_with(true_type, source, opts, dejavu);
        let r2 = assign_with(false_type, source, opts, dejavu);
        if r1.is_ok() || r2.is_ok() {
            return Ok(());
        }
        return Err(not_assignable(target, source));
    }

    // ---- Void / Undefined / Null ----------------------------------------

    // `void` target accepts `void` and `undefined`.
    if matches!(target, Type::Void) {
        return if matches!(source, Type::Void | Type::Undefined) {
            Ok(())
        } else {
            Err(not_assignable(target, source))
        };
    }

    // `undefined` target accepts `undefined` and `void`.
    if matches!(target, Type::Undefined) {
        return if matches!(source, Type::Undefined | Type::Void) {
            Ok(())
        } else {
            Err(not_assignable(target, source))
        };
    }

    // `null` target accepts only `null`.
    if matches!(target, Type::Null) {
        return if matches!(source, Type::Null) {
            Ok(())
        } else {
            Err(not_assignable(target, source))
        };
    }

    // ---- Union target: source must match at least one member ------------
    if let Type::Union(members) = target {
        for member in members.iter() {
            if assign_with(member, source, opts, dejavu).is_ok() {
                return Ok(());
            }
        }
        return Err(not_assignable(target, source));
    }

    // ---- Union source: ALL members must be assignable to target ---------
    if let Type::Union(members) = source {
        let mut errors = Vec::new();
        for member in members.iter() {
            if let Err(e) = assign_with(target, member, opts, dejavu) {
                errors.push(e);
            }
        }
        return if errors.is_empty() {
            Ok(())
        } else {
            Err(AssignError::Multiple(errors))
        };
    }

    // ---- Intersection target: source must match ALL members -------------
    if let Type::Intersection(members) = target {
        let mut errors = Vec::new();
        for member in members.iter() {
            // tsc skips the weak-type check for intersection constituents:
            // a primitive satisfies an all-optional object member
            // (`string & { tag?: never }` accepts a string).
            if is_primitive_source(source) && is_weak_object(member) {
                continue;
            }
            if let Err(e) = assign_with(member, source, opts, dejavu) {
                errors.push(e);
            }
        }
        return if errors.is_empty() {
            Ok(())
        } else {
            Err(AssignError::Multiple(errors))
        };
    }

    // ---- Intersection source: at least one member must match ------------
    if let Type::Intersection(members) = source {
        for member in members.iter() {
            if assign_with(target, member, opts, dejavu).is_ok() {
                return Ok(());
            }
        }
        return Err(not_assignable(target, source));
    }

    // ---- Primitive widening ---------------------------------------------

    // String accepts StringLiteral and TemplateLiteral.
    if matches!(target, Type::String) {
        match source {
            Type::StringLiteral(_) => return Ok(()),
            Type::TemplateLiteral { .. } => return Ok(()),
            Type::StringMapping { .. } => return Ok(()),
            _ => return Err(not_assignable(target, source)),
        }
    }

    // Number accepts NumberLiteral.
    if matches!(target, Type::Number) {
        return if matches!(source, Type::NumberLiteral(_)) {
            Ok(())
        } else {
            Err(not_assignable(target, source))
        };
    }

    // Boolean accepts BooleanLiteral.
    if matches!(target, Type::Boolean) {
        return if matches!(source, Type::BooleanLiteral(_)) {
            Ok(())
        } else {
            Err(not_assignable(target, source))
        };
    }

    // BigInt accepts BigIntLiteral.
    if matches!(target, Type::BigInt) {
        return if matches!(source, Type::BigIntLiteral(_)) {
            Ok(())
        } else {
            Err(not_assignable(target, source))
        };
    }

    // Symbol accepts UniqueSymbol.
    if matches!(target, Type::Symbol) {
        return if matches!(source, Type::UniqueSymbol(_) | Type::Symbol) {
            Ok(())
        } else {
            Err(not_assignable(target, source))
        };
    }

    // ---- Literal equality -----------------------------------------------

    match (target, source) {
        (Type::StringLiteral(a), Type::StringLiteral(b)) => {
            return if a == b {
                Ok(())
            } else {
                Err(not_assignable(target, source))
            };
        }
        (Type::NumberLiteral(a), Type::NumberLiteral(b)) => {
            return if a == b {
                Ok(())
            } else {
                Err(not_assignable(target, source))
            };
        }
        (Type::BooleanLiteral(a), Type::BooleanLiteral(b)) => {
            return if a == b {
                Ok(())
            } else {
                Err(not_assignable(target, source))
            };
        }
        (Type::BigIntLiteral(a), Type::BigIntLiteral(b)) => {
            return if a == b {
                Ok(())
            } else {
                Err(not_assignable(target, source))
            };
        }
        (Type::UniqueSymbol(a), Type::UniqueSymbol(b)) => {
            return if a == b {
                Ok(())
            } else {
                Err(not_assignable(target, source))
            };
        }
        // For castability: literal target accepts its base type.
        (Type::StringLiteral(_), Type::String) if opts.for_castability => return Ok(()),
        (Type::NumberLiteral(_), Type::Number) if opts.for_castability => return Ok(()),
        (Type::BooleanLiteral(_), Type::Boolean) if opts.for_castability => return Ok(()),
        (Type::BigIntLiteral(_), Type::BigInt) if opts.for_castability => return Ok(()),
        _ => {}
    }

    // ---- Object target accepts `null` and `undefined` only when Any -----
    // (already handled above). Bare `Object` target accepts everything
    // except `null` and `undefined`.
    if matches!(target, Type::Object) {
        return if matches!(source, Type::Null | Type::Undefined | Type::Void) {
            Err(not_assignable(target, source))
        } else {
            Ok(())
        };
    }

    // ---- Function types (contravariant params, covariant return) ---------
    if let Type::Function(target_fn) = target {
        return assign_to_function(target_fn, source, opts, dejavu);
    }

    // ---- Constructor types -----------------------------------------------
    if let Type::Constructor(target_ctor) = target {
        if let Type::Constructor(source_ctor) = source {
            return assign_constructor(target_ctor, source_ctor, opts, dejavu);
        }
        if let Type::ObjectType(source_obj) = source {
            if source_obj.construct_signatures.iter().any(|source_ctor| {
                assign_constructor(target_ctor, source_ctor, opts, dejavu).is_ok()
            }) {
                return Ok(());
            }
        }
        return Err(not_assignable(target, source));
    }

    // ---- Object types (structural property-by-property) -----------------
    if let Type::ObjectType(target_obj) = target {
        if let Type::Constructor(source_ctor) = source {
            let source_object =
                Type::ObjectType(crate::ObjectTypeInfo::new(crate::ObjectTypeData {
                    properties: Vec::new(),
                    call_signatures: Vec::new(),
                    construct_signatures: vec![source_ctor.clone()],
                    index_signature: None,
                    index_signature_name: None,
                    method_names: Vec::new(),
                }));
            return assign_to_object(target_obj, target, &source_object, opts, dejavu);
        }
        return assign_to_object(target_obj, target, source, opts, dejavu);
    }

    // ---- Array types ----------------------------------------------------
    if let Type::Array(target_elem) = target {
        return assign_to_array(target_elem, target, source, opts, dejavu);
    }

    // ---- Tuple types ----------------------------------------------------
    if let Type::Tuple(target_elems) = target {
        return assign_to_tuple(target_elems, target, source, opts, dejavu);
    }

    // ---- TypeReference --------------------------------------------------
    if let Type::TypeReference(t_name, t_args) = target {
        if let Type::TypeReference(s_name, s_args) = source {
            if t_name == s_name && t_args.len() == s_args.len() {
                let mut errors = Vec::new();
                for (ta, sa) in t_args.iter().zip(s_args.iter()) {
                    if let Err(e) = assign_with(ta, sa, opts, dejavu) {
                        errors.push(e);
                    }
                }
                return if errors.is_empty() {
                    Ok(())
                } else {
                    Err(AssignError::Multiple(errors))
                };
            }
        }
        return Err(not_assignable(target, source));
    }

    // ---- TypeParameter --------------------------------------------------
    if let Type::TypeParameter(_) = target {
        return if opts.allow_assignment_to_param {
            Ok(())
        } else if target == source {
            Ok(())
        } else {
            Err(not_assignable(target, source))
        };
    }

    if let Type::TypeParameter(_) = source {
        // A type parameter source is generally not assignable to a concrete
        // target unless the target is `any`/`unknown` (handled above).
        return Err(not_assignable(target, source));
    }

    // ---- Enum types -----------------------------------------------------
    if let Type::EnumType(target_enum) = target {
        match source {
            Type::EnumType(source_enum) => {
                return if target_enum.name == source_enum.name {
                    Ok(())
                } else {
                    Err(not_assignable(target, source))
                };
            }
            Type::EnumVariant { enum_name, .. } => {
                return if target_enum.name == *enum_name {
                    Ok(())
                } else {
                    Err(not_assignable(target, source))
                };
            }
            // A number/string literal might match if not strict_enum.
            _ if !opts.strict_enum => {
                // Try to see if source matches the underlying type of the enum.
                let has_number = target_enum
                    .members
                    .iter()
                    .any(|(_, t)| matches!(t, Type::Number | Type::NumberLiteral(_)));
                let has_string = target_enum
                    .members
                    .iter()
                    .any(|(_, t)| matches!(t, Type::String | Type::StringLiteral(_)));

                match source {
                    Type::Number | Type::NumberLiteral(_) if has_number => return Ok(()),
                    Type::String | Type::StringLiteral(_) if has_string => return Ok(()),
                    _ => {}
                }
                return Err(not_assignable(target, source));
            }
            _ => return Err(not_assignable(target, source)),
        }
    }

    // ---- Enum variant source to Number/String ---------------------------
    if let Type::EnumVariant { value, .. } = source {
        if let Some(val) = value {
            return assign_with(target, val, opts, dejavu);
        }
        // If no value, treat as number (default enum).
        return assign_with(target, &Type::Number, opts, dejavu);
    }

    // ---- TemplateLiteral target -----------------------------------------
    if let Type::TemplateLiteral { .. } = target {
        // TemplateLiteral target accepts string literals and template literals.
        match source {
            Type::StringLiteral(_) | Type::TemplateLiteral { .. } => return Ok(()),
            _ => return Err(not_assignable(target, source)),
        }
    }

    // ---- Import types ---------------------------------------------------
    if let Type::Import {
        name: t_name,
        type_args: t_args,
        ..
    } = target
    {
        if let Type::Import {
            name: s_name,
            type_args: s_args,
            ..
        } = source
        {
            if t_name == s_name && t_args.len() == s_args.len() {
                for (ta, sa) in t_args.iter().zip(s_args.iter()) {
                    assign_with(ta, sa, opts, dejavu)?;
                }
                return Ok(());
            }
        }
        return Err(not_assignable(target, source));
    }

    // ---- Predicate types ------------------------------------------------
    if let Type::Predicate(tp) = target {
        if let Type::Predicate(sp) = source {
            return assign_with(&tp.target_type, &sp.target_type, opts, dejavu);
        }
        // A function returning boolean is assignable to a predicate target
        // in some contexts – but for simplicity we reject here.
        return Err(not_assignable(target, source));
    }

    // ---- Namespace / Module types ----------------------------------------
    if let Type::Namespace(tn) = target {
        if let Type::Namespace(sn) = source {
            return assign_exports(&tn.exports, &sn.exports, target, source, opts, dejavu);
        }
        return Err(not_assignable(target, source));
    }
    if let Type::Module(tm) = target {
        if let Type::Module(sm) = source {
            return assign_exports(&tm.exports, &sm.exports, target, source, opts, dejavu);
        }
        return Err(not_assignable(target, source));
    }

    // ---- Mapped / IndexedAccess / Keyof / Typeof / Infer / This ---------
    // These require type normalization that is beyond the scope of pure
    // structural checking. For now we do structural equality.
    match (target, source) {
        (Type::Keyof(a), Type::Keyof(b)) => return assign_with(a, b, opts, dejavu),
        (Type::Typeof(a), Type::Typeof(b)) => {
            return if a == b {
                Ok(())
            } else {
                Err(not_assignable(target, source))
            };
        }
        (Type::IndexedAccess(to, ti), Type::IndexedAccess(so, si)) => {
            assign_with(to, so, opts, dejavu)?;
            return assign_with(ti, si, opts, dejavu);
        }
        (Type::Infer(a), Type::Infer(b)) => {
            return if a == b {
                Ok(())
            } else {
                Err(not_assignable(target, source))
            };
        }
        (Type::This, Type::This) => return Ok(()),
        (
            Type::StringMapping {
                kind: k1,
                inner: i1,
            },
            Type::StringMapping {
                kind: k2,
                inner: i2,
            },
        ) => {
            return if k1 == k2 {
                assign_with(i1, i2, opts, dejavu)
            } else {
                Err(not_assignable(target, source))
            };
        }
        (
            Type::Mapped {
                param: p1,
                constraint: c1,
                template: t1,
                ..
            },
            Type::Mapped {
                param: p2,
                constraint: c2,
                template: t2,
                ..
            },
        ) => {
            if p1 != p2 {
                return Err(not_assignable(target, source));
            }
            assign_with(c1, c2, opts, dejavu)?;
            match (t1, t2) {
                (Some(a), Some(b)) => return assign_with(a, b, opts, dejavu),
                (None, None) => return Ok(()),
                _ => return Err(not_assignable(target, source)),
            }
        }
        _ => {}
    }

    // ---- Fallthrough: not assignable ------------------------------------
    Err(not_assignable(target, source))
}

// ---------------------------------------------------------------------------
// Helper: function assignability
// ---------------------------------------------------------------------------

fn assign_to_function(
    target_fn: &crate::FunctionType,
    source: &Type,
    opts: AssignOpts,
    dejavu: DejaVu<'_>,
) -> AssignResult {
    let source_fn = match source {
        Type::Function(f) => f,
        // An object with call signatures can be assigned to a function target.
        Type::ObjectType(obj) if !obj.call_signatures.is_empty() => {
            // Try each call signature.
            for sig in &obj.call_signatures {
                if assign_function_types(target_fn, sig, opts, dejavu).is_ok() {
                    return Ok(());
                }
            }
            return Err(not_assignable_types(
                &Type::Function(target_fn.clone()),
                source,
            ));
        }
        _ => {
            return Err(not_assignable_types(
                &Type::Function(target_fn.clone()),
                source,
            ));
        }
    };

    assign_function_types(target_fn, source_fn, opts, dejavu)
}

fn assign_function_types(
    target_fn: &crate::FunctionType,
    source_fn: &crate::FunctionType,
    opts: AssignOpts,
    dejavu: DejaVu<'_>,
) -> AssignResult {
    // Target may have fewer params than source (TypeScript allows this).
    // But source must have at least as many params as target.
    let param_count = target_fn.params.len();

    for i in 0..param_count {
        let (_, ref t_param) = target_fn.params[i];
        if let Some((_, ref s_param)) = source_fn.params.get(i) {
            // Parameters are CONTRAVARIANT: source param must accept target param.
            assign_with(s_param, t_param, opts, dejavu).map_err(|e| {
                AssignError::PropertyMismatch {
                    property_name: format!("parameter {}", i),
                    inner: Box::new(e),
                }
            })?;
        }
        // If source has fewer params, that's fine (callback shortening).
    }

    // Return type is COVARIANT: source return must be assignable to target return.
    assign_with(&target_fn.return_type, &source_fn.return_type, opts, dejavu).map_err(|e| {
        AssignError::PropertyMismatch {
            property_name: "return type".to_string(),
            inner: Box::new(e),
        }
    })?;

    Ok(())
}

fn assign_constructor(
    target_ctor: &crate::ConstructorType,
    source_ctor: &crate::ConstructorType,
    opts: AssignOpts,
    dejavu: DejaVu<'_>,
) -> AssignResult {
    if source_ctor.is_abstract && !target_ctor.is_abstract {
        return Err(not_assignable(
            &Type::Constructor(target_ctor.clone()),
            &Type::Constructor(source_ctor.clone()),
        ));
    }
    if source_ctor.type_params.is_empty() && !target_ctor.type_params.is_empty() {
        return Err(not_assignable(
            &Type::Constructor(target_ctor.clone()),
            &Type::Constructor(source_ctor.clone()),
        ));
    }
    let normalized;
    let (target_ctor, source_ctor) =
        if !source_ctor.type_params.is_empty() && !target_ctor.type_params.is_empty() {
            if source_ctor.type_params.len() != target_ctor.type_params.len() {
                return Err(not_assignable(
                    &Type::Constructor(target_ctor.clone()),
                    &Type::Constructor(source_ctor.clone()),
                ));
            }
            let canonical = (0..source_ctor.type_params.len())
                .map(|index| Type::TypeParameter(format!("__ctor{index}")))
                .collect::<Vec<_>>();
            let source = crate::alpha_normalize_constructor(source_ctor, &canonical);
            let target = crate::alpha_normalize_constructor(target_ctor, &canonical);
            for index in 0..canonical.len() {
                let source_constraint = source
                    .type_param_constraints
                    .get(index)
                    .and_then(|constraint| constraint.as_ref())
                    .cloned()
                    .unwrap_or(Type::Unknown);
                let target_constraint = target
                    .type_param_constraints
                    .get(index)
                    .and_then(|constraint| constraint.as_ref())
                    .cloned()
                    .unwrap_or(Type::Unknown);
                assign_with(&source_constraint, &target_constraint, opts, dejavu)?;
            }
            normalized = (target, source);
            (&normalized.0, &normalized.1)
        } else {
            (target_ctor, source_ctor)
        };
    if crate::constructor_required_count(source_ctor)
        > crate::constructor_required_count(target_ctor)
    {
        return Err(not_assignable(
            &Type::Constructor(target_ctor.clone()),
            &Type::Constructor(source_ctor.clone()),
        ));
    }
    let compared = crate::constructor_max_count(target_ctor)
        .unwrap_or(target_ctor.params.len())
        .max(crate::constructor_max_count(source_ctor).unwrap_or(source_ctor.params.len()));
    for index in 0..compared {
        let Some(source_param) = crate::constructor_parameter_at(source_ctor, index) else {
            continue;
        };
        let Some(target_param) = crate::constructor_parameter_at(target_ctor, index) else {
            break;
        };
        assign_with(&source_param, &target_param, opts, dejavu).map_err(|e| {
            AssignError::PropertyMismatch {
                property_name: format!("constructor parameter {}", index),
                inner: Box::new(e),
            }
        })?;
    }

    assign_with(
        &target_ctor.return_type,
        &source_ctor.return_type,
        opts,
        dejavu,
    )
    .map_err(|e| AssignError::PropertyMismatch {
        property_name: "constructor return type".to_string(),
        inner: Box::new(e),
    })?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Helper: object assignability (structural property-by-property)
// ---------------------------------------------------------------------------

fn assign_to_object(
    target_obj: &crate::ObjectTypeInfo,
    target: &Type,
    source: &Type,
    opts: AssignOpts,
    dejavu: DejaVu<'_>,
) -> AssignResult {
    // An empty object type `{}` accepts almost anything except
    // `null`/`undefined`.
    if target_obj.properties.is_empty()
        && target_obj.call_signatures.is_empty()
        && target_obj.construct_signatures.is_empty()
        && target_obj.index_signature.is_none()
    {
        return if matches!(source, Type::Null | Type::Undefined | Type::Void) {
            Err(not_assignable(target, source))
        } else {
            Ok(())
        };
    }

    let source_obj = match source {
        Type::ObjectType(o) => o,
        // Arrays/Tuples are not directly assignable to an object literal unless
        // the target has an index signature.
        _ => return Err(not_assignable(target, source)),
    };

    let mut errors = Vec::new();

    // Check each target property exists in the source with a compatible type.
    for (prop_name, target_ty) in &target_obj.properties {
        let source_prop = source_obj
            .properties
            .iter()
            .find(|(name, _)| name == prop_name);
        match source_prop {
            Some((_, source_ty)) => {
                if let Err(e) = assign_with(target_ty, source_ty, opts, dejavu) {
                    errors.push(AssignError::PropertyMismatch {
                        property_name: prop_name.clone(),
                        inner: Box::new(e),
                    });
                }
            }
            None => {
                // Optional target properties (`b?: T`, wrapped as
                // `Type::Optional`) need not be present on the source — a
                // missing optional property is not an assignability error.
                let is_optional = matches!(target_ty.as_ref(), Type::Optional(_));
                if !is_optional && !opts.allow_missing_fields {
                    // Check if target has an index signature that might cover it.
                    if target_obj.index_signature.is_none() {
                        errors.push(AssignError::MissingProperty {
                            property_name: prop_name.clone(),
                            target: target.clone(),
                            source: source.clone(),
                        });
                    }
                }
            }
        }
    }

    // Excess property check.
    if !opts.allow_excess_properties {
        for (prop_name, _) in &source_obj.properties {
            let exists_in_target = target_obj
                .properties
                .iter()
                .any(|(name, _)| name == prop_name);
            if !exists_in_target && target_obj.index_signature.is_none() {
                // For now we do not report excess properties as hard errors
                // since TypeScript allows them in many contexts (e.g. variable
                // assignment). They are only errors in fresh object literals.
                // We skip this for compatibility.
            }
        }
    }

    // Check index signatures.
    if let Some((ref t_key, ref t_val)) = target_obj.index_signature {
        if let Some((ref s_key, ref s_val)) = source_obj.index_signature {
            if let Err(e) = assign_with(t_key, s_key, opts, dejavu) {
                errors.push(AssignError::PropertyMismatch {
                    property_name: "[index key]".to_string(),
                    inner: Box::new(e),
                });
            }
            if let Err(e) = assign_with(t_val, s_val, opts, dejavu) {
                errors.push(AssignError::PropertyMismatch {
                    property_name: "[index value]".to_string(),
                    inner: Box::new(e),
                });
            }
        }
    }

    // Check call signatures.
    if !target_obj.call_signatures.is_empty() {
        for t_sig in &target_obj.call_signatures {
            let matched = source_obj
                .call_signatures
                .iter()
                .any(|s_sig| assign_function_types(t_sig, s_sig, opts, dejavu).is_ok());
            if !matched && !source_obj.call_signatures.is_empty() {
                // At least one source signature should match each target signature.
                // (Lenient: we only error if there are source sigs but none match.)
                errors.push(AssignError::NotAssignable {
                    target: target.clone(),
                    source: source.clone(),
                });
                break;
            }
        }
    }

    // Check construct signatures.
    if !target_obj.construct_signatures.is_empty() {
        for t_sig in &target_obj.construct_signatures {
            let matched = source_obj
                .construct_signatures
                .iter()
                .any(|s_sig| assign_constructor(t_sig, s_sig, opts, dejavu).is_ok());
            if !matched {
                errors.push(AssignError::NotAssignable {
                    target: target.clone(),
                    source: source.clone(),
                });
                break;
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(AssignError::Multiple(errors))
    }
}

// ---------------------------------------------------------------------------
// Helper: array assignability
// ---------------------------------------------------------------------------

fn assign_to_array(
    target_elem: &Type,
    target: &Type,
    source: &Type,
    opts: AssignOpts,
    dejavu: DejaVu<'_>,
) -> AssignResult {
    match source {
        Type::Array(source_elem) => assign_with(target_elem, source_elem, opts, dejavu),
        Type::Tuple(source_elems) => {
            // A tuple is assignable to an array if every element type is
            // assignable to the array element type.
            let mut errors = Vec::new();
            for elem in source_elems.iter() {
                let check_ty = match elem {
                    Type::Rest(inner) => match inner.as_ref() {
                        Type::Array(element) => element.as_ref(),
                        other => other,
                    },
                    Type::Optional(inner) => inner.as_ref(),
                    other => other,
                };
                if let Err(e) = assign_with(target_elem, check_ty, opts, dejavu) {
                    errors.push(e);
                }
            }
            if errors.is_empty() {
                Ok(())
            } else {
                Err(AssignError::Multiple(errors))
            }
        }
        _ => Err(not_assignable(target, source)),
    }
}

// ---------------------------------------------------------------------------
// Helper: tuple assignability
// ---------------------------------------------------------------------------

fn assign_to_tuple(
    target_elems: &[Type],
    target: &Type,
    source: &Type,
    opts: AssignOpts,
    dejavu: DejaVu<'_>,
) -> AssignResult {
    match source {
        Type::Tuple(source_elems) => {
            // Check if there is a rest element in target.
            let target_has_rest = target_elems.iter().any(|e| matches!(e, Type::Rest(_)));
            if !target_has_rest && target_elems.len() != source_elems.len() {
                return Err(not_assignable(target, source));
            }

            let mut errors = Vec::new();
            let mut si = 0;
            for te in target_elems.iter() {
                if let Type::Rest(inner) = te {
                    // Rest element consumes remaining source elements.
                    let remaining: Vec<Type> = source_elems[si..].to_vec();
                    if let Type::Array(arr_elem) = inner.as_ref() {
                        for re in &remaining {
                            if let Err(e) = assign_with(arr_elem, re, opts, dejavu) {
                                errors.push(e);
                            }
                        }
                    }
                    si = source_elems.len();
                } else if si < source_elems.len() {
                    if let Err(e) = assign_with(te, &source_elems[si], opts, dejavu) {
                        errors.push(e);
                    }
                    si += 1;
                } else {
                    // Target element with no corresponding source element.
                    // Only ok if target element is optional.
                    if !matches!(te, Type::Optional(_)) {
                        errors.push(not_assignable(target, source));
                    }
                }
            }

            if errors.is_empty() {
                Ok(())
            } else {
                Err(AssignError::Multiple(errors))
            }
        }
        Type::Array(source_elem) => {
            // An ordinary array cannot satisfy a tuple's required positions:
            // its runtime length might be zero. It can satisfy a tuple made
            // exclusively of optional/rest positions.
            if target_elems
                .iter()
                .any(|element| !matches!(element, Type::Optional(_) | Type::Rest(_)))
            {
                return Err(not_assignable(target, source));
            }
            let mut errors = Vec::new();
            for te in target_elems.iter() {
                let check_ty = match te {
                    Type::Rest(inner) => match inner.as_ref() {
                        Type::Array(element) => element.as_ref(),
                        other => other,
                    },
                    Type::Optional(inner) => inner.as_ref(),
                    other => other,
                };
                if let Err(e) = assign_with(check_ty, source_elem, opts, dejavu) {
                    errors.push(e);
                }
            }
            if errors.is_empty() {
                Ok(())
            } else {
                Err(AssignError::Multiple(errors))
            }
        }
        _ => Err(not_assignable(target, source)),
    }
}

// ---------------------------------------------------------------------------
// Helper: namespace / module export assignability
// ---------------------------------------------------------------------------

#[allow(clippy::large_enum_variant)]
fn assign_exports(
    target_exports: &[(String, Type)],
    source_exports: &[(String, Type)],
    target: &Type,
    source: &Type,
    opts: AssignOpts,
    dejavu: DejaVu<'_>,
) -> AssignResult {
    let mut errors = Vec::new();
    for (name, target_ty) in target_exports {
        let found = source_exports.iter().find(|(n, _)| n == name);
        match found {
            Some((_, source_ty)) => {
                if let Err(e) = assign_with(target_ty, source_ty, opts, dejavu) {
                    errors.push(AssignError::PropertyMismatch {
                        property_name: name.clone(),
                        inner: Box::new(e),
                    });
                }
            }
            None => {
                errors.push(AssignError::MissingProperty {
                    property_name: name.clone(),
                    target: target.clone(),
                    source: source.clone(),
                });
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(AssignError::Multiple(errors))
    }
}

// ---------------------------------------------------------------------------
// Error constructors
// ---------------------------------------------------------------------------

fn not_assignable(target: &Type, source: &Type) -> AssignError {
    AssignError::NotAssignable {
        target: target.clone(),
        source: source.clone(),
    }
}

fn not_assignable_types(target: &Type, source: &Type) -> AssignError {
    AssignError::NotAssignable {
        target: target.clone(),
        source: source.clone(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FunctionType, ObjectTypeData, ObjectTypeInfo};

    #[test]
    fn any_accepts_everything() {
        assert!(is_assignable(&Type::Any, &Type::Number));
        assert!(is_assignable(&Type::Any, &Type::String));
        assert!(is_assignable(&Type::Any, &Type::Never));
        assert!(is_assignable(&Type::Any, &Type::Null));
    }

    #[test]
    fn unknown_accepts_everything() {
        assert!(is_assignable(&Type::Unknown, &Type::Number));
        assert!(is_assignable(&Type::Unknown, &Type::Null));
    }

    #[test]
    fn never_is_bottom() {
        assert!(is_assignable(&Type::Number, &Type::Never));
        assert!(is_assignable(&Type::String, &Type::Never));
        assert!(!is_assignable(&Type::Never, &Type::Number));
        assert!(is_assignable(&Type::Never, &Type::Never));
    }

    #[test]
    fn string_accepts_string_literal() {
        assert!(is_assignable(
            &Type::String,
            &Type::StringLiteral("hello".into())
        ));
        assert!(!is_assignable(
            &Type::StringLiteral("hello".into()),
            &Type::String
        ));
    }

    #[test]
    fn number_accepts_number_literal() {
        assert!(is_assignable(
            &Type::Number,
            &Type::NumberLiteral("42".into())
        ));
        assert!(!is_assignable(
            &Type::NumberLiteral("42".into()),
            &Type::Number
        ));
    }

    #[test]
    fn boolean_accepts_boolean_literal() {
        assert!(is_assignable(&Type::Boolean, &Type::BooleanLiteral(true)));
        assert!(is_assignable(&Type::Boolean, &Type::BooleanLiteral(false)));
        assert!(!is_assignable(&Type::BooleanLiteral(true), &Type::Boolean));
    }

    #[test]
    fn literal_equality() {
        assert!(is_assignable(
            &Type::StringLiteral("a".into()),
            &Type::StringLiteral("a".into())
        ));
        assert!(!is_assignable(
            &Type::StringLiteral("a".into()),
            &Type::StringLiteral("b".into())
        ));
    }

    #[test]
    fn union_target() {
        let target = Type::Union(vec![Type::String, Type::Number].into());
        assert!(is_assignable(&target, &Type::String));
        assert!(is_assignable(&target, &Type::Number));
        assert!(is_assignable(&target, &Type::StringLiteral("x".into())));
        assert!(!is_assignable(&target, &Type::Boolean));
    }

    #[test]
    fn union_source() {
        // All members of source union must be assignable to target.
        let source = Type::Union(
            vec![
                Type::StringLiteral("a".into()),
                Type::StringLiteral("b".into()),
            ]
            .into(),
        );
        assert!(is_assignable(&Type::String, &source));
        assert!(!is_assignable(&Type::Number, &source));
    }

    #[test]
    fn intersection_target() {
        // Source must be assignable to ALL members of target intersection.
        let obj1 = Type::ObjectType(ObjectTypeInfo::new(ObjectTypeData {
            properties: vec![("x".into(), Arc::new(Type::Number))],
            call_signatures: vec![],
            construct_signatures: vec![],
            index_signature: None,
            index_signature_name: None,
            method_names: Vec::new(),
        }));
        let obj2 = Type::ObjectType(ObjectTypeInfo::new(ObjectTypeData {
            properties: vec![("y".into(), Arc::new(Type::String))],
            call_signatures: vec![],
            construct_signatures: vec![],
            index_signature: None,
            index_signature_name: None,
            method_names: Vec::new(),
        }));
        let target = Type::Intersection(vec![obj1.clone(), obj2.clone()].into());
        let source = Type::ObjectType(ObjectTypeInfo::new(ObjectTypeData {
            properties: vec![
                ("x".into(), Arc::new(Type::Number)),
                ("y".into(), Arc::new(Type::String)),
            ],
            call_signatures: vec![],
            construct_signatures: vec![],
            index_signature: None,
            index_signature_name: None,
            method_names: Vec::new(),
        }));
        assert!(is_assignable(&target, &source));
    }

    #[test]
    fn array_assignability() {
        let t = Type::Array(Arc::new(Type::Number));
        let s = Type::Array(Arc::new(Type::Number));
        assert!(is_assignable(&t, &s));

        let s2 = Type::Array(Arc::new(Type::String));
        assert!(!is_assignable(&t, &s2));
    }

    #[test]
    fn tuple_assignability() {
        let t = Type::Tuple(vec![Type::Number, Type::String].into());
        let s = Type::Tuple(vec![Type::Number, Type::String].into());
        assert!(is_assignable(&t, &s));

        let s2 = Type::Tuple(vec![Type::Number].into());
        assert!(!is_assignable(&t, &s2));
    }

    #[test]
    fn tuple_to_array() {
        let t = Type::Array(Arc::new(Type::Number));
        let s = Type::Tuple(
            vec![
                Type::NumberLiteral("1".into()),
                Type::NumberLiteral("2".into()),
            ]
            .into(),
        );
        assert!(is_assignable(&t, &s));
    }

    #[test]
    fn function_covariant_return() {
        let target = Type::Function(FunctionType {
            params: vec![],
            return_type: Arc::new(Type::String),
            type_params: vec![],
            type_param_defaults: Vec::new(),
            type_param_constraints: Vec::new(),
            type_predicate: None,
        });
        let source = Type::Function(FunctionType {
            params: vec![],
            return_type: Arc::new(Type::StringLiteral("x".into())),
            type_params: vec![],
            type_param_defaults: Vec::new(),
            type_param_constraints: Vec::new(),
            type_predicate: None,
        });
        assert!(is_assignable(&target, &source));
    }

    #[test]
    fn function_contravariant_params() {
        // target: (x: string) => void
        // source: (x: any) => void    -- any is wider, so it's fine
        let target = Type::Function(FunctionType {
            params: vec![("x".into(), Type::String)],
            return_type: Arc::new(Type::Void),
            type_params: vec![],
            type_param_defaults: Vec::new(),
            type_param_constraints: Vec::new(),
            type_predicate: None,
        });
        let source = Type::Function(FunctionType {
            params: vec![("x".into(), Type::Any)],
            return_type: Arc::new(Type::Void),
            type_params: vec![],
            type_param_defaults: Vec::new(),
            type_param_constraints: Vec::new(),
            type_predicate: None,
        });
        assert!(is_assignable(&target, &source));
    }

    #[test]
    fn function_fewer_params_ok() {
        // TypeScript allows callbacks with fewer params.
        let target = Type::Function(FunctionType {
            params: vec![("a".into(), Type::String), ("b".into(), Type::Number)],
            return_type: Arc::new(Type::Void),
            type_params: vec![],
            type_param_defaults: Vec::new(),
            type_param_constraints: Vec::new(),
            type_predicate: None,
        });
        let source = Type::Function(FunctionType {
            params: vec![("a".into(), Type::String)],
            return_type: Arc::new(Type::Void),
            type_params: vec![],
            type_param_defaults: Vec::new(),
            type_param_constraints: Vec::new(),
            type_predicate: None,
        });
        assert!(is_assignable(&target, &source));
    }

    #[test]
    fn object_structural() {
        let target = Type::ObjectType(ObjectTypeInfo::new(ObjectTypeData {
            properties: vec![("name".into(), Arc::new(Type::String))],
            call_signatures: vec![],
            construct_signatures: vec![],
            index_signature: None,
            index_signature_name: None,
            method_names: Vec::new(),
        }));
        let source = Type::ObjectType(ObjectTypeInfo::new(ObjectTypeData {
            properties: vec![
                ("name".into(), Arc::new(Type::String)),
                ("age".into(), Arc::new(Type::Number)),
            ],
            call_signatures: vec![],
            construct_signatures: vec![],
            index_signature: None,
            index_signature_name: None,
            method_names: Vec::new(),
        }));
        assert!(is_assignable(&target, &source));
    }

    #[test]
    fn object_missing_property() {
        let target = Type::ObjectType(ObjectTypeInfo::new(ObjectTypeData {
            properties: vec![
                ("name".into(), Arc::new(Type::String)),
                ("age".into(), Arc::new(Type::Number)),
            ],
            call_signatures: vec![],
            construct_signatures: vec![],
            index_signature: None,
            index_signature_name: None,
            method_names: Vec::new(),
        }));
        let source = Type::ObjectType(ObjectTypeInfo::new(ObjectTypeData {
            properties: vec![("name".into(), Arc::new(Type::String))],
            call_signatures: vec![],
            construct_signatures: vec![],
            index_signature: None,
            index_signature_name: None,
            method_names: Vec::new(),
        }));
        assert!(!is_assignable(&target, &source));
    }

    #[test]
    fn type_reference_match() {
        let t = Type::TypeReference("Promise".into(), vec![Type::String].into());
        let s = Type::TypeReference("Promise".into(), vec![Type::String].into());
        assert!(is_assignable(&t, &s));

        let s2 = Type::TypeReference("Promise".into(), vec![Type::Number].into());
        assert!(!is_assignable(&t, &s2));

        let s3 = Type::TypeReference("Observable".into(), vec![Type::String].into());
        assert!(!is_assignable(&t, &s3));
    }

    #[test]
    fn void_and_undefined() {
        assert!(is_assignable(&Type::Void, &Type::Undefined));
        assert!(is_assignable(&Type::Undefined, &Type::Void));
        assert!(!is_assignable(&Type::Void, &Type::Null));
        assert!(!is_assignable(&Type::Void, &Type::Number));
    }

    #[test]
    fn castability_allows_widening_to_literal() {
        let mut dejavu = HashSet::new();
        let opts = AssignOpts {
            for_castability: true,
            ..Default::default()
        };
        assert!(assign_with(
            &Type::StringLiteral("hello".into()),
            &Type::String,
            opts,
            &mut dejavu,
        )
        .is_ok());
    }

    #[test]
    fn error_type_is_lenient() {
        assert!(is_assignable(&Type::Number, &Type::Error));
        assert!(is_assignable(&Type::Error, &Type::Number));
    }

    #[test]
    fn bigint_accepts_bigint_literal() {
        assert!(is_assignable(
            &Type::BigInt,
            &Type::BigIntLiteral("100n".into())
        ));
        assert!(!is_assignable(
            &Type::BigIntLiteral("100n".into()),
            &Type::BigInt
        ));
    }

    #[test]
    fn symbol_accepts_unique_symbol() {
        assert!(is_assignable(
            &Type::Symbol,
            &Type::UniqueSymbol("s1".into())
        ));
    }

    #[test]
    fn conditional_source_both_branches() {
        // source is Conditional { true: number, false: string }
        // target is number | string
        let source = Type::Conditional {
            check: Arc::new(Type::Any),
            extends: Arc::new(Type::Any),
            true_type: Arc::new(Type::Number),
            false_type: Arc::new(Type::String),
        };
        let target = Type::Union(vec![Type::Number, Type::String].into());
        assert!(is_assignable(&target, &source));
    }

    #[test]
    fn readonly_is_transparent() {
        assert!(is_assignable(
            &Type::Readonly(Arc::new(Type::Number)),
            &Type::Number
        ));
        assert!(is_assignable(
            &Type::Number,
            &Type::Readonly(Arc::new(Type::Number))
        ));
    }

    #[test]
    fn optional_accepts_undefined() {
        assert!(is_assignable(
            &Type::Optional(Arc::new(Type::String)),
            &Type::Undefined
        ));
        assert!(is_assignable(
            &Type::Optional(Arc::new(Type::String)),
            &Type::StringLiteral("x".into())
        ));
    }
}
