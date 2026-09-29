//! Control flow analysis data structures for type narrowing.
//!
//! These structures track type information through branching control flow
//! (if/else, switch, &&, ||, ternary) to enable type narrowing.

use std::sync::Arc;

use std::collections::HashMap;

use crate::type_facts::TypeFacts;
use crate::Type;

// ---------------------------------------------------------------------------
// Name – tracks narrowed variables through property access chains
// ---------------------------------------------------------------------------

/// A name path for tracking narrowed variables.
///
/// Supports dotted paths like `obj.foo.bar` for property narrowing and
/// discriminated union narrowing.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Name {
    /// Simple variable: `x`
    Ident(String),
    /// Property access: `x.foo.bar`
    Member { object: Box<Name>, property: String },
}

impl Name {
    /// Create a simple identifier name.
    pub fn ident(s: impl Into<String>) -> Self {
        Name::Ident(s.into())
    }

    /// Create a member access name from a parent and property.
    pub fn member(obj: Name, prop: impl Into<String>) -> Self {
        Name::Member {
            object: Box::new(obj),
            property: prop.into(),
        }
    }
}

impl std::fmt::Display for Name {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Name::Ident(s) => write!(f, "{}", s),
            Name::Member { object, property } => write!(f, "{}.{}", object, property),
        }
    }
}

// ---------------------------------------------------------------------------
// CondFacts – type information known after evaluating a condition
// ---------------------------------------------------------------------------

/// Conditional facts: type information known to be true after evaluating a
/// condition in one direction (true or false branch).
#[derive(Debug, Clone, Default)]
pub struct CondFacts {
    /// Simple type facts per variable (from typeof, truthiness checks).
    pub facts: HashMap<Name, TypeFacts>,
    /// Narrowed types per variable (from instanceof, equality, type guards).
    pub vars: HashMap<Name, Type>,
    /// Types to exclude from a variable's union (from negative checks).
    pub excludes: HashMap<Name, Vec<Type>>,
}

impl CondFacts {
    pub fn new() -> Self {
        Self::default()
    }

    /// True when no facts have been recorded.
    pub fn is_empty(&self) -> bool {
        self.facts.is_empty() && self.vars.is_empty() && self.excludes.is_empty()
    }

    /// Merge another set of conditional facts by OR (union of branches).
    ///
    /// For OR semantics we keep facts that appear in *either* branch, combining
    /// them with bitwise OR for TypeFacts (conservative widening).
    pub fn or_with(&mut self, other: &CondFacts) {
        for (k, &v) in &other.facts {
            let entry = self.facts.entry(k.clone()).or_insert(TypeFacts::NONE);
            *entry |= v;
        }

        // For narrowed types in an OR context we create unions
        for (k, v) in &other.vars {
            match self.vars.get(k) {
                Some(existing) => {
                    // Already have a narrowed type for this name – build a union
                    let union = Type::Union(vec![existing.clone(), v.clone()].into());
                    self.vars.insert(k.clone(), union);
                }
                None => {
                    self.vars.insert(k.clone(), v.clone());
                }
            }
        }

        for (k, v) in &other.excludes {
            self.excludes
                .entry(k.clone())
                .or_default()
                .extend(v.iter().cloned());
        }
    }

    /// Merge by intersection (AND) – facts from both branches are combined
    /// additively.
    pub fn and_with(&mut self, other: &CondFacts) {
        for (k, &v) in &other.facts {
            let entry = self.facts.entry(k.clone()).or_insert(TypeFacts::NONE);
            *entry |= v;
        }
        for (k, v) in &other.vars {
            self.vars.entry(k.clone()).or_insert_with(|| v.clone());
        }
        for (k, v) in &other.excludes {
            self.excludes
                .entry(k.clone())
                .or_default()
                .extend(v.iter().cloned());
        }
    }

    /// Take all facts out, leaving `self` empty.
    pub fn take(&mut self) -> Self {
        Self {
            facts: std::mem::take(&mut self.facts),
            vars: std::mem::take(&mut self.vars),
            excludes: std::mem::take(&mut self.excludes),
        }
    }

    /// Override narrowed variable types with those from `other`, consuming them.
    pub fn override_vars_using(&mut self, other: &mut CondFacts) {
        for (k, ty) in other.vars.drain() {
            self.vars.insert(k, ty);
        }
    }
}

impl std::ops::AddAssign for CondFacts {
    fn add_assign(&mut self, rhs: Self) {
        self.and_with(&rhs);
    }
}

impl std::ops::BitOr for CondFacts {
    type Output = Self;

    fn bitor(mut self, rhs: Self) -> Self {
        self.or_with(&rhs);
        self
    }
}

// ---------------------------------------------------------------------------
// Facts – a pair of CondFacts for true/false branches
// ---------------------------------------------------------------------------

/// A pair of facts: what is known if a condition is true vs. false.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    pub true_facts: CondFacts,
    pub false_facts: CondFacts,
}

impl Facts {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create facts from `typeof x === type_str`.
    pub fn from_typeof_eq(name: Name, type_str: &str) -> Self {
        let mut true_facts = CondFacts::new();
        let mut false_facts = CondFacts::new();

        if let Some(eq_facts) = TypeFacts::typeof_eq(type_str) {
            true_facts.facts.insert(name.clone(), eq_facts);
        }
        if let Some(ne_facts) = TypeFacts::typeof_ne(type_str) {
            false_facts.facts.insert(name, ne_facts);
        }

        Facts {
            true_facts,
            false_facts,
        }
    }

    /// Create facts from `typeof x !== type_str`.
    pub fn from_typeof_ne(name: Name, type_str: &str) -> Self {
        // Negation of typeof_eq
        Self::from_typeof_eq(name, type_str).negate()
    }

    /// Create facts from `x === null`.
    pub fn from_eq_null(name: Name) -> Self {
        let mut true_facts = CondFacts::new();
        let mut false_facts = CondFacts::new();

        true_facts.facts.insert(
            name.clone(),
            TypeFacts::EQ_NULL | TypeFacts::EQ_UNDEFINED_OR_NULL,
        );
        false_facts.facts.insert(name, TypeFacts::NE_NULL);

        Facts {
            true_facts,
            false_facts,
        }
    }

    /// Create facts from `x !== null`.
    pub fn from_ne_null(name: Name) -> Self {
        Self::from_eq_null(name).negate()
    }

    /// Create facts from `x === undefined`.
    pub fn from_eq_undefined(name: Name) -> Self {
        let mut true_facts = CondFacts::new();
        let mut false_facts = CondFacts::new();

        true_facts.facts.insert(
            name.clone(),
            TypeFacts::EQ_UNDEFINED | TypeFacts::EQ_UNDEFINED_OR_NULL,
        );
        false_facts.facts.insert(name, TypeFacts::NE_UNDEFINED);

        Facts {
            true_facts,
            false_facts,
        }
    }

    /// Create facts from `x !== undefined`.
    pub fn from_ne_undefined(name: Name) -> Self {
        Self::from_eq_undefined(name).negate()
    }

    /// Create facts from `x == null` (loose equality – matches both null and undefined).
    pub fn from_eq_nullish(name: Name) -> Self {
        let mut true_facts = CondFacts::new();
        let mut false_facts = CondFacts::new();

        true_facts.facts.insert(
            name.clone(),
            TypeFacts::EQ_UNDEFINED | TypeFacts::EQ_NULL | TypeFacts::EQ_UNDEFINED_OR_NULL,
        );
        false_facts.facts.insert(
            name,
            TypeFacts::NE_UNDEFINED | TypeFacts::NE_NULL | TypeFacts::NE_UNDEFINED_OR_NULL,
        );

        Facts {
            true_facts,
            false_facts,
        }
    }

    /// Create facts from `x != null` (loose inequality).
    pub fn from_ne_nullish(name: Name) -> Self {
        Self::from_eq_nullish(name).negate()
    }

    /// Create facts from truthiness: `if (x) { ... }`.
    pub fn from_truthy(name: Name) -> Self {
        let mut true_facts = CondFacts::new();
        let mut false_facts = CondFacts::new();

        true_facts.facts.insert(name.clone(), TypeFacts::TRUTHY);
        false_facts.facts.insert(name, TypeFacts::FALSY);

        Facts {
            true_facts,
            false_facts,
        }
    }

    /// Create facts from `x instanceof Foo`.
    ///
    /// In the true branch, `x` is narrowed to `class_type`.
    /// In the false branch, `class_type` is added to excludes.
    pub fn from_instanceof(name: Name, class_type: Type) -> Self {
        let mut true_facts = CondFacts::new();
        let mut false_facts = CondFacts::new();

        true_facts.vars.insert(name.clone(), class_type.clone());
        false_facts
            .excludes
            .entry(name)
            .or_default()
            .push(class_type);

        Facts {
            true_facts,
            false_facts,
        }
    }

    /// Negate: swap true and false facts.
    pub fn negate(self) -> Self {
        Facts {
            true_facts: self.false_facts,
            false_facts: self.true_facts,
        }
    }

    /// Take all facts out, leaving `self` empty.
    pub fn take(&mut self) -> Self {
        Self {
            true_facts: self.true_facts.take(),
            false_facts: self.false_facts.take(),
        }
    }
}

impl std::ops::Not for Facts {
    type Output = Self;

    #[inline]
    fn not(self) -> Self {
        self.negate()
    }
}

impl std::ops::AddAssign for Facts {
    fn add_assign(&mut self, rhs: Self) {
        self.true_facts += rhs.true_facts;
        self.false_facts += rhs.false_facts;
    }
}

impl std::ops::BitOr for Facts {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Facts {
            true_facts: self.true_facts | rhs.true_facts,
            false_facts: self.false_facts | rhs.false_facts,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_display() {
        let n = Name::member(Name::ident("obj"), "foo");
        assert_eq!(format!("{}", n), "obj.foo");

        let deep = Name::member(Name::member(Name::ident("a"), "b"), "c");
        assert_eq!(format!("{}", deep), "a.b.c");
    }

    #[test]
    fn cond_facts_empty_by_default() {
        let cf = CondFacts::new();
        assert!(cf.is_empty());
    }

    #[test]
    fn and_with_combines_facts() {
        let name = Name::ident("x");

        let mut a = CondFacts::new();
        a.facts.insert(name.clone(), TypeFacts::TRUTHY);

        let mut b = CondFacts::new();
        b.facts.insert(name.clone(), TypeFacts::NE_NULL);

        a.and_with(&b);

        let combined = a.facts.get(&name).copied().unwrap();
        assert!(combined.contains(TypeFacts::TRUTHY));
        assert!(combined.contains(TypeFacts::NE_NULL));
    }

    #[test]
    fn or_with_combines_facts() {
        let name = Name::ident("x");

        let mut a = CondFacts::new();
        a.facts.insert(name.clone(), TypeFacts::TYPEOF_EQ_STRING);

        let mut b = CondFacts::new();
        b.facts.insert(name.clone(), TypeFacts::TYPEOF_EQ_NUMBER);

        a.or_with(&b);

        let combined = a.facts.get(&name).copied().unwrap();
        assert!(combined.contains(TypeFacts::TYPEOF_EQ_STRING));
        assert!(combined.contains(TypeFacts::TYPEOF_EQ_NUMBER));
    }

    #[test]
    fn facts_typeof_eq() {
        let name = Name::ident("x");
        let facts = Facts::from_typeof_eq(name.clone(), "string");

        let true_f = facts.true_facts.facts.get(&name).copied().unwrap();
        assert!(true_f.contains(TypeFacts::TYPEOF_EQ_STRING));

        let false_f = facts.false_facts.facts.get(&name).copied().unwrap();
        assert!(false_f.contains(TypeFacts::TYPEOF_NE_STRING));
    }

    #[test]
    fn facts_typeof_ne_is_negation() {
        let name = Name::ident("x");
        let eq = Facts::from_typeof_eq(name.clone(), "number");
        let ne = Facts::from_typeof_ne(name.clone(), "number");

        // typeof_ne true branch should equal typeof_eq false branch
        assert_eq!(
            ne.true_facts.facts.get(&name),
            eq.false_facts.facts.get(&name)
        );
        assert_eq!(
            ne.false_facts.facts.get(&name),
            eq.true_facts.facts.get(&name)
        );
    }

    #[test]
    fn facts_truthy() {
        let name = Name::ident("x");
        let facts = Facts::from_truthy(name.clone());

        assert!(facts
            .true_facts
            .facts
            .get(&name)
            .unwrap()
            .contains(TypeFacts::TRUTHY));
        assert!(facts
            .false_facts
            .facts
            .get(&name)
            .unwrap()
            .contains(TypeFacts::FALSY));
    }

    #[test]
    fn facts_negate_swaps() {
        let name = Name::ident("x");
        let facts = Facts::from_truthy(name.clone());
        let neg = facts.negate();

        assert!(neg
            .true_facts
            .facts
            .get(&name)
            .unwrap()
            .contains(TypeFacts::FALSY));
        assert!(neg
            .false_facts
            .facts
            .get(&name)
            .unwrap()
            .contains(TypeFacts::TRUTHY));
    }

    #[test]
    fn facts_not_operator() {
        let name = Name::ident("x");
        let facts = Facts::from_truthy(name.clone());
        let neg = !facts;

        assert!(neg
            .true_facts
            .facts
            .get(&name)
            .unwrap()
            .contains(TypeFacts::FALSY));
    }

    #[test]
    fn facts_instanceof() {
        let name = Name::ident("x");
        let class_ty = Type::TypeReference("Foo".into(), vec![].into());
        let facts = Facts::from_instanceof(name.clone(), class_ty.clone());

        assert_eq!(facts.true_facts.vars.get(&name), Some(&class_ty));
        assert!(facts
            .false_facts
            .excludes
            .get(&name)
            .unwrap()
            .contains(&class_ty));
    }

    #[test]
    fn facts_eq_null() {
        let name = Name::ident("x");
        let facts = Facts::from_eq_null(name.clone());

        let true_f = facts.true_facts.facts.get(&name).copied().unwrap();
        assert!(true_f.contains(TypeFacts::EQ_NULL));
        assert!(true_f.contains(TypeFacts::EQ_UNDEFINED_OR_NULL));

        let false_f = facts.false_facts.facts.get(&name).copied().unwrap();
        assert!(false_f.contains(TypeFacts::NE_NULL));
    }

    #[test]
    fn facts_eq_nullish() {
        let name = Name::ident("x");
        let facts = Facts::from_eq_nullish(name.clone());

        let true_f = facts.true_facts.facts.get(&name).copied().unwrap();
        assert!(true_f.contains(TypeFacts::EQ_UNDEFINED));
        assert!(true_f.contains(TypeFacts::EQ_NULL));
        assert!(true_f.contains(TypeFacts::EQ_UNDEFINED_OR_NULL));

        let false_f = facts.false_facts.facts.get(&name).copied().unwrap();
        assert!(false_f.contains(TypeFacts::NE_UNDEFINED));
        assert!(false_f.contains(TypeFacts::NE_NULL));
        assert!(false_f.contains(TypeFacts::NE_UNDEFINED_OR_NULL));
    }

    #[test]
    fn facts_add_assign() {
        let x = Name::ident("x");
        let y = Name::ident("y");

        let mut a = Facts::from_truthy(x.clone());
        let b = Facts::from_typeof_eq(y.clone(), "string");
        a += b;

        assert!(a.true_facts.facts.contains_key(&x));
        assert!(a.true_facts.facts.contains_key(&y));
    }

    #[test]
    fn facts_bitor() {
        let x = Name::ident("x");
        let y = Name::ident("y");

        let a = Facts::from_truthy(x.clone());
        let b = Facts::from_truthy(y.clone());
        let combined = a | b;

        assert!(combined.true_facts.facts.contains_key(&x));
        assert!(combined.true_facts.facts.contains_key(&y));
    }

    #[test]
    fn cond_facts_take_empties() {
        let name = Name::ident("x");
        let mut cf = CondFacts::new();
        cf.facts.insert(name.clone(), TypeFacts::TRUTHY);

        let taken = cf.take();
        assert!(cf.is_empty());
        assert!(!taken.is_empty());
        assert!(taken.facts.contains_key(&name));
    }

    #[test]
    fn cond_facts_override_vars() {
        let name = Name::ident("x");

        let mut base = CondFacts::new();
        base.vars.insert(name.clone(), Type::String);

        let mut other = CondFacts::new();
        other.vars.insert(name.clone(), Type::Number);

        base.override_vars_using(&mut other);

        assert_eq!(base.vars.get(&name), Some(&Type::Number));
        assert!(other.vars.is_empty()); // drained
    }
}
