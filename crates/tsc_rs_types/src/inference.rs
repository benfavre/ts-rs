//! Generic type parameter inference.
//!
//! Given a function with type parameters and actual arguments, this module
//! infers concrete types for each parameter.  The algorithm is ported from
//! stc's `stc_ts_file_analyzer::analyzer::generic` module.

use std::sync::Arc;

use std::collections::HashMap;

use crate::{FunctionType, ObjectTypeInfo, Type};

// ---------------------------------------------------------------------------
// Inference priority
// ---------------------------------------------------------------------------

/// Priority flags for inference candidates.
///
/// Lower numeric values indicate higher confidence.  When a candidate arrives
/// with a strictly lower priority value than the current best, previous
/// candidates are discarded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct InferencePriority(pub u32);

impl InferencePriority {
    pub const NONE: Self = Self(0);
    /// Naked type variable in union or intersection type.
    pub const NAKED_TYPE_VARIABLE: Self = Self(1 << 0);
    /// Speculative tuple inference.
    pub const SPECULATIVE_TUPLE: Self = Self(1 << 1);
    /// Source of inference originated within a substitution type's substitute.
    pub const SUBSTITUTE_SOURCE: Self = Self(1 << 2);
    /// Reverse inference for homomorphic mapped type.
    pub const HOMOMORPHIC_MAPPED_TYPE: Self = Self(1 << 3);
    /// Partial reverse inference for homomorphic mapped type.
    pub const PARTIAL_HOMOMORPHIC_MAPPED_TYPE: Self = Self(1 << 4);
    /// Reverse inference for mapped type.
    pub const MAPPED_TYPE_CONSTRAINT: Self = Self(1 << 5);
    /// Conditional type in contravariant position.
    pub const CONTRAVARIANT_CONDITIONAL: Self = Self(1 << 6);
    /// Inference made from return type of generic function.
    pub const RETURN_TYPE: Self = Self(1 << 7);
    /// Inference made from a string literal to a keyof T.
    pub const LITERAL_KEYOF: Self = Self(1 << 8);
    /// Don't infer from constraints of instantiable types.
    pub const NO_CONSTRAINTS: Self = Self(1 << 9);
    /// Always use strict rules for contravariant inferences.
    pub const ALWAYS_STRICT: Self = Self(1 << 10);
    /// Seed for inference priority tracking.
    pub const MAX_VALUE: Self = Self(1 << 11);

    /// These priorities imply that the resulting type should be a combination
    /// of all candidates.
    pub const PRIORITY_IMPLIES_COMBINATION: Self =
        Self(Self::RETURN_TYPE.0 | Self::MAPPED_TYPE_CONSTRAINT.0 | Self::LITERAL_KEYOF.0);

    /// Combine (bitwise OR) two priorities.
    #[inline]
    pub fn combine(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

// ---------------------------------------------------------------------------
// Per-type-parameter inference state
// ---------------------------------------------------------------------------

/// Tracks inference state for a single type parameter.
#[derive(Debug, Clone)]
pub struct InferenceInfo {
    /// Candidates in covariant positions.
    pub candidates: Vec<Type>,
    /// Candidates in contravariant positions.
    pub contra_candidates: Vec<Type>,
    /// Priority of the current inference set.
    pub priority: InferencePriority,
    /// `true` if all inferences are to top-level occurrences.
    pub top_level: bool,
    /// `true` if inferences are fixed (user-supplied).
    pub is_fixed: bool,
}

impl InferenceInfo {
    fn new() -> Self {
        Self {
            candidates: Vec::new(),
            contra_candidates: Vec::new(),
            priority: InferencePriority::MAX_VALUE,
            top_level: true,
            is_fixed: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Inference context
// ---------------------------------------------------------------------------

/// State for an inference session.
///
/// Create one context per call-site, feed it parameter/argument pairs via
/// [`InferenceContext::infer_type`], then call [`InferenceContext::finalize`]
/// to obtain the resolved substitution map.
#[derive(Debug)]
pub struct InferenceContext {
    /// Per-type-parameter inference info, keyed by parameter name.
    pub type_params: HashMap<String, InferenceInfo>,
    /// `true` when we are currently in a contravariant position (e.g. function
    /// parameter types).
    pub contravariant: bool,
    /// Pairs already visited, used to break infinite recursion.
    dejavu: Vec<(Type, Type)>,
}

impl InferenceContext {
    /// Create a new context for the given type parameter names.
    pub fn new(type_param_names: &[String]) -> Self {
        let mut type_params = HashMap::new();
        for name in type_param_names {
            type_params.insert(name.clone(), InferenceInfo::new());
        }
        Self {
            type_params,
            contravariant: false,
            dejavu: Vec::new(),
        }
    }

    // -----------------------------------------------------------------
    // Core inference
    // -----------------------------------------------------------------

    /// Infer types so that `param` is compatible with `arg`.
    ///
    /// This is the main entry point modelled after `inferFromTypes` in tsc.
    pub fn infer_type(&mut self, param: &Type, arg: &Type) {
        // Cycle detection.
        let pair = (param.clone(), arg.clone());
        if self.dejavu.contains(&pair) {
            return;
        }
        self.dejavu.push(pair);

        // Trivial: identical types need no work.
        if param == arg {
            return;
        }
        // A union SOURCE infers from each constituent (tsc inferFromTypes):
        // `Box<T>` against `Box<number> | undefined` binds T = number.
        if let Type::Union(members) = arg {
            if !matches!(
                param,
                Type::Union(_) | Type::TypeParameter(_) | Type::Infer(_)
            ) && !matches!(param, Type::TypeReference(_, args) if args.is_empty())
            {
                for member in members.iter() {
                    self.infer_type(param, member);
                }
                return;
            }
        }

        match param {
            // ---- TypeParameter: record candidate ----
            Type::TypeParameter(name) => {
                if let Some(info) = self.type_params.get_mut(name) {
                    if info.is_fixed {
                        return;
                    }
                    if self.contravariant {
                        info.contra_candidates.push(arg.clone());
                    } else {
                        info.candidates.push(arg.clone());
                    }
                }
            }

            // ---- Infer (inside conditional extends): same as TypeParameter ----
            Type::Infer(name) => {
                if let Some(info) = self.type_params.get_mut(name) {
                    if info.is_fixed {
                        return;
                    }
                    if self.contravariant {
                        info.contra_candidates.push(arg.clone());
                    } else {
                        info.candidates.push(arg.clone());
                    }
                }
            }

            // ---- Array ----
            Type::Array(param_elem) => {
                match arg {
                    Type::Array(arg_elem) => {
                        self.infer_type(param_elem, arg_elem);
                    }
                    Type::Tuple(arg_elems) => {
                        // Infer element type as union of tuple members.
                        for elem in arg_elems.iter() {
                            self.infer_type(param_elem, elem);
                        }
                    }
                    _ => {}
                }
            }

            // ---- Tuple ----
            Type::Tuple(param_elems) => {
                if let Type::Tuple(arg_elems) = arg {
                    for (p, a) in param_elems.iter().zip(arg_elems.iter()) {
                        self.infer_type(p, a);
                    }
                } else if let Type::Array(arg_elem) = arg {
                    for p in param_elems.iter() {
                        self.infer_type(p, arg_elem);
                    }
                }
            }

            // ---- Function ----
            Type::Function(p_fn) => {
                if let Type::Function(a_fn) = arg {
                    self.infer_function(p_fn, a_fn);
                }
            }

            // ---- Union ----
            Type::Union(param_members) => {
                // tsc inferFromMatchingTypes: members present on both sides
                // match each other and drop out (`T | undefined` against
                // `Statement | undefined` binds T = Statement, not the union);
                // the rest infers from the remaining source members.
                if let Type::Union(arg_members) = arg {
                    let remaining_params: Vec<&Type> = param_members
                        .iter()
                        .filter(|pm| !arg_members.contains(pm))
                        .collect();
                    let remaining_args: Vec<Type> = arg_members
                        .iter()
                        .filter(|am| !param_members.contains(am))
                        .cloned()
                        .collect();
                    if remaining_params.len() < param_members.len() {
                        if remaining_params.is_empty() || remaining_args.is_empty() {
                            return;
                        }
                        let rest = if remaining_args.len() == 1 {
                            remaining_args.into_iter().next().unwrap()
                        } else {
                            Type::Union(remaining_args.into())
                        };
                        for pm in remaining_params {
                            self.infer_type(pm, &rest);
                        }
                        return;
                    }
                }
                // Try to infer from each param member that matches the arg.
                for pm in param_members.iter() {
                    self.infer_type(pm, arg);
                }
            }

            // ---- Intersection ----
            Type::Intersection(param_members) => {
                for pm in param_members.iter() {
                    self.infer_type(pm, arg);
                }
            }

            // ---- ObjectType ----
            Type::ObjectType(p_obj) => {
                if let Type::ObjectType(a_obj) = arg {
                    self.infer_object_type(p_obj, a_obj);
                }
            }

            // ---- Conditional ----
            Type::Conditional {
                check: p_check,
                extends: p_extends,
                true_type: p_true,
                false_type: p_false,
            } => {
                if let Type::Conditional {
                    check: a_check,
                    extends: a_extends,
                    true_type: a_true,
                    false_type: a_false,
                } = arg
                {
                    self.infer_type(p_check, a_check);
                    self.infer_type(p_extends, a_extends);
                    self.infer_type(p_true, a_true);
                    self.infer_type(p_false, a_false);
                } else {
                    // Infer from both branches.
                    self.infer_type(p_true, arg);
                    self.infer_type(p_false, arg);
                }
            }

            // ---- TypeReference ----
            Type::TypeReference(p_name, p_args) => {
                if let Type::TypeReference(a_name, a_args) = arg {
                    if p_name == a_name {
                        for (pa, aa) in p_args.iter().zip(a_args.iter()) {
                            self.infer_type(pa, aa);
                        }
                    }
                }
            }

            // ---- IndexedAccess ----
            Type::IndexedAccess(p_obj, p_idx) => {
                if let Type::IndexedAccess(a_obj, a_idx) = arg {
                    self.infer_type(p_obj, a_obj);
                    self.infer_type(p_idx, a_idx);
                }
            }

            // ---- Keyof ----
            Type::Keyof(p_inner) => {
                if let Type::Keyof(a_inner) = arg {
                    self.infer_type(p_inner, a_inner);
                }
            }

            // ---- Mapped ----
            Type::Mapped {
                constraint: p_constraint,
                template: p_template,
                ..
            } => {
                if let Type::Mapped {
                    constraint: a_constraint,
                    template: a_template,
                    ..
                } = arg
                {
                    self.infer_type(p_constraint, a_constraint);
                    if let (Some(pt), Some(at)) = (p_template.as_deref(), a_template.as_deref()) {
                        self.infer_type(pt, at);
                    }
                }
            }

            // ---- Rest ----
            Type::Rest(p_inner) => {
                if let Type::Rest(a_inner) = arg {
                    self.infer_type(p_inner, a_inner);
                } else {
                    self.infer_type(p_inner, arg);
                }
            }

            // ---- Optional ----
            Type::Optional(p_inner) => {
                if let Type::Optional(a_inner) = arg {
                    self.infer_type(p_inner, a_inner);
                } else {
                    self.infer_type(p_inner, arg);
                }
            }

            // ---- Readonly ----
            Type::Readonly(p_inner) => {
                if let Type::Readonly(a_inner) = arg {
                    self.infer_type(p_inner, a_inner);
                } else {
                    self.infer_type(p_inner, arg);
                }
            }

            // ---- Instance ----
            Type::Instance(p_inner) => {
                if let Type::Instance(a_inner) = arg {
                    self.infer_type(p_inner, a_inner);
                }
            }

            // ---- Import ----
            Type::Import {
                module: p_mod,
                name: p_name,
                type_args: p_args,
            } => {
                if let Type::Import {
                    module: a_mod,
                    name: a_name,
                    type_args: a_args,
                } = arg
                {
                    if p_mod == a_mod && p_name == a_name {
                        for (pa, aa) in p_args.iter().zip(a_args.iter()) {
                            self.infer_type(pa, aa);
                        }
                    }
                }
            }

            // ---- Constructor ----
            Type::Constructor(p_ctor) => {
                if let Type::Constructor(a_ctor) = arg {
                    let normalized;
                    let p_ctor = if p_ctor.type_params.is_empty() {
                        p_ctor
                    } else {
                        let canonical = (0..p_ctor.type_params.len())
                            .map(|index| Type::TypeParameter(format!("__inner_constructor{index}")))
                            .collect::<Vec<_>>();
                        normalized = crate::alpha_normalize_constructor(p_ctor, &canonical);
                        &normalized
                    };
                    // params contravariant
                    self.contravariant = !self.contravariant;
                    for (pp, ap) in p_ctor.params.iter().zip(a_ctor.params.iter()) {
                        self.infer_type(&pp.1, &ap.1);
                    }
                    self.contravariant = !self.contravariant;
                    // return type covariant
                    self.infer_type(&p_ctor.return_type, &a_ctor.return_type);
                }
            }

            // ---- Predicate ----
            Type::Predicate(p_pred) => {
                if let Type::Predicate(a_pred) = arg {
                    self.infer_type(&p_pred.target_type, &a_pred.target_type);
                }
            }

            // ---- StringMapping ----
            Type::StringMapping { inner: p_inner, .. } => {
                if let Type::StringMapping {
                    inner: a_inner,
                    kind: a_kind,
                } = arg
                {
                    if let Type::StringMapping { kind: p_kind, .. } = param {
                        if p_kind == a_kind {
                            self.infer_type(p_inner, a_inner);
                        }
                    }
                }
            }

            // ---- TemplateLiteral ----
            Type::TemplateLiteral { types: p_types, .. } => {
                if let Type::TemplateLiteral { types: a_types, .. } = arg {
                    for (pt, at) in p_types.iter().zip(a_types.iter()) {
                        self.infer_type(pt, at);
                    }
                }
            }

            // All other types are leaves or do not contain inferrable sub-types.
            _ => {}
        }
    }

    /// Infer from function types.  Parameter positions are contravariant;
    /// the return type is covariant.
    fn infer_function(&mut self, p: &FunctionType, a: &FunctionType) {
        // Parameters: contravariant.
        let saved = self.contravariant;
        self.contravariant = !self.contravariant;
        for (pp, ap) in p.params.iter().zip(a.params.iter()) {
            self.infer_type(&pp.1, &ap.1);
        }
        self.contravariant = saved;

        // Return type: covariant.
        self.infer_type(&p.return_type, &a.return_type);
    }

    /// Infer from object type members, matching properties by name.
    fn infer_object_type(&mut self, p: &ObjectTypeInfo, a: &ObjectTypeInfo) {
        for (p_name, p_ty) in &p.properties {
            if let Some((_, a_ty)) = a.properties.iter().find(|(n, _)| n == p_name) {
                self.infer_type(p_ty, a_ty);
            }
        }

        // Call signatures.
        for (ps, a_s) in p.call_signatures.iter().zip(a.call_signatures.iter()) {
            self.infer_function(ps, a_s);
        }

        // Construct signatures.
        for (ps, a_s) in p
            .construct_signatures
            .iter()
            .zip(a.construct_signatures.iter())
        {
            let normalized;
            let ps = if ps.type_params.is_empty() {
                ps
            } else {
                let canonical = (0..ps.type_params.len())
                    .map(|index| Type::TypeParameter(format!("__inner_constructor{index}")))
                    .collect::<Vec<_>>();
                normalized = crate::alpha_normalize_constructor(ps, &canonical);
                &normalized
            };
            let saved = self.contravariant;
            self.contravariant = !self.contravariant;
            for (pp, ap) in ps.params.iter().zip(a_s.params.iter()) {
                self.infer_type(&pp.1, &ap.1);
            }
            self.contravariant = saved;
            self.infer_type(&ps.return_type, &a_s.return_type);
        }

        // Index signature.
        if let (Some((pk, pv)), Some((ak, av))) = (&p.index_signature, &a.index_signature) {
            self.infer_type(pk, ak);
            self.infer_type(pv, av);
        }
    }

    /// Infer in a contravariant context (flips the flag, runs inference,
    /// restores the flag).
    pub fn infer_contravariant(&mut self, param: &Type, arg: &Type) {
        let saved = self.contravariant;
        self.contravariant = !self.contravariant;
        self.infer_type(param, arg);
        self.contravariant = saved;
    }

    // -----------------------------------------------------------------
    // Finalization
    // -----------------------------------------------------------------

    /// Resolve candidates to final types.
    ///
    /// For each type parameter:
    /// - If there are covariant candidates, use them.
    /// - Otherwise fall back to contravariant candidates.
    /// - If a single candidate exists, use it directly.
    /// - If multiple exist, form a `Union` of all candidates.
    /// - If none exist, default to `Type::Unknown`.
    pub fn finalize(self) -> HashMap<String, Type> {
        let mut result = HashMap::new();
        for (name, info) in self.type_params {
            let candidates = if !info.candidates.is_empty() {
                info.candidates
            } else if !info.contra_candidates.is_empty() {
                info.contra_candidates
            } else {
                result.insert(name, Type::Unknown);
                continue;
            };

            let ty = if candidates.len() == 1 {
                candidates.into_iter().next().unwrap()
            } else {
                // Deduplicate.
                let mut deduped: Vec<Type> = Vec::new();
                for c in candidates {
                    if !deduped.contains(&c) {
                        deduped.push(c);
                    }
                }
                if deduped.len() == 1 {
                    deduped.into_iter().next().unwrap()
                } else {
                    Type::Union(deduped.into())
                }
            };
            result.insert(name, ty);
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Substitute
// ---------------------------------------------------------------------------

/// Replace `TypeParameter` references with concrete types from the
/// substitution map, recursively walking all `Type` variants.
pub fn substitute(ty: &Type, substitutions: &HashMap<String, Type>) -> Type {
    match ty {
        // TypeParameter: look up in the map.
        Type::TypeParameter(name) => {
            if let Some(replacement) = substitutions.get(name) {
                replacement.clone()
            } else {
                ty.clone()
            }
        }

        // Infer: treated like TypeParameter for substitution purposes.
        Type::Infer(name) => {
            if let Some(replacement) = substitutions.get(name) {
                replacement.clone()
            } else {
                ty.clone()
            }
        }

        // Compound types: recurse into children.
        Type::Array(elem) => Type::Array(Arc::new(substitute(elem, substitutions))),

        Type::Tuple(elems) => {
            Type::Tuple(elems.iter().map(|e| substitute(e, substitutions)).collect())
        }

        Type::Union(members) => Type::Union(
            members
                .iter()
                .map(|m| substitute(m, substitutions))
                .collect(),
        ),

        Type::Intersection(members) => Type::Intersection(
            members
                .iter()
                .map(|m| substitute(m, substitutions))
                .collect(),
        ),

        Type::Function(f) => Type::Function(FunctionType {
            type_param_constraints: f
                .type_param_constraints
                .iter()
                .map(|default| default.as_ref().map(|ty| substitute(ty, substitutions)))
                .collect(),
            params: f
                .params
                .iter()
                .map(|(name, ty)| (name.clone(), substitute(ty, substitutions)))
                .collect(),
            return_type: Arc::new(substitute(&f.return_type, substitutions)),
            type_params: f.type_params.clone(),
            type_param_defaults: f
                .type_param_defaults
                .iter()
                .map(|default| default.as_ref().map(|ty| substitute(ty, substitutions)))
                .collect(),
            type_predicate: f.type_predicate.as_ref().map(|pred| crate::TypePredicate {
                param_name: pred.param_name.clone(),
                target_type: Arc::new(substitute(&pred.target_type, substitutions)),
                is_asserts: pred.is_asserts,
            }),
        }),

        Type::ObjectType(obj) => Type::ObjectType(ObjectTypeInfo {
            properties: obj
                .properties
                .iter()
                .map(|(name, ty)| (name.clone(), Arc::new(substitute(ty, substitutions))))
                .collect(),
            call_signatures: obj
                .call_signatures
                .iter()
                .map(|sig| substitute_function_type(sig, substitutions))
                .collect(),
            construct_signatures: obj
                .construct_signatures
                .iter()
                .map(
                    |sig| match substitute(&Type::Constructor(sig.clone()), substitutions) {
                        Type::Constructor(signature) => signature,
                        _ => unreachable!("constructor substitution changed its kind"),
                    },
                )
                .collect(),
            index_signature: obj.index_signature.as_ref().map(|(k, v)| {
                (
                    Arc::new(substitute(k, substitutions)),
                    Arc::new(substitute(v, substitutions)),
                )
            }),
            index_signature_name: obj.index_signature_name.clone(),
            method_names: obj.method_names.clone(),
        }),

        Type::TypeReference(name, args) => Type::TypeReference(
            name.clone(),
            args.iter().map(|a| substitute(a, substitutions)).collect(),
        ),

        Type::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => Type::Conditional {
            check: Arc::new(substitute(check, substitutions)),
            extends: Arc::new(substitute(extends, substitutions)),
            true_type: Arc::new(substitute(true_type, substitutions)),
            false_type: Arc::new(substitute(false_type, substitutions)),
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
            constraint: Arc::new(substitute(constraint, substitutions)),
            template: template
                .as_ref()
                .map(|t| Arc::new(substitute(t, substitutions))),
            name_type: name_type
                .as_ref()
                .map(|n| Arc::new(substitute(n, substitutions))),
            readonly_mod: *readonly_mod,
            optional_mod: *optional_mod,
        },

        Type::IndexedAccess(obj, idx) => Type::IndexedAccess(
            Arc::new(substitute(obj, substitutions)),
            Arc::new(substitute(idx, substitutions)),
        ),

        Type::Keyof(inner) => Type::Keyof(Arc::new(substitute(inner, substitutions))),

        Type::Rest(inner) => Type::Rest(Arc::new(substitute(inner, substitutions))),

        Type::Optional(inner) => Type::Optional(Arc::new(substitute(inner, substitutions))),

        Type::Readonly(inner) => Type::Readonly(Arc::new(substitute(inner, substitutions))),

        Type::Instance(inner) => Type::Instance(Arc::new(substitute(inner, substitutions))),

        Type::Constructor(ctor) => {
            let mut inner_substitutions = substitutions.clone();
            for parameter in &ctor.type_params {
                inner_substitutions.remove(parameter);
            }
            Type::Constructor(crate::ConstructorType {
                is_abstract: ctor.is_abstract,
                params: ctor
                    .params
                    .iter()
                    .map(|(name, ty)| (name.clone(), substitute(ty, &inner_substitutions)))
                    .collect(),
                return_type: Arc::new(substitute(&ctor.return_type, &inner_substitutions)),
                type_params: ctor.type_params.clone(),
                type_param_constraints: ctor
                    .type_param_constraints
                    .iter()
                    .map(|constraint| {
                        constraint
                            .as_ref()
                            .map(|ty| substitute(ty, &inner_substitutions))
                    })
                    .collect(),
                type_param_defaults: ctor
                    .type_param_defaults
                    .iter()
                    .map(|default| {
                        default
                            .as_ref()
                            .map(|ty| substitute(ty, &inner_substitutions))
                    })
                    .collect(),
            })
        }

        Type::Import {
            module,
            name,
            type_args,
        } => Type::Import {
            module: module.clone(),
            name: name.clone(),
            type_args: type_args
                .iter()
                .map(|a| substitute(a, substitutions))
                .collect(),
        },

        Type::Predicate(pred) => Type::Predicate(crate::TypePredicate {
            param_name: pred.param_name.clone(),
            target_type: Arc::new(substitute(&pred.target_type, substitutions)),
            is_asserts: pred.is_asserts,
        }),

        Type::StringMapping { kind, inner } => Type::StringMapping {
            kind: *kind,
            inner: Arc::new(substitute(inner, substitutions)),
        },

        Type::TemplateLiteral { quasis, types } => Type::TemplateLiteral {
            quasis: quasis.clone(),
            types: types.iter().map(|t| substitute(t, substitutions)).collect(),
        },

        Type::EnumVariant {
            enum_name,
            variant_name,
            value,
        } => Type::EnumVariant {
            enum_name: enum_name.clone(),
            variant_name: variant_name.clone(),
            value: value
                .as_ref()
                .map(|v| Arc::new(substitute(v, substitutions))),
        },

        Type::EnumType(info) => Type::EnumType(crate::EnumTypeInfo {
            name: info.name.clone(),
            members: info
                .members
                .iter()
                .map(|(n, ty)| (n.clone(), substitute(ty, substitutions)))
                .collect(),
            is_const: info.is_const,
        }),

        Type::Namespace(ns) => Type::Namespace(crate::NamespaceType {
            name: ns.name.clone(),
            exports: ns
                .exports
                .iter()
                .map(|(n, ty)| (n.clone(), substitute(ty, substitutions)))
                .collect(),
            readonly_exports: ns.readonly_exports.clone(),
        }),

        Type::Module(m) => Type::Module(crate::ModuleType {
            name: m.name.clone(),
            exports: m
                .exports
                .iter()
                .map(|(n, ty)| (n.clone(), substitute(ty, substitutions)))
                .collect(),
        }),

        // Leaf types: no sub-types to recurse into.
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
        | Type::Typeof(_)
        | Type::UniqueSymbol(_)
        | Type::Error => ty.clone(),
    }
}

/// Helper: substitute inside a `FunctionType`.
fn substitute_function_type(
    f: &FunctionType,
    substitutions: &HashMap<String, Type>,
) -> FunctionType {
    FunctionType {
        type_param_constraints: f
            .type_param_constraints
            .iter()
            .map(|default| default.as_ref().map(|ty| substitute(ty, substitutions)))
            .collect(),
        params: f
            .params
            .iter()
            .map(|(name, ty)| (name.clone(), substitute(ty, substitutions)))
            .collect(),
        return_type: Arc::new(substitute(&f.return_type, substitutions)),
        type_params: f.type_params.clone(),
        type_param_defaults: f
            .type_param_defaults
            .iter()
            .map(|default| default.as_ref().map(|ty| substitute(ty, substitutions)))
            .collect(),
        type_predicate: f.type_predicate.as_ref().map(|pred| crate::TypePredicate {
            param_name: pred.param_name.clone(),
            target_type: Arc::new(substitute(&pred.target_type, substitutions)),
            is_asserts: pred.is_asserts,
        }),
    }
}

// ---------------------------------------------------------------------------
// Convenience function
// ---------------------------------------------------------------------------

/// Infer type arguments for a generic function given parameter types and
/// argument types.
///
/// ```text
/// function id<T>(x: T): T { return x; }
/// id(42)  // infer T = number
/// ```
pub fn infer_type_arguments(
    type_param_names: &[String],
    param_types: &[Type],
    arg_types: &[Type],
) -> HashMap<String, Type> {
    let mut ctx = InferenceContext::new(type_param_names);
    for (param, arg) in param_types.iter().zip(arg_types.iter()) {
        ctx.infer_type(param, arg);
    }
    ctx.finalize()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infer_simple_type_param() {
        // function id<T>(x: T): T — call with number
        let result = infer_type_arguments(
            &["T".to_string()],
            &[Type::TypeParameter("T".to_string())],
            &[Type::Number],
        );
        assert_eq!(result.get("T"), Some(&Type::Number));
    }

    #[test]
    fn infer_array_element() {
        // function first<T>(arr: T[]): T — call with number[]
        let result = infer_type_arguments(
            &["T".to_string()],
            &[Type::Array(Arc::new(Type::TypeParameter("T".to_string())))],
            &[Type::Array(Arc::new(Type::Number))],
        );
        assert_eq!(result.get("T"), Some(&Type::Number));
    }

    #[test]
    fn infer_tuple_elements() {
        // function pair<A, B>(t: [A, B]): [A, B]
        let result = infer_type_arguments(
            &["A".to_string(), "B".to_string()],
            &[Type::Tuple(
                vec![
                    Type::TypeParameter("A".to_string()),
                    Type::TypeParameter("B".to_string()),
                ]
                .into(),
            )],
            &[Type::Tuple(vec![Type::Number, Type::String].into())],
        );
        assert_eq!(result.get("A"), Some(&Type::Number));
        assert_eq!(result.get("B"), Some(&Type::String));
    }

    #[test]
    fn infer_function_return() {
        // function apply<T>(f: () => T): T
        let result = infer_type_arguments(
            &["T".to_string()],
            &[Type::Function(FunctionType {
                params: vec![],
                return_type: Arc::new(Type::TypeParameter("T".to_string())),
                type_params: vec![],
                type_param_defaults: Vec::new(),
                type_param_constraints: Vec::new(),
                type_predicate: None,
            })],
            &[Type::Function(FunctionType {
                params: vec![],
                return_type: Arc::new(Type::String),
                type_params: vec![],
                type_param_defaults: Vec::new(),
                type_param_constraints: Vec::new(),
                type_predicate: None,
            })],
        );
        assert_eq!(result.get("T"), Some(&Type::String));
    }

    #[test]
    fn infer_object_property() {
        // function get<T>(obj: { value: T }): T
        let result = infer_type_arguments(
            &["T".to_string()],
            &[Type::ObjectType(ObjectTypeInfo {
                properties: vec![(
                    "value".to_string(),
                    Arc::new(Type::TypeParameter("T".to_string())),
                )],
                call_signatures: vec![],
                construct_signatures: vec![],
                index_signature: None,
                index_signature_name: None,
                method_names: Vec::new(),
            })],
            &[Type::ObjectType(ObjectTypeInfo {
                properties: vec![("value".to_string(), Arc::new(Type::Boolean))],
                call_signatures: vec![],
                construct_signatures: vec![],
                index_signature: None,
                index_signature_name: None,
                method_names: Vec::new(),
            })],
        );
        assert_eq!(result.get("T"), Some(&Type::Boolean));
    }

    #[test]
    fn infer_type_reference_args() {
        // function box<T>(x: Box<T>): T
        let result = infer_type_arguments(
            &["T".to_string()],
            &[Type::TypeReference(
                "Box".to_string(),
                vec![Type::TypeParameter("T".to_string())].into(),
            )],
            &[Type::TypeReference(
                "Box".to_string(),
                vec![Type::Number].into(),
            )],
        );
        assert_eq!(result.get("T"), Some(&Type::Number));
    }

    #[test]
    fn infer_conditional_both_branches() {
        // param: T extends string ? T : never  with arg: number
        // Should infer T from both branches; true_type has T => number candidate
        let result = infer_type_arguments(
            &["T".to_string()],
            &[Type::Conditional {
                check: Arc::new(Type::TypeParameter("T".to_string())),
                extends: Arc::new(Type::String),
                true_type: Arc::new(Type::TypeParameter("T".to_string())),
                false_type: Arc::new(Type::Never),
            }],
            &[Type::Number],
        );
        assert_eq!(result.get("T"), Some(&Type::Number));
    }

    #[test]
    fn infer_defaults_to_unknown() {
        let result = infer_type_arguments(&["T".to_string()], &[Type::Number], &[Type::Number]);
        // T is never used, so defaults to Unknown.
        assert_eq!(result.get("T"), Some(&Type::Unknown));
    }

    #[test]
    fn substitute_replaces_type_param() {
        let mut subs = HashMap::new();
        subs.insert("T".to_string(), Type::Number);

        let input = Type::Array(Arc::new(Type::TypeParameter("T".to_string())));
        let output = substitute(&input, &subs);
        assert_eq!(output, Type::Array(Arc::new(Type::Number)));
    }

    #[test]
    fn substitute_deep_conditional() {
        let mut subs = HashMap::new();
        subs.insert("T".to_string(), Type::String);

        let input = Type::Conditional {
            check: Arc::new(Type::TypeParameter("T".to_string())),
            extends: Arc::new(Type::String),
            true_type: Arc::new(Type::TypeParameter("T".to_string())),
            false_type: Arc::new(Type::Never),
        };
        let output = substitute(&input, &subs);
        assert_eq!(
            output,
            Type::Conditional {
                check: Arc::new(Type::String),
                extends: Arc::new(Type::String),
                true_type: Arc::new(Type::String),
                false_type: Arc::new(Type::Never),
            }
        );
    }

    #[test]
    fn substitute_leaves_unmatched() {
        let subs = HashMap::new();
        let input = Type::TypeParameter("U".to_string());
        let output = substitute(&input, &subs);
        assert_eq!(output, Type::TypeParameter("U".to_string()));
    }

    #[test]
    fn infer_multiple_candidates_union() {
        // function f<T>(a: T, b: T): T — call with (number, string)
        let result = infer_type_arguments(
            &["T".to_string()],
            &[
                Type::TypeParameter("T".to_string()),
                Type::TypeParameter("T".to_string()),
            ],
            &[Type::Number, Type::String],
        );
        assert_eq!(
            result.get("T"),
            Some(&Type::Union(vec![Type::Number, Type::String].into()))
        );
    }

    #[test]
    fn substitute_function_type_test() {
        let mut subs = HashMap::new();
        subs.insert("T".to_string(), Type::Boolean);

        let input = Type::Function(FunctionType {
            params: vec![("x".to_string(), Type::TypeParameter("T".to_string()))],
            return_type: Arc::new(Type::TypeParameter("T".to_string())),
            type_params: vec!["T".to_string()],
            type_param_defaults: Vec::new(),
            type_param_constraints: Vec::new(),
            type_predicate: None,
        });
        let output = substitute(&input, &subs);
        assert_eq!(
            output,
            Type::Function(FunctionType {
                params: vec![("x".to_string(), Type::Boolean)],
                return_type: Arc::new(Type::Boolean),
                type_params: vec!["T".to_string()],
                type_param_defaults: Vec::new(),
                type_param_constraints: Vec::new(),
                type_predicate: None,
            })
        );
    }
}
