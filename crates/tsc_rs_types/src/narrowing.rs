//! Type narrowing: applying control flow facts to refine types.
//!
//! When control flow analysis determines facts about a variable
//! (e.g., `typeof x === "string"`), this module applies those facts
//! to narrow the variable's type.

use std::sync::Arc;

use crate::control_flow::{CondFacts, Name};
use crate::type_facts::TypeFacts;
use crate::Type;

// ---------------------------------------------------------------------------
// Core narrowing by TypeFacts
// ---------------------------------------------------------------------------

/// Narrow a type using TypeFacts (from typeof, truthiness checks).
pub fn narrow_type_by_facts(ty: &Type, facts: TypeFacts) -> Type {
    if facts == TypeFacts::NONE || facts == TypeFacts::ALL {
        return ty.clone();
    }

    match ty {
        Type::Union(members) => {
            let narrowed: Vec<Type> = members
                .iter()
                .filter(|m| type_satisfies_facts(m, facts))
                .cloned()
                .collect();
            match narrowed.len() {
                0 => Type::Never,
                1 => narrowed.into_iter().next().unwrap(),
                _ => Type::Union(narrowed.into()),
            }
        }
        other => {
            if type_satisfies_facts(other, facts) {
                other.clone()
            } else {
                Type::Never
            }
        }
    }
}

/// Check if a type is compatible with the given TypeFacts.
///
/// A type satisfies facts when every flag in `facts` is also present in the
/// type's own inherent facts. This means the type is not contradicted by the
/// narrowing condition.
pub fn type_satisfies_facts(ty: &Type, facts: TypeFacts) -> bool {
    let type_facts = get_type_facts(ty);
    type_facts.contains(facts)
}

/// Get the TypeFacts that a type inherently satisfies.
pub fn get_type_facts(ty: &Type) -> TypeFacts {
    match ty {
        Type::String => TypeFacts::STRING_FACTS,
        Type::Number => TypeFacts::NUMBER_FACTS,
        Type::BigInt => TypeFacts::BIGINT_FACTS,
        Type::Boolean => TypeFacts::BOOLEAN_FACTS,
        Type::Symbol | Type::UniqueSymbol(_) => TypeFacts::SYMBOL_FACTS,
        Type::Undefined => TypeFacts::UNDEFINED_FACTS,
        Type::Null => TypeFacts::NULL_FACTS,
        Type::Void => TypeFacts::VOID_FACTS,
        Type::Function(_) | Type::Constructor(_) => TypeFacts::FUNCTION_FACTS,
        Type::ObjectType(_) | Type::Array(_) | Type::Tuple(_) => TypeFacts::OBJECT_FACTS,

        // Literal types with truthiness refinement
        Type::StringLiteral(s) => {
            if s.is_empty() {
                TypeFacts::EMPTY_STRING_FACTS
            } else {
                TypeFacts::NON_EMPTY_STRING_FACTS
            }
        }
        Type::NumberLiteral(n) => {
            if n == "0" || n == "-0" || n == "NaN" {
                TypeFacts::ZERO_FACTS
            } else {
                TypeFacts::NON_ZERO_FACTS
            }
        }
        Type::BigIntLiteral(n) => {
            if n == "0" {
                TypeFacts::ZERO_BIGINT_FACTS
            } else {
                TypeFacts::NON_ZERO_BIGINT_FACTS
            }
        }
        Type::BooleanLiteral(b) => {
            if *b {
                TypeFacts::TRUE_FACTS
            } else {
                TypeFacts::FALSE_FACTS
            }
        }

        // Union: any member's facts count
        Type::Union(members) => members
            .iter()
            .fold(TypeFacts::NONE, |acc, m| acc | get_type_facts(m)),

        // Intersection: all member's facts must hold
        Type::Intersection(members) => members
            .iter()
            .fold(TypeFacts::ALL, |acc, m| acc & get_type_facts(m)),

        // Any / Unknown / Error are conservative (all facts)
        Type::Any | Type::Unknown | Type::Error => TypeFacts::ALL,

        // Never has no facts
        Type::Never => TypeFacts::NONE,

        // Everything else: conservative
        _ => TypeFacts::ALL,
    }
}

// ---------------------------------------------------------------------------
// Convenience narrowing functions
// ---------------------------------------------------------------------------

/// Narrow a type by typeof check: `typeof x === type_str` or `typeof x !== type_str`.
pub fn narrow_by_typeof(ty: &Type, type_str: &str, is_eq: bool) -> Type {
    let facts = if is_eq {
        TypeFacts::typeof_eq(type_str)
    } else {
        TypeFacts::typeof_ne(type_str)
    };
    match facts {
        Some(f) => narrow_type_by_facts(ty, f),
        None => ty.clone(),
    }
}

/// Narrow a type by null/undefined equality check.
///
/// `check_null` and `check_undefined` control which nullish values to consider.
/// For `=== null`, use check_null=true, check_undefined=false.
/// For `== null` (loose), use check_null=true, check_undefined=true.
pub fn narrow_by_nullish(ty: &Type, check_null: bool, check_undefined: bool, is_eq: bool) -> Type {
    match ty {
        Type::Union(members) => {
            let narrowed: Vec<Type> = members
                .iter()
                .filter(|m| {
                    let is_null = matches!(m, Type::Null);
                    let is_undef = matches!(m, Type::Undefined | Type::Void);
                    let is_nullish = (check_null && is_null) || (check_undefined && is_undef);
                    if is_eq {
                        is_nullish
                    } else {
                        !is_nullish
                    }
                })
                .cloned()
                .collect();
            match narrowed.len() {
                0 => Type::Never,
                1 => narrowed.into_iter().next().unwrap(),
                _ => Type::Union(narrowed.into()),
            }
        }
        _ => {
            let is_null = matches!(ty, Type::Null);
            let is_undef = matches!(ty, Type::Undefined | Type::Void);
            let is_nullish = (check_null && is_null) || (check_undefined && is_undef);
            if is_eq == is_nullish {
                ty.clone()
            } else {
                Type::Never
            }
        }
    }
}

/// Narrow a type by `x instanceof Foo`.
///
/// In the true branch the type becomes the class type (or filtered union
/// members). In the false branch, use `exclude_types` to remove the class.
pub fn narrow_by_instanceof(ty: &Type, class_type: &Type) -> Type {
    match ty {
        Type::Union(members) => {
            let narrowed: Vec<Type> = members
                .iter()
                .filter(|m| could_be_instance_of(m, class_type))
                .cloned()
                .collect();
            if narrowed.is_empty() {
                class_type.clone()
            } else {
                match narrowed.len() {
                    1 => narrowed.into_iter().next().unwrap(),
                    _ => Type::Union(narrowed.into()),
                }
            }
        }
        _ => class_type.clone(),
    }
}

pub fn could_be_instance_of(ty: &Type, class_type: &Type) -> bool {
    matches!(ty, Type::Any | Type::Unknown)
        || ty == class_type
        || matches!(
            (ty, class_type),
            (Type::TypeReference(a, _), Type::TypeReference(b, _)) if a == b
        )
        || matches!(ty, Type::ObjectType(_))
}

/// Narrow a type by truthiness: `if (x) { ... }`.
pub fn narrow_by_truthiness(ty: &Type, is_truthy: bool) -> Type {
    match ty {
        // `boolean` is `true | false`: a truthiness check keeps one literal.
        Type::Boolean => Type::BooleanLiteral(is_truthy),
        Type::Union(members) => {
            let narrowed: Vec<Type> = members
                .iter()
                .filter(|m| {
                    if is_truthy {
                        !is_definitely_falsy(m)
                    } else {
                        !is_definitely_truthy(m)
                    }
                })
                .map(|m| match m {
                    Type::Boolean => Type::BooleanLiteral(is_truthy),
                    other => other.clone(),
                })
                .collect();
            match narrowed.len() {
                0 => Type::Never,
                1 => narrowed.into_iter().next().unwrap(),
                _ => Type::Union(narrowed.into()),
            }
        }
        _ => {
            if (is_truthy && is_definitely_falsy(ty)) || (!is_truthy && is_definitely_truthy(ty)) {
                Type::Never
            } else {
                ty.clone()
            }
        }
    }
}

/// Returns true if the type is always falsy.
fn is_definitely_falsy(ty: &Type) -> bool {
    match ty {
        Type::Null | Type::Undefined | Type::Void => true,
        Type::BooleanLiteral(false) => true,
        Type::NumberLiteral(n) => n == "0" || n == "-0" || n == "NaN",
        Type::StringLiteral(s) => s.is_empty(),
        Type::BigIntLiteral(n) => n == "0",
        _ => false,
    }
}

/// Returns true if the type is always truthy.
fn is_definitely_truthy(ty: &Type) -> bool {
    match ty {
        Type::BooleanLiteral(true) => true,
        Type::NumberLiteral(n) => n != "0" && n != "-0" && n != "NaN",
        Type::StringLiteral(s) => !s.is_empty(),
        Type::BigIntLiteral(n) => n != "0",
        Type::Symbol | Type::UniqueSymbol(_) => true,
        Type::Function(_) | Type::Constructor(_) => true,
        Type::ObjectType(_) | Type::Array(_) | Type::Tuple(_) => true,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Discriminated union narrowing
// ---------------------------------------------------------------------------

/// Narrow a union by discriminant property: `x.kind === "foo"`.
pub fn narrow_by_discriminant(ty: &Type, property_name: &str, discriminant_value: &Type) -> Type {
    if let Type::Union(members) = ty {
        let narrowed: Vec<Type> = members
            .iter()
            .filter(|m| member_has_discriminant(m, property_name, discriminant_value))
            .cloned()
            .collect();
        match narrowed.len() {
            0 => Type::Never,
            1 => narrowed.into_iter().next().unwrap(),
            _ => Type::Union(narrowed.into()),
        }
    } else {
        ty.clone()
    }
}

fn member_has_discriminant(ty: &Type, property_name: &str, value: &Type) -> bool {
    if let Type::ObjectType(obj) = ty {
        for (name, prop_ty) in &obj.properties {
            if name == property_name {
                return prop_ty.as_ref() == value || matches!(prop_ty.as_ref(), Type::Any);
            }
        }
    }
    // If we can't determine, assume it could match (conservative)
    true
}

// ---------------------------------------------------------------------------
// Applying CondFacts
// ---------------------------------------------------------------------------

/// Apply CondFacts to narrow a variable's type.
pub fn apply_cond_facts(ty: &Type, name: &Name, facts: &CondFacts) -> Type {
    let mut result = ty.clone();

    // Apply TypeFacts
    if let Some(&type_facts) = facts.facts.get(name) {
        result = narrow_type_by_facts(&result, type_facts);
    }

    // Apply direct type narrowing (from instanceof, type guards)
    if let Some(narrowed_type) = facts.vars.get(name) {
        result = narrowed_type.clone();
    }

    // Apply type exclusions
    if let Some(excluded) = facts.excludes.get(name) {
        result = exclude_types(&result, excluded);
    }

    result
}

/// Remove specific types from a type (used for negative instanceof checks).
pub fn exclude_types(ty: &Type, excluded: &[Type]) -> Type {
    if let Type::Union(members) = ty {
        let remaining: Vec<Type> = members
            .iter()
            .filter(|m| !excluded.iter().any(|e| e == *m))
            .cloned()
            .collect();
        match remaining.len() {
            0 => Type::Never,
            1 => remaining.into_iter().next().unwrap(),
            _ => Type::Union(remaining.into()),
        }
    } else if excluded.iter().any(|e| e == ty) {
        Type::Never
    } else {
        ty.clone()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::type_facts::TypeFacts;

    #[test]
    fn narrow_union_by_typeof_string() {
        let ty = Type::Union(vec![Type::String, Type::Number, Type::Boolean].into());
        let result = narrow_by_typeof(&ty, "string", true);
        assert_eq!(result, Type::String);
    }

    #[test]
    fn narrow_union_by_typeof_ne_string() {
        let ty = Type::Union(vec![Type::String, Type::Number, Type::Boolean].into());
        let result = narrow_by_typeof(&ty, "string", false);
        assert_eq!(
            result,
            Type::Union(vec![Type::Number, Type::Boolean].into())
        );
    }

    #[test]
    fn narrow_non_union_by_typeof() {
        let result = narrow_by_typeof(&Type::String, "string", true);
        assert_eq!(result, Type::String);

        let result = narrow_by_typeof(&Type::String, "number", true);
        assert_eq!(result, Type::Never);
    }

    #[test]
    fn narrow_by_nullish_eq() {
        let ty = Type::Union(vec![Type::String, Type::Null, Type::Undefined].into());
        let result = narrow_by_nullish(&ty, true, true, true);
        assert_eq!(
            result,
            Type::Union(vec![Type::Null, Type::Undefined].into())
        );
    }

    #[test]
    fn narrow_by_nullish_ne() {
        let ty = Type::Union(vec![Type::String, Type::Null, Type::Undefined].into());
        let result = narrow_by_nullish(&ty, true, true, false);
        assert_eq!(result, Type::String);
    }

    #[test]
    fn narrow_by_strict_null_eq() {
        let ty = Type::Union(vec![Type::String, Type::Null, Type::Undefined].into());
        let result = narrow_by_nullish(&ty, true, false, true);
        assert_eq!(result, Type::Null);
    }

    #[test]
    fn narrow_truthy_removes_falsy_literals() {
        let ty = Type::Union(
            vec![
                Type::StringLiteral("hello".into()),
                Type::StringLiteral("".into()),
                Type::Null,
            ]
            .into(),
        );
        let result = narrow_by_truthiness(&ty, true);
        assert_eq!(result, Type::StringLiteral("hello".into()));
    }

    #[test]
    fn narrow_falsy_removes_truthy_literals() {
        let ty = Type::Union(
            vec![
                Type::BooleanLiteral(true),
                Type::BooleanLiteral(false),
                Type::Null,
            ]
            .into(),
        );
        let result = narrow_by_truthiness(&ty, false);
        assert_eq!(
            result,
            Type::Union(vec![Type::BooleanLiteral(false), Type::Null].into())
        );
    }

    #[test]
    fn narrow_single_type_truthy() {
        assert_eq!(narrow_by_truthiness(&Type::Null, true), Type::Never);
        assert_eq!(narrow_by_truthiness(&Type::String, true), Type::String);
    }

    #[test]
    fn narrow_instanceof() {
        let class_ty = Type::TypeReference("Foo".into(), vec![].into());
        let other_ty = Type::TypeReference("Bar".into(), vec![].into());
        let ty = Type::Union(vec![class_ty.clone(), other_ty.clone()].into());

        let result = narrow_by_instanceof(&ty, &class_ty);
        assert_eq!(result, class_ty);
    }

    #[test]
    fn narrow_discriminant() {
        let a = Type::ObjectType(crate::ObjectTypeInfo {
            properties: vec![("kind".into(), Arc::new(Type::StringLiteral("a".into())))],
            call_signatures: vec![],
            construct_signatures: vec![],
            index_signature: None,
            index_signature_name: None,
            method_names: Vec::new(),
        });
        let b = Type::ObjectType(crate::ObjectTypeInfo {
            properties: vec![("kind".into(), Arc::new(Type::StringLiteral("b".into())))],
            call_signatures: vec![],
            construct_signatures: vec![],
            index_signature: None,
            index_signature_name: None,
            method_names: Vec::new(),
        });
        let ty = Type::Union(vec![a.clone(), b.clone()].into());

        let result = narrow_by_discriminant(&ty, "kind", &Type::StringLiteral("a".into()));
        assert_eq!(result, a);
    }

    #[test]
    fn exclude_types_from_union() {
        let ty = Type::Union(vec![Type::String, Type::Number, Type::Boolean].into());
        let result = exclude_types(&ty, &[Type::Number]);
        assert_eq!(
            result,
            Type::Union(vec![Type::String, Type::Boolean].into())
        );
    }

    #[test]
    fn exclude_all_gives_never() {
        let ty = Type::Union(vec![Type::String, Type::Number].into());
        let result = exclude_types(&ty, &[Type::String, Type::Number]);
        assert_eq!(result, Type::Never);
    }

    #[test]
    fn exclude_from_single_type() {
        assert_eq!(exclude_types(&Type::String, &[Type::String]), Type::Never);
        assert_eq!(exclude_types(&Type::String, &[Type::Number]), Type::String);
    }

    #[test]
    fn apply_cond_facts_combines() {
        let name = Name::ident("x");
        let ty = Type::Union(vec![Type::String, Type::Number, Type::Null].into());

        let mut facts = CondFacts::new();
        facts.facts.insert(
            name.clone(),
            TypeFacts::NE_NULL | TypeFacts::NE_UNDEFINED | TypeFacts::NE_UNDEFINED_OR_NULL,
        );

        let result = apply_cond_facts(&ty, &name, &facts);
        // Null should be removed (it doesn't satisfy NE_NULL)
        assert_eq!(result, Type::Union(vec![Type::String, Type::Number].into()));
    }

    #[test]
    fn get_type_facts_for_primitives() {
        assert_eq!(get_type_facts(&Type::String), TypeFacts::STRING_FACTS);
        assert_eq!(get_type_facts(&Type::Number), TypeFacts::NUMBER_FACTS);
        assert_eq!(get_type_facts(&Type::Undefined), TypeFacts::UNDEFINED_FACTS);
        assert_eq!(get_type_facts(&Type::Null), TypeFacts::NULL_FACTS);
    }

    #[test]
    fn get_type_facts_for_literals() {
        assert_eq!(
            get_type_facts(&Type::StringLiteral("".into())),
            TypeFacts::EMPTY_STRING_FACTS
        );
        assert_eq!(
            get_type_facts(&Type::StringLiteral("hi".into())),
            TypeFacts::NON_EMPTY_STRING_FACTS
        );
        assert_eq!(
            get_type_facts(&Type::NumberLiteral("0".into())),
            TypeFacts::ZERO_FACTS
        );
        assert_eq!(
            get_type_facts(&Type::NumberLiteral("42".into())),
            TypeFacts::NON_ZERO_FACTS
        );
        assert_eq!(
            get_type_facts(&Type::BooleanLiteral(true)),
            TypeFacts::TRUE_FACTS
        );
        assert_eq!(
            get_type_facts(&Type::BooleanLiteral(false)),
            TypeFacts::FALSE_FACTS
        );
    }

    #[test]
    fn unknown_typeof_returns_original() {
        let ty = Type::String;
        let result = narrow_by_typeof(&ty, "foobar", true);
        assert_eq!(result, Type::String);
    }
}
