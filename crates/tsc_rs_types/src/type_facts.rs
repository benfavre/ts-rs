use std::sync::Arc;

/// Bitflag representation of type facts used for type narrowing.
///
/// Each flag represents a known fact about a value (e.g. "typeof x === 'string'").
/// Combinations of flags describe what is known about a type after a narrowing guard.
///
/// This is a standalone data structure with no dependency on the `Type` enum.
#[derive(Copy, Clone, Default, Eq, PartialEq, Hash)]
pub struct TypeFacts(u32);

// ---------------------------------------------------------------------------
// Individual flags
// ---------------------------------------------------------------------------

impl TypeFacts {
    pub const NONE: Self = Self(0);

    // typeof equality flags (bits 0..7)
    /// typeof x === "string"
    pub const TYPEOF_EQ_STRING: Self = Self(1 << 0);
    /// typeof x === "number"
    pub const TYPEOF_EQ_NUMBER: Self = Self(1 << 1);
    /// typeof x === "bigint"
    pub const TYPEOF_EQ_BIGINT: Self = Self(1 << 2);
    /// typeof x === "boolean"
    pub const TYPEOF_EQ_BOOLEAN: Self = Self(1 << 3);
    /// typeof x === "symbol"
    pub const TYPEOF_EQ_SYMBOL: Self = Self(1 << 4);
    /// typeof x === "object"
    pub const TYPEOF_EQ_OBJECT: Self = Self(1 << 5);
    /// typeof x === "function"
    pub const TYPEOF_EQ_FUNCTION: Self = Self(1 << 6);
    /// typeof x === (host-defined)
    pub const TYPEOF_EQ_HOST_OBJECT: Self = Self(1 << 7);

    // typeof inequality flags (bits 8..15)
    /// typeof x !== "string"
    pub const TYPEOF_NE_STRING: Self = Self(1 << 8);
    /// typeof x !== "number"
    pub const TYPEOF_NE_NUMBER: Self = Self(1 << 9);
    /// typeof x !== "bigint"
    pub const TYPEOF_NE_BIGINT: Self = Self(1 << 10);
    /// typeof x !== "boolean"
    pub const TYPEOF_NE_BOOLEAN: Self = Self(1 << 11);
    /// typeof x !== "symbol"
    pub const TYPEOF_NE_SYMBOL: Self = Self(1 << 12);
    /// typeof x !== "object"
    pub const TYPEOF_NE_OBJECT: Self = Self(1 << 13);
    /// typeof x !== "function"
    pub const TYPEOF_NE_FUNCTION: Self = Self(1 << 14);
    /// typeof x !== (host-defined)
    pub const TYPEOF_NE_HOST_OBJECT: Self = Self(1 << 15);

    // Null / undefined equality flags (bits 16..21)
    /// x === undefined
    pub const EQ_UNDEFINED: Self = Self(1 << 16);
    /// x === null
    pub const EQ_NULL: Self = Self(1 << 17);
    /// x == undefined / x == null  (loose equality)
    pub const EQ_UNDEFINED_OR_NULL: Self = Self(1 << 18);
    /// x !== undefined
    pub const NE_UNDEFINED: Self = Self(1 << 19);
    /// x !== null
    pub const NE_NULL: Self = Self(1 << 20);
    /// x != undefined / x != null  (loose inequality)
    pub const NE_UNDEFINED_OR_NULL: Self = Self(1 << 21);

    // Truthiness flags (bits 22..23)
    /// x  (truthy)
    pub const TRUTHY: Self = Self(1 << 22);
    /// !x  (falsy)
    pub const FALSY: Self = Self(1 << 23);

    /// All 24 flags set.
    pub const ALL: Self = Self((1 << 24) - 1);
}

// ---------------------------------------------------------------------------
// Pre-computed fact combinations for primitive types
// ---------------------------------------------------------------------------

impl TypeFacts {
    // -- String ---------------------------------------------------------------

    /// Base strict facts for any string value (no truthiness, no nullable eq).
    pub const BASE_STRING_STRICT_FACTS: Self = Self(
        Self::TYPEOF_EQ_STRING.0
            | Self::TYPEOF_NE_NUMBER.0
            | Self::TYPEOF_NE_BIGINT.0
            | Self::TYPEOF_NE_BOOLEAN.0
            | Self::TYPEOF_NE_SYMBOL.0
            | Self::TYPEOF_NE_OBJECT.0
            | Self::TYPEOF_NE_FUNCTION.0
            | Self::TYPEOF_NE_HOST_OBJECT.0
            | Self::NE_UNDEFINED.0
            | Self::NE_NULL.0
            | Self::NE_UNDEFINED_OR_NULL.0,
    );

    /// Base facts for a possibly-nullable string (strict + nullable eq + falsy).
    pub const BASE_STRING_FACTS: Self = Self(
        Self::BASE_STRING_STRICT_FACTS.0
            | Self::EQ_UNDEFINED.0
            | Self::EQ_NULL.0
            | Self::EQ_UNDEFINED_OR_NULL.0
            | Self::FALSY.0,
    );

    /// A string that may or may not be empty (strict context).
    pub const STRING_STRICT_FACTS: Self =
        Self(Self::BASE_STRING_STRICT_FACTS.0 | Self::TRUTHY.0 | Self::FALSY.0);

    /// A string that may or may not be empty (non-strict / nullable context).
    pub const STRING_FACTS: Self = Self(Self::BASE_STRING_FACTS.0 | Self::TRUTHY.0);

    /// The empty string `""` (strict context).
    pub const EMPTY_STRING_STRICT_FACTS: Self =
        Self(Self::BASE_STRING_STRICT_FACTS.0 | Self::FALSY.0);

    /// The empty string `""` (non-strict / nullable context).
    pub const EMPTY_STRING_FACTS: Self = Self(Self::BASE_STRING_FACTS.0);

    /// A non-empty string (strict context).
    pub const NON_EMPTY_STRING_STRICT_FACTS: Self =
        Self(Self::BASE_STRING_STRICT_FACTS.0 | Self::TRUTHY.0);

    /// A non-empty string (non-strict / nullable context).
    pub const NON_EMPTY_STRING_FACTS: Self = Self(Self::BASE_STRING_FACTS.0 | Self::TRUTHY.0);

    // -- Number ---------------------------------------------------------------

    pub const BASE_NUMBER_STRICT_FACTS: Self = Self(
        Self::TYPEOF_EQ_NUMBER.0
            | Self::TYPEOF_NE_STRING.0
            | Self::TYPEOF_NE_BIGINT.0
            | Self::TYPEOF_NE_BOOLEAN.0
            | Self::TYPEOF_NE_SYMBOL.0
            | Self::TYPEOF_NE_OBJECT.0
            | Self::TYPEOF_NE_FUNCTION.0
            | Self::TYPEOF_NE_HOST_OBJECT.0
            | Self::NE_UNDEFINED.0
            | Self::NE_NULL.0
            | Self::NE_UNDEFINED_OR_NULL.0,
    );

    pub const BASE_NUMBER_FACTS: Self = Self(
        Self::BASE_NUMBER_STRICT_FACTS.0
            | Self::EQ_UNDEFINED.0
            | Self::EQ_NULL.0
            | Self::EQ_UNDEFINED_OR_NULL.0
            | Self::FALSY.0,
    );

    pub const NUMBER_STRICT_FACTS: Self =
        Self(Self::BASE_NUMBER_STRICT_FACTS.0 | Self::TRUTHY.0 | Self::FALSY.0);

    pub const NUMBER_FACTS: Self = Self(Self::BASE_NUMBER_FACTS.0 | Self::TRUTHY.0);

    pub const ZERO_STRICT_FACTS: Self = Self(Self::BASE_NUMBER_STRICT_FACTS.0 | Self::FALSY.0);

    pub const ZERO_FACTS: Self = Self(Self::BASE_NUMBER_FACTS.0);

    pub const NON_ZERO_STRICT_FACTS: Self = Self(Self::BASE_NUMBER_STRICT_FACTS.0 | Self::TRUTHY.0);

    pub const NON_ZERO_FACTS: Self = Self(Self::BASE_NUMBER_FACTS.0 | Self::TRUTHY.0);

    // -- BigInt ---------------------------------------------------------------

    pub const BASE_BIGINT_STRICT_FACTS: Self = Self(
        Self::TYPEOF_EQ_BIGINT.0
            | Self::TYPEOF_NE_STRING.0
            | Self::TYPEOF_NE_NUMBER.0
            | Self::TYPEOF_NE_BOOLEAN.0
            | Self::TYPEOF_NE_SYMBOL.0
            | Self::TYPEOF_NE_OBJECT.0
            | Self::TYPEOF_NE_FUNCTION.0
            | Self::TYPEOF_NE_HOST_OBJECT.0
            | Self::NE_UNDEFINED.0
            | Self::NE_NULL.0
            | Self::NE_UNDEFINED_OR_NULL.0,
    );

    pub const BASE_BIGINT_FACTS: Self = Self(
        Self::BASE_BIGINT_STRICT_FACTS.0
            | Self::EQ_UNDEFINED.0
            | Self::EQ_NULL.0
            | Self::EQ_UNDEFINED_OR_NULL.0
            | Self::FALSY.0,
    );

    pub const BIGINT_STRICT_FACTS: Self =
        Self(Self::BASE_BIGINT_STRICT_FACTS.0 | Self::TRUTHY.0 | Self::FALSY.0);

    pub const BIGINT_FACTS: Self = Self(Self::BASE_BIGINT_FACTS.0 | Self::TRUTHY.0);

    pub const ZERO_BIGINT_STRICT_FACTS: Self =
        Self(Self::BASE_BIGINT_STRICT_FACTS.0 | Self::FALSY.0);

    pub const ZERO_BIGINT_FACTS: Self = Self(Self::BASE_BIGINT_FACTS.0);

    pub const NON_ZERO_BIGINT_STRICT_FACTS: Self =
        Self(Self::BASE_BIGINT_STRICT_FACTS.0 | Self::TRUTHY.0);

    pub const NON_ZERO_BIGINT_FACTS: Self = Self(Self::BASE_BIGINT_FACTS.0 | Self::TRUTHY.0);

    // -- Boolean --------------------------------------------------------------

    pub const BASE_BOOLEAN_STRICT_FACTS: Self = Self(
        Self::TYPEOF_EQ_BOOLEAN.0
            | Self::TYPEOF_NE_STRING.0
            | Self::TYPEOF_NE_NUMBER.0
            | Self::TYPEOF_NE_BIGINT.0
            | Self::TYPEOF_NE_SYMBOL.0
            | Self::TYPEOF_NE_OBJECT.0
            | Self::TYPEOF_NE_FUNCTION.0
            | Self::TYPEOF_NE_HOST_OBJECT.0
            | Self::NE_UNDEFINED.0
            | Self::NE_NULL.0
            | Self::NE_UNDEFINED_OR_NULL.0,
    );

    pub const BASE_BOOLEAN_FACTS: Self = Self(
        Self::BASE_BOOLEAN_STRICT_FACTS.0
            | Self::EQ_UNDEFINED.0
            | Self::EQ_NULL.0
            | Self::EQ_UNDEFINED_OR_NULL.0
            | Self::FALSY.0,
    );

    pub const BOOLEAN_STRICT_FACTS: Self =
        Self(Self::BASE_BOOLEAN_STRICT_FACTS.0 | Self::TRUTHY.0 | Self::FALSY.0);

    pub const BOOLEAN_FACTS: Self = Self(Self::BASE_BOOLEAN_FACTS.0 | Self::TRUTHY.0);

    pub const FALSE_STRICT_FACTS: Self = Self(Self::BASE_BOOLEAN_STRICT_FACTS.0 | Self::FALSY.0);

    pub const FALSE_FACTS: Self = Self(Self::BASE_BOOLEAN_FACTS.0);

    pub const TRUE_STRICT_FACTS: Self = Self(Self::BASE_BOOLEAN_STRICT_FACTS.0 | Self::TRUTHY.0);

    pub const TRUE_FACTS: Self = Self(Self::BASE_BOOLEAN_FACTS.0 | Self::TRUTHY.0);

    // -- Symbol ---------------------------------------------------------------

    pub const SYMBOL_STRICT_FACTS: Self = Self(
        Self::TYPEOF_EQ_SYMBOL.0
            | Self::TYPEOF_NE_STRING.0
            | Self::TYPEOF_NE_NUMBER.0
            | Self::TYPEOF_NE_BIGINT.0
            | Self::TYPEOF_NE_BOOLEAN.0
            | Self::TYPEOF_NE_OBJECT.0
            | Self::TYPEOF_NE_FUNCTION.0
            | Self::TYPEOF_NE_HOST_OBJECT.0
            | Self::NE_UNDEFINED.0
            | Self::NE_NULL.0
            | Self::NE_UNDEFINED_OR_NULL.0
            | Self::TRUTHY.0,
    );

    pub const SYMBOL_FACTS: Self = Self(
        Self::SYMBOL_STRICT_FACTS.0
            | Self::EQ_UNDEFINED.0
            | Self::EQ_NULL.0
            | Self::EQ_UNDEFINED_OR_NULL.0
            | Self::FALSY.0,
    );

    // -- Object ---------------------------------------------------------------

    pub const OBJECT_STRICT_FACTS: Self = Self(
        Self::TYPEOF_EQ_OBJECT.0
            | Self::TYPEOF_EQ_HOST_OBJECT.0
            | Self::TYPEOF_NE_STRING.0
            | Self::TYPEOF_NE_NUMBER.0
            | Self::TYPEOF_NE_BIGINT.0
            | Self::TYPEOF_NE_BOOLEAN.0
            | Self::TYPEOF_NE_SYMBOL.0
            | Self::TYPEOF_NE_FUNCTION.0
            | Self::NE_UNDEFINED.0
            | Self::NE_NULL.0
            | Self::NE_UNDEFINED_OR_NULL.0
            | Self::TRUTHY.0,
    );

    pub const OBJECT_FACTS: Self = Self(
        Self::OBJECT_STRICT_FACTS.0
            | Self::EQ_UNDEFINED.0
            | Self::EQ_NULL.0
            | Self::EQ_UNDEFINED_OR_NULL.0
            | Self::FALSY.0,
    );

    // -- Function -------------------------------------------------------------

    pub const FUNCTION_STRICT_FACTS: Self = Self(
        Self::TYPEOF_EQ_FUNCTION.0
            | Self::TYPEOF_EQ_HOST_OBJECT.0
            | Self::TYPEOF_NE_STRING.0
            | Self::TYPEOF_NE_NUMBER.0
            | Self::TYPEOF_NE_BIGINT.0
            | Self::TYPEOF_NE_BOOLEAN.0
            | Self::TYPEOF_NE_SYMBOL.0
            | Self::TYPEOF_NE_OBJECT.0
            | Self::NE_UNDEFINED.0
            | Self::NE_NULL.0
            | Self::NE_UNDEFINED_OR_NULL.0
            | Self::TRUTHY.0,
    );

    pub const FUNCTION_FACTS: Self = Self(
        Self::FUNCTION_STRICT_FACTS.0
            | Self::EQ_UNDEFINED.0
            | Self::EQ_NULL.0
            | Self::EQ_UNDEFINED_OR_NULL.0
            | Self::FALSY.0,
    );

    // -- Undefined ------------------------------------------------------------

    pub const UNDEFINED_FACTS: Self = Self(
        Self::TYPEOF_NE_STRING.0
            | Self::TYPEOF_NE_NUMBER.0
            | Self::TYPEOF_NE_BIGINT.0
            | Self::TYPEOF_NE_BOOLEAN.0
            | Self::TYPEOF_NE_SYMBOL.0
            | Self::TYPEOF_NE_OBJECT.0
            | Self::TYPEOF_NE_FUNCTION.0
            | Self::TYPEOF_NE_HOST_OBJECT.0
            | Self::EQ_UNDEFINED.0
            | Self::EQ_UNDEFINED_OR_NULL.0
            | Self::NE_NULL.0
            | Self::FALSY.0,
    );

    // -- Null -----------------------------------------------------------------

    pub const NULL_FACTS: Self = Self(
        Self::TYPEOF_EQ_OBJECT.0
            | Self::TYPEOF_NE_STRING.0
            | Self::TYPEOF_NE_NUMBER.0
            | Self::TYPEOF_NE_BIGINT.0
            | Self::TYPEOF_NE_BOOLEAN.0
            | Self::TYPEOF_NE_SYMBOL.0
            | Self::TYPEOF_NE_FUNCTION.0
            | Self::TYPEOF_NE_HOST_OBJECT.0
            | Self::EQ_NULL.0
            | Self::EQ_UNDEFINED_OR_NULL.0
            | Self::NE_UNDEFINED.0
            | Self::FALSY.0,
    );

    // -- Void  (same as Undefined in terms of facts) --------------------------

    pub const VOID_FACTS: Self = Self(Self::UNDEFINED_FACTS.0);

    // -- Empty object ---------------------------------------------------------

    pub const EMPTY_OBJECT_STRICT_FACTS: Self = Self(
        Self::ALL.0 & !(Self::EQ_UNDEFINED.0 | Self::EQ_NULL.0 | Self::EQ_UNDEFINED_OR_NULL.0),
    );

    pub const EMPTY_OBJECT_FACTS: Self = Self::ALL;
}

// ---------------------------------------------------------------------------
// Helper methods
// ---------------------------------------------------------------------------

impl TypeFacts {
    /// Construct from raw bits.
    #[inline]
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    /// Return the underlying bit representation.
    #[inline]
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// True when no flags are set.
    #[inline]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// True when `self` has at least one flag in common with `other`.
    #[inline]
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// True when `self` is a superset of `other` (contains all flags in `other`).
    #[inline]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Return the narrowed type facts for `typeof x === s`.
    ///
    /// Returns `None` for unknown typeof strings.
    pub fn typeof_eq(s: &str) -> Option<Self> {
        Some(match s {
            "string" => Self::BASE_STRING_STRICT_FACTS,
            "number" => Self::BASE_NUMBER_STRICT_FACTS,
            "bigint" => Self::BASE_BIGINT_STRICT_FACTS,
            "boolean" => Self::BASE_BOOLEAN_STRICT_FACTS,
            "symbol" => Self::SYMBOL_STRICT_FACTS,
            "undefined" => Self::EQ_UNDEFINED,
            "object" => Self(
                Self::TYPEOF_EQ_OBJECT.0
                    | Self::TYPEOF_NE_STRING.0
                    | Self::TYPEOF_NE_NUMBER.0
                    | Self::TYPEOF_NE_BIGINT.0
                    | Self::TYPEOF_NE_BOOLEAN.0
                    | Self::TYPEOF_NE_SYMBOL.0
                    | Self::TYPEOF_NE_FUNCTION.0
                    | Self::NE_UNDEFINED.0,
            ),
            "function" => Self(
                Self::TYPEOF_EQ_FUNCTION.0
                    | Self::TYPEOF_NE_STRING.0
                    | Self::TYPEOF_NE_NUMBER.0
                    | Self::TYPEOF_NE_BIGINT.0
                    | Self::TYPEOF_NE_BOOLEAN.0
                    | Self::TYPEOF_NE_SYMBOL.0
                    | Self::TYPEOF_NE_OBJECT.0
                    | Self::NE_UNDEFINED.0
                    | Self::NE_NULL.0
                    | Self::NE_UNDEFINED_OR_NULL.0
                    | Self::TRUTHY.0,
            ),
            _ => return None,
        })
    }

    /// Return the narrowed type facts for `typeof x !== s`.
    ///
    /// Returns `None` for unknown typeof strings.
    pub fn typeof_ne(s: &str) -> Option<Self> {
        Some(match s {
            "string" => Self::TYPEOF_NE_STRING,
            "number" => Self::TYPEOF_NE_NUMBER,
            "bigint" => Self::TYPEOF_NE_BIGINT,
            "boolean" => Self::TYPEOF_NE_BOOLEAN,
            "symbol" => Self::TYPEOF_NE_SYMBOL,
            "undefined" => Self::NE_UNDEFINED,
            "object" => Self::TYPEOF_NE_OBJECT,
            "function" => Self::TYPEOF_NE_FUNCTION,
            _ => return None,
        })
    }
}

// ---------------------------------------------------------------------------
// Bitwise operator impls
// ---------------------------------------------------------------------------

impl std::ops::BitAnd for TypeFacts {
    type Output = Self;
    #[inline]
    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

impl std::ops::BitAndAssign for TypeFacts {
    #[inline]
    fn bitand_assign(&mut self, rhs: Self) {
        self.0 &= rhs.0;
    }
}

impl std::ops::BitOr for TypeFacts {
    type Output = Self;
    #[inline]
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for TypeFacts {
    #[inline]
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl std::ops::Not for TypeFacts {
    type Output = Self;
    #[inline]
    fn not(self) -> Self {
        // Mask to ALL so we don't set bits beyond the 24-bit range.
        Self(!self.0 & Self::ALL.0)
    }
}

// ---------------------------------------------------------------------------
// Debug / Display
// ---------------------------------------------------------------------------

impl std::fmt::Debug for TypeFacts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_empty() {
            return write!(f, "TypeFacts::NONE");
        }

        let flag_names: &[(Self, &str)] = &[
            (Self::TYPEOF_EQ_STRING, "TypeofEqString"),
            (Self::TYPEOF_EQ_NUMBER, "TypeofEqNumber"),
            (Self::TYPEOF_EQ_BIGINT, "TypeofEqBigint"),
            (Self::TYPEOF_EQ_BOOLEAN, "TypeofEqBoolean"),
            (Self::TYPEOF_EQ_SYMBOL, "TypeofEqSymbol"),
            (Self::TYPEOF_EQ_OBJECT, "TypeofEqObject"),
            (Self::TYPEOF_EQ_FUNCTION, "TypeofEqFunction"),
            (Self::TYPEOF_EQ_HOST_OBJECT, "TypeofEqHostObject"),
            (Self::TYPEOF_NE_STRING, "TypeofNeString"),
            (Self::TYPEOF_NE_NUMBER, "TypeofNeNumber"),
            (Self::TYPEOF_NE_BIGINT, "TypeofNeBigint"),
            (Self::TYPEOF_NE_BOOLEAN, "TypeofNeBoolean"),
            (Self::TYPEOF_NE_SYMBOL, "TypeofNeSymbol"),
            (Self::TYPEOF_NE_OBJECT, "TypeofNeObject"),
            (Self::TYPEOF_NE_FUNCTION, "TypeofNeFunction"),
            (Self::TYPEOF_NE_HOST_OBJECT, "TypeofNeHostObject"),
            (Self::EQ_UNDEFINED, "EqUndefined"),
            (Self::EQ_NULL, "EqNull"),
            (Self::EQ_UNDEFINED_OR_NULL, "EqUndefinedOrNull"),
            (Self::NE_UNDEFINED, "NeUndefined"),
            (Self::NE_NULL, "NeNull"),
            (Self::NE_UNDEFINED_OR_NULL, "NeUndefinedOrNull"),
            (Self::TRUTHY, "Truthy"),
            (Self::FALSY, "Falsy"),
        ];

        let mut first = true;
        write!(f, "TypeFacts(")?;
        for &(flag, name) in flag_names {
            if self.contains(flag) {
                if !first {
                    write!(f, " | ")?;
                }
                write!(f, "{}", name)?;
                first = false;
            }
        }
        write!(f, ")")
    }
}

impl std::fmt::Display for TypeFacts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_is_empty() {
        assert!(TypeFacts::NONE.is_empty());
        assert_eq!(TypeFacts::NONE.bits(), 0);
    }

    #[test]
    fn all_has_24_bits() {
        assert_eq!(TypeFacts::ALL.bits(), (1u32 << 24) - 1);
    }

    #[test]
    fn bitor_combines_flags() {
        let combined = TypeFacts::TYPEOF_EQ_STRING | TypeFacts::TRUTHY;
        assert!(combined.contains(TypeFacts::TYPEOF_EQ_STRING));
        assert!(combined.contains(TypeFacts::TRUTHY));
        assert!(!combined.contains(TypeFacts::FALSY));
    }

    #[test]
    fn bitand_intersects_flags() {
        let a = TypeFacts::TYPEOF_EQ_STRING | TypeFacts::TRUTHY | TypeFacts::NE_NULL;
        let b = TypeFacts::TRUTHY | TypeFacts::NE_NULL | TypeFacts::FALSY;
        let c = a & b;
        assert!(c.contains(TypeFacts::TRUTHY));
        assert!(c.contains(TypeFacts::NE_NULL));
        assert!(!c.contains(TypeFacts::TYPEOF_EQ_STRING));
        assert!(!c.contains(TypeFacts::FALSY));
    }

    #[test]
    fn not_inverts_within_all() {
        let a = TypeFacts::TYPEOF_EQ_STRING;
        let b = !a;
        assert!(!b.contains(TypeFacts::TYPEOF_EQ_STRING));
        assert!(b.contains(TypeFacts::TYPEOF_NE_STRING));
        assert!(b.contains(TypeFacts::TRUTHY));
        // No bits beyond ALL are set.
        assert_eq!(b.bits() & !TypeFacts::ALL.bits(), 0);
    }

    #[test]
    fn string_facts_include_correct_typeof() {
        assert!(TypeFacts::BASE_STRING_STRICT_FACTS.contains(TypeFacts::TYPEOF_EQ_STRING));
        assert!(!TypeFacts::BASE_STRING_STRICT_FACTS.contains(TypeFacts::TYPEOF_EQ_NUMBER));
        assert!(TypeFacts::BASE_STRING_STRICT_FACTS.contains(TypeFacts::TYPEOF_NE_NUMBER));
        assert!(TypeFacts::BASE_STRING_STRICT_FACTS.contains(TypeFacts::NE_UNDEFINED));
        assert!(TypeFacts::BASE_STRING_STRICT_FACTS.contains(TypeFacts::NE_NULL));
    }

    #[test]
    fn empty_string_is_falsy() {
        assert!(TypeFacts::EMPTY_STRING_STRICT_FACTS.contains(TypeFacts::FALSY));
        assert!(!TypeFacts::EMPTY_STRING_STRICT_FACTS.contains(TypeFacts::TRUTHY));
    }

    #[test]
    fn non_empty_string_is_truthy() {
        assert!(TypeFacts::NON_EMPTY_STRING_STRICT_FACTS.contains(TypeFacts::TRUTHY));
        assert!(!TypeFacts::NON_EMPTY_STRING_STRICT_FACTS.contains(TypeFacts::FALSY));
    }

    #[test]
    fn undefined_facts_correct() {
        assert!(TypeFacts::UNDEFINED_FACTS.contains(TypeFacts::EQ_UNDEFINED));
        assert!(TypeFacts::UNDEFINED_FACTS.contains(TypeFacts::EQ_UNDEFINED_OR_NULL));
        assert!(TypeFacts::UNDEFINED_FACTS.contains(TypeFacts::NE_NULL));
        assert!(TypeFacts::UNDEFINED_FACTS.contains(TypeFacts::FALSY));
        assert!(!TypeFacts::UNDEFINED_FACTS.contains(TypeFacts::TRUTHY));
        // undefined has no typeof equality flag (typeof undefined === "undefined" is
        // handled differently)
        assert!(!TypeFacts::UNDEFINED_FACTS.contains(TypeFacts::TYPEOF_EQ_STRING));
    }

    #[test]
    fn null_facts_correct() {
        assert!(TypeFacts::NULL_FACTS.contains(TypeFacts::EQ_NULL));
        assert!(TypeFacts::NULL_FACTS.contains(TypeFacts::EQ_UNDEFINED_OR_NULL));
        assert!(TypeFacts::NULL_FACTS.contains(TypeFacts::NE_UNDEFINED));
        assert!(TypeFacts::NULL_FACTS.contains(TypeFacts::FALSY));
        // typeof null === "object"
        assert!(TypeFacts::NULL_FACTS.contains(TypeFacts::TYPEOF_EQ_OBJECT));
    }

    #[test]
    fn typeof_eq_returns_correct_facts() {
        assert_eq!(
            TypeFacts::typeof_eq("string"),
            Some(TypeFacts::BASE_STRING_STRICT_FACTS)
        );
        assert_eq!(
            TypeFacts::typeof_eq("boolean"),
            Some(TypeFacts::BASE_BOOLEAN_STRICT_FACTS)
        );
        assert_eq!(
            TypeFacts::typeof_eq("undefined"),
            Some(TypeFacts::EQ_UNDEFINED)
        );
        assert_eq!(TypeFacts::typeof_eq("blah"), None);
    }

    #[test]
    fn typeof_ne_returns_correct_facts() {
        assert_eq!(
            TypeFacts::typeof_ne("string"),
            Some(TypeFacts::TYPEOF_NE_STRING)
        );
        assert_eq!(
            TypeFacts::typeof_ne("undefined"),
            Some(TypeFacts::NE_UNDEFINED)
        );
        assert_eq!(TypeFacts::typeof_ne("blah"), None);
    }

    #[test]
    fn intersects_works() {
        let a = TypeFacts::TYPEOF_EQ_STRING | TypeFacts::TRUTHY;
        let b = TypeFacts::TRUTHY | TypeFacts::FALSY;
        assert!(a.intersects(b));
        assert!(!TypeFacts::TYPEOF_EQ_STRING.intersects(TypeFacts::FALSY));
    }

    #[test]
    fn display_shows_flags() {
        let f = TypeFacts::TYPEOF_EQ_STRING | TypeFacts::TRUTHY;
        let s = format!("{}", f);
        assert!(s.contains("TypeofEqString"));
        assert!(s.contains("Truthy"));
    }

    #[test]
    fn bitand_assign_works() {
        let mut a = TypeFacts::TYPEOF_EQ_STRING | TypeFacts::TRUTHY | TypeFacts::NE_NULL;
        a &= TypeFacts::TRUTHY | TypeFacts::FALSY;
        assert_eq!(a, TypeFacts::TRUTHY);
    }

    #[test]
    fn bitor_assign_works() {
        let mut a = TypeFacts::TYPEOF_EQ_STRING;
        a |= TypeFacts::TRUTHY;
        assert!(a.contains(TypeFacts::TYPEOF_EQ_STRING));
        assert!(a.contains(TypeFacts::TRUTHY));
    }

    #[test]
    fn void_facts_equals_undefined_facts() {
        assert_eq!(TypeFacts::VOID_FACTS, TypeFacts::UNDEFINED_FACTS);
    }

    #[test]
    fn object_strict_facts_has_typeof_eq_host_object() {
        assert!(TypeFacts::OBJECT_STRICT_FACTS.contains(TypeFacts::TYPEOF_EQ_HOST_OBJECT));
        assert!(TypeFacts::OBJECT_STRICT_FACTS.contains(TypeFacts::TYPEOF_EQ_OBJECT));
    }

    #[test]
    fn function_strict_facts_has_typeof_eq_host_object() {
        assert!(TypeFacts::FUNCTION_STRICT_FACTS.contains(TypeFacts::TYPEOF_EQ_HOST_OBJECT));
        assert!(TypeFacts::FUNCTION_STRICT_FACTS.contains(TypeFacts::TYPEOF_EQ_FUNCTION));
    }

    #[test]
    fn empty_object_strict_facts() {
        assert!(TypeFacts::EMPTY_OBJECT_STRICT_FACTS.contains(TypeFacts::TRUTHY));
        assert!(TypeFacts::EMPTY_OBJECT_STRICT_FACTS.contains(TypeFacts::FALSY));
        assert!(!TypeFacts::EMPTY_OBJECT_STRICT_FACTS.contains(TypeFacts::EQ_UNDEFINED));
        assert!(!TypeFacts::EMPTY_OBJECT_STRICT_FACTS.contains(TypeFacts::EQ_NULL));
        assert!(!TypeFacts::EMPTY_OBJECT_STRICT_FACTS.contains(TypeFacts::EQ_UNDEFINED_OR_NULL));
    }

    #[test]
    fn empty_object_facts_is_all() {
        assert_eq!(TypeFacts::EMPTY_OBJECT_FACTS, TypeFacts::ALL);
    }
}
