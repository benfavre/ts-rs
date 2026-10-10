//! Diagnostic error helper functions for the type checker.

use std::sync::Arc;

use super::*;

/// TS2774: a definitely present function was tested for truthiness without
/// being called.
pub(crate) fn error_uncalled_function_truthiness(span: Span) -> Diagnostic {
    Diagnostic {
        code: 2774,
        message: "This condition will always return true since this function is always defined. Did you mean to call it instead?"
            .to_string(),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS2714: an ambient export assignment whose expression is not an entity name.
pub(crate) fn error_invalid_ambient_export_assignment(span: Span) -> Diagnostic {
    Diagnostic {
        code: 2714,
        message:
            "The expression of an export assignment must be an identifier or qualified name in an ambient context."
                .to_string(),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS2358: the left operand of `instanceof` is definitely primitive.
pub(crate) fn error_invalid_instanceof_left(span: Span) -> Diagnostic {
    Diagnostic {
        code: 2358,
        message: "The left-hand side of an 'instanceof' expression must be of type 'any', an object type or a type parameter."
            .to_string(),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS2359: the right operand of `instanceof` is definitely not callable,
/// constructable, or equipped with a custom `Symbol.hasInstance` method.
pub(crate) fn error_invalid_instanceof_right(span: Span) -> Diagnostic {
    Diagnostic {
        code: 2359,
        message: "The right-hand side of an 'instanceof' expression must be either of type 'any', a class, function, or other type assignable to the 'Function' interface type, or an object type with a 'Symbol.hasInstance' method."
            .to_string(),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS2725: a runtime class named `Object` would shadow the global used by
/// legacy module emit.
pub(crate) fn error_class_name_object_collision(module: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 2725,
        message: format!(
            "Class name cannot be 'Object' when targeting ES5 and above with module {}.",
            module
        ),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS1183: a body where an ambient context permits only a signature.
pub(crate) fn error_implementation_in_ambient(span: Span) -> Diagnostic {
    Diagnostic {
        code: 1183,
        message: "An implementation cannot be declared in ambient contexts.".to_string(),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS2695: the left operand of a comma has no side effects, so its value is
/// simply discarded.
pub(crate) fn error_comma_left_unused(span: Span) -> Diagnostic {
    Diagnostic {
        code: 2695,
        message: "Left side of comma operator is unused and has no side effects.".to_string(),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS2411: a declared property whose type does not satisfy the container's
/// string/number index signature.
pub(crate) fn error_property_not_assignable_to_index(
    prop: &str,
    prop_ty: &str,
    index_kind: &str,
    index_ty: &str,
    span: Span,
) -> Diagnostic {
    Diagnostic {
        code: 2411,
        message: format!(
            "Property '{}' of type '{}' is not assignable to '{}' index type '{}'.",
            prop, prop_ty, index_kind, index_ty
        ),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS2540: assignment to a `readonly` property or a getter-only accessor.
pub(crate) fn error_cannot_assign_readonly(prop: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 2540,
        message: format!(
            "Cannot assign to '{}' because it is a read-only property.",
            prop
        ),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS7026: a JSX intrinsic element in a file whose program declares no
/// `JSX.IntrinsicElements` interface (no JSX typings loaded).
pub(crate) fn error_jsx_no_intrinsic_elements(span: Span) -> Diagnostic {
    Diagnostic {
        code: 7026,
        message:
            "JSX element implicitly has type 'any' because no interface 'JSX.IntrinsicElements' exists."
                .to_string(),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS2301: an instance member initializer referencing a constructor
/// parameter — the parameter is in scope for the constructor body only.
pub(crate) fn error_initializer_references_ctor_param(
    member: &str,
    ident: &str,
    span: Span,
) -> Diagnostic {
    Diagnostic {
        code: 2301,
        message: format!(
            "Initializer of instance member variable '{}' cannot reference identifier '{}' declared in the constructor.",
            member, ident
        ),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS2304 for an identifier in EXPRESSION position, with tsc's
/// name-specific variants (`getCannotFindNameDiagnosticForName`): DOM
/// globals suggest the `dom` lib (TS2584), node globals / test-runner
/// globals / `$` suggest installing type definitions (TS2591 / 2593 /
/// 2592 — the "add to the types field" forms, since the harness never uses
/// wildcard `types`).
pub(crate) fn error_cannot_find_name_in_expression(name: &str, span: Span) -> Diagnostic {
    let (code, message) = match name {
        "document" | "console" => (
            2584,
            format!(
                "Cannot find name '{name}'. Do you need to change your target library? Try changing the 'lib' compiler option to include 'dom'."
            ),
        ),
        "$" => (
            2592,
            format!(
                "Cannot find name '{name}'. Do you need to install type definitions for jQuery? Try `npm i --save-dev @types/jquery` and then add 'jquery' to the types field in your tsconfig."
            ),
        ),
        "beforeEach" | "describe" | "suite" | "it" | "test" => (
            2593,
            format!(
                "Cannot find name '{name}'. Do you need to install type definitions for a test runner? Try `npm i --save-dev @types/jest` or `npm i --save-dev @types/mocha` and then add 'jest' or 'mocha' to the types field in your tsconfig."
            ),
        ),
        "process" | "require" | "Buffer" | "module" | "NodeJS" => (
            2591,
            format!(
                "Cannot find name '{name}'. Do you need to install type definitions for node? Try `npm i --save-dev @types/node` and then add 'node' to the types field in your tsconfig."
            ),
        ),
        _ => return error_cannot_find_name(name, span),
    };
    Diagnostic {
        code,
        message,
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS1194: `export { … }` / `export … from` inside a namespace body.
pub(crate) fn error_export_declarations_not_permitted_in_namespace(span: Span) -> Diagnostic {
    Diagnostic {
        code: 1194,
        message: "Export declarations are not permitted in a namespace.".to_string(),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS18016: a private identifier used outside any class body.
pub(crate) fn error_private_identifier_outside_class(span: Span) -> Diagnostic {
    Diagnostic {
        code: 18016,
        message: "Private identifiers are not allowed outside class bodies.".to_string(),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS2703 / TS2790: `delete` operand checks.
pub(crate) fn error_delete_operand(code: u32, span: Span) -> Diagnostic {
    let message = match code {
        2790 => "The operand of a 'delete' operator must be optional.",
        2704 => "The operand of a 'delete' operator cannot be a read-only property.",
        _ => "The operand of a 'delete' operator must be a property reference.",
    };
    Diagnostic {
        code,
        message: message.to_string(),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

pub(crate) fn error_cannot_find_name(name: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 2304,
        message: format!("Cannot find name '{}'.", name),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS1100: `eval` and `arguments` cannot be bindings or assignment targets
/// in strict-mode source.
pub(crate) fn error_invalid_strict_mode_name(name: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 1100,
        message: format!("Invalid use of '{}' in strict mode.", name),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS2662: bare reference to a name that is actually a static member of the
/// enclosing class (e.g. `foo` inside an instance method where `static foo`
/// exists — the reference must be qualified as `C.foo`).
pub(crate) fn error_cannot_find_name_static_member(
    name: &str,
    class_name: &str,
    span: Span,
) -> Diagnostic {
    Diagnostic {
        code: 2662,
        message: format!(
            "Cannot find name '{}'. Did you mean the static member '{}.{}'?",
            name, class_name, name
        ),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS2693: '{name}' only refers to a type, but is being used as a value here.
pub(crate) fn error_type_used_as_value(name: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 2693,
        message: format!(
            "'{}' only refers to a type, but is being used as a value here.",
            name
        ),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

pub(crate) fn error_not_assignable(source: &str, target: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 2322,
        message: format!("Type '{}' is not assignable to type '{}'.", source, target),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

pub(crate) fn error_no_properties_in_common(source: &str, target: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 2559,
        message: format!(
            "Type '{}' has no properties in common with type '{}'.",
            source, target
        ),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS2741: source is missing exactly one required property of the target.
pub(crate) fn error_property_missing_single(
    prop: &str,
    source: &str,
    target: &str,
    span: Span,
) -> Diagnostic {
    Diagnostic {
        code: 2741,
        message: format!(
            "Property '{}' is missing in type '{}' but required in type '{}'.",
            prop, source, target
        ),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS2739 / TS2740: source is missing multiple required properties of the
/// target. tsc lists all when 2–5 are missing (TS2739) and the first four plus
/// "and N more" when more than five are missing (TS2740).
pub(crate) fn error_missing_properties(
    missing: &[String],
    source: &str,
    target: &str,
    span: Span,
) -> Diagnostic {
    let (code, list) = if missing.len() > 5 {
        (
            2740,
            format!(
                "{}, and {} more.",
                missing[..4].join(", "),
                missing.len() - 4
            ),
        )
    } else {
        (2739, missing.join(", "))
    };
    Diagnostic {
        code,
        message: format!(
            "Type '{}' is missing the following properties from type '{}': {}",
            source, target, list
        ),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS2792: module-not-found under CLASSIC module resolution — tsc swaps
/// TS2307 for this suggestion-bearing variant.
pub(crate) fn error_cannot_find_module_classic(module: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 2792,
        message: format!(
            "Cannot find module '{}'. Did you mean to set the 'moduleResolution' option to 'nodenext', or to add aliases to the 'paths' option?",
            module
        ),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS2882: module-not-found for a side-effect import (`import "./x"`).
pub(crate) fn error_cannot_find_module_side_effect(module: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 2882,
        message: format!(
            "Cannot find module or type declarations for side-effect import of '{}'.",
            module
        ),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

pub(crate) fn error_type_only_esm_import_requires_resolution_mode(span: Span) -> Diagnostic {
    Diagnostic {
        code: 1541,
        message: "Type-only import of an ECMAScript module from a CommonJS module must have a 'resolution-mode' attribute.".to_string(),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

pub(crate) fn error_import_type_requires_resolution_mode(span: Span) -> Diagnostic {
    Diagnostic {
        code: 1542,
        message: "Type import of an ECMAScript module from a CommonJS module must have a 'resolution-mode' attribute.".to_string(),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS17009: `this` accessed before `super()` in a derived-class constructor.
pub(crate) fn error_this_before_super(span: Span) -> Diagnostic {
    Diagnostic {
        code: 17009,
        message:
            "'super' must be called before accessing 'this' in the constructor of a derived class."
                .to_string(),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS1210: `arguments`/`eval` bound inside a class (strict mode).
pub(crate) fn error_invalid_strict_name(name: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 1210,
        message: format!(
            "Code contained in a class is evaluated in JavaScript's strict mode which does not allow this use of '{}'. For more information, see https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Strict_mode.",
            name
        ),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS2441: declaration of a compiler-reserved module-scope name.
pub(crate) fn error_reserved_module_name(name: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 2441,
        message: format!(
            "Duplicate identifier '{}'. Compiler reserves name '{}' in top level scope of a module.",
            name, name
        ),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS2814: bodied function merged with a non-ambient class.
pub(crate) fn error_function_class_merge(span: Span) -> Diagnostic {
    Diagnostic {
        code: 2814,
        message: "Function with bodies can only merge with classes that are ambient.".to_string(),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS1308: `await` outside an async function.
pub(crate) fn error_await_in_non_async(span: Span) -> Diagnostic {
    Diagnostic {
        code: 1308,
        message: "'await' expressions are only allowed within async functions and at the top levels of modules.".to_string(),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS2538: invalid index type in an element access.
pub(crate) fn error_invalid_index_type(ty: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 2538,
        message: format!("Type '{}' cannot be used as an index type.", ty),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS2567: enum declaration merged with a non-namespace/non-enum declaration.
pub(crate) fn error_enum_merge_conflict(span: Span) -> Diagnostic {
    Diagnostic {
        code: 2567,
        message: "Enum declarations can only merge with namespace or other enum declarations."
            .to_string(),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS2367: equality comparison between types with no overlap.
pub(crate) fn error_static_member_via_instance(
    prop: &str,
    class_name: &str,
    span: Span,
) -> Diagnostic {
    Diagnostic {
        code: 2576,
        message: format!(
            "Property '{}' does not exist on type '{}'. Did you mean to access the static member '{}.{}' instead?",
            prop, class_name, class_name, prop
        ),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

pub(crate) fn error_bigint_property_name(span: Span) -> Diagnostic {
    Diagnostic {
        code: 1539,
        message: "A 'bigint' literal cannot be used as a property name.".to_string(),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

pub(crate) fn error_class_implements_primitive(prim: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 2864,
        message: format!(
            "A class cannot implement a primitive type like '{}'. It can only implement other named object types.",
            prim
        ),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

pub(crate) fn error_enum_used_before_declaration(name: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 2450,
        message: format!("Enum '{}' used before its declaration.", name),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

pub(crate) fn error_class_implements_overloads(name: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 2813,
        message: format!(
            "Class declaration cannot implement overload list for '{}'.",
            name
        ),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

pub(crate) fn error_class_not_callable(class_name: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 2348,
        message: format!(
            "Value of type 'typeof {}' is not callable. Did you mean to include 'new'?",
            class_name
        ),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

pub(crate) fn error_boolean_operator(op: &str, suggestion: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 2447,
        message: format!(
            "The '{}' operator is not allowed for boolean types. Consider using '{}' instead.",
            op, suggestion
        ),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

pub(crate) fn error_operator_cannot_be_applied(
    op: &str,
    left: &str,
    right: &str,
    span: Span,
) -> Diagnostic {
    Diagnostic {
        code: 2365,
        message: format!(
            "Operator '{}' cannot be applied to types '{}' and '{}'.",
            op, left, right
        ),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

pub(crate) fn error_arithmetic_lhs(span: Span) -> Diagnostic {
    Diagnostic {
        code: 2362,
        message: "The left-hand side of an arithmetic operation must be of type 'any', 'number', 'bigint' or an enum type.".to_string(),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

pub(crate) fn error_arithmetic_rhs(span: Span) -> Diagnostic {
    Diagnostic {
        code: 2363,
        message: "The right-hand side of an arithmetic operation must be of type 'any', 'number', 'bigint' or an enum type.".to_string(),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

pub(crate) fn error_not_callable(type_str: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 2349,
        message: format!(
            "This expression is not callable.\n  Type '{}' has no call signatures.",
            type_str
        ),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

pub(crate) fn error_comparison_no_overlap(left: &str, right: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 2367,
        message: format!(
            "This comparison appears to be unintentional because the types '{}' and '{}' have no overlap.",
            left, right
        ),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS2815: `arguments` referenced in a class property initializer or a class
/// static initialization block.
pub(crate) fn error_arguments_in_class_field(span: Span) -> Diagnostic {
    Diagnostic {
        code: 2815,
        message: "'arguments' cannot be referenced in property initializers or class static initialization blocks.".to_string(),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS2715: abstract property accessed through `this` during class
/// initialization (constructor body / instance property initializer).
pub(crate) fn error_abstract_property_in_constructor(
    prop: &str,
    declaring_class: &str,
    span: Span,
) -> Diagnostic {
    Diagnostic {
        code: 2715,
        message: format!(
            "Abstract property '{}' in class '{}' cannot be accessed in the constructor.",
            prop, declaring_class
        ),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS2631 / TS2630 / TS2632 / TS2539 / TS2588: assignment target is a
/// namespace, function, import, non-variable, or constant binding.
pub(crate) fn error_cannot_assign_binding(code: u32, name: &str, span: Span) -> Diagnostic {
    let what = match code {
        2631 => "because it is a namespace",
        2630 => "because it is a function",
        2632 => "because it is an import",
        2588 => "because it is a constant",
        _ => "because it is not a variable",
    };
    Diagnostic {
        code,
        message: format!("Cannot assign to '{}' {}.", name, what),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS2629: assignment target is a class binding.
pub(crate) fn error_cannot_assign_class(name: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 2629,
        message: format!("Cannot assign to '{}' because it is a class.", name),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS2628: assignment target is an enum binding.
pub(crate) fn error_cannot_assign_enum(name: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 2628,
        message: format!("Cannot assign to '{}' because it is an enum.", name),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

pub(crate) fn error_arg_not_assignable(source: &str, target: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 2345,
        message: format!(
            "Argument of type '{}' is not assignable to parameter of type '{}'.",
            source, target
        ),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

pub(crate) fn error_property_not_exist(prop: &str, on_type: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 2339,
        message: format!("Property '{}' does not exist on type '{}'.", prop, on_type),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

pub(crate) fn error_property_requires_newer_lib(
    prop: &str,
    on_type: &str,
    library: &str,
    span: Span,
) -> Diagnostic {
    Diagnostic {
        code: 2550,
        message: format!(
            "Property '{}' does not exist on type '{}'. Do you need to change your target library? Try changing the 'lib' compiler option to '{}' or later.",
            prop, on_type, library
        ),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS2585: the configured libs declare the TYPE but not the VALUE of a
/// standard global (`Symbol()` with only es5). Note the lib is unquoted.
pub(crate) fn error_type_only_global_used_as_value(
    name: &str,
    library: &str,
    span: Span,
) -> Diagnostic {
    Diagnostic {
        code: 2585,
        message: format!(
            "'{}' only refers to a type, but is being used as a value here. Do you need to change your target library? Try changing the 'lib' compiler option to {} or later.",
            name, library
        ),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

pub(crate) fn error_name_requires_newer_lib(name: &str, library: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 2583,
        message: format!(
            "Cannot find name '{}'. Do you need to change your target library? Try changing the 'lib' compiler option to '{}' or later.",
            name, library
        ),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

pub(crate) fn error_arg_count(
    min_expected: usize,
    max_expected: usize,
    got: usize,
    span: Span,
) -> Diagnostic {
    let expected_str = if min_expected == max_expected {
        format!("{}", min_expected)
    } else {
        format!("{}-{}", min_expected, max_expected)
    };
    Diagnostic {
        code: 2554,
        message: format!("Expected {} arguments, but got {}.", expected_str, got),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

pub(crate) fn error_type_arg_count(
    min_expected: usize,
    max_expected: usize,
    got: usize,
    span: Span,
) -> Diagnostic {
    let expected = if min_expected == max_expected {
        min_expected.to_string()
    } else {
        format!("{}-{}", min_expected, max_expected)
    };
    Diagnostic {
        code: 2558,
        message: format!("Expected {} type arguments, but got {}.", expected, got),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

pub(crate) fn error_type_arg_constraint(actual: &str, constraint: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 2344,
        message: format!(
            "Type '{}' does not satisfy the constraint '{}'.",
            actual, constraint
        ),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

pub(crate) fn error_value_unknown(name: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 18046,
        message: format!("'{}' is of type 'unknown'.", name),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

pub(crate) fn error_arithmetic_operand(span: Span) -> Diagnostic {
    Diagnostic {
        code: 2356,
        message: "An arithmetic operand must be of type 'any', 'number', 'bigint' or an enum type."
            .to_string(),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS2736: unary `+` never applies to bigint values.
pub(crate) fn error_bigint_unary_plus(span: Span) -> Diagnostic {
    Diagnostic {
        code: 2736,
        message: "Operator '+' cannot be applied to type 'bigint'.".to_string(),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

pub(crate) fn error_no_overload_match(num_overloads: usize, span: Span) -> Diagnostic {
    Diagnostic {
        code: 2769,
        message: format!(
            "No overload matches this call.\n  {} overload(s) were tried, but none matched.",
            num_overloads
        ),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

pub(crate) fn error_implicit_any(name: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 7006,
        message: format!("Parameter '{}' implicitly has an 'any' type.", name),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

pub(crate) fn error_implicit_any_rest(name: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 7019,
        message: format!("Rest parameter '{}' implicitly has an 'any[]' type.", name),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS18047 / TS18048 / TS18049: a nullable ENTITY NAME (`x`, `a.b.c`) is
/// dereferenced — tsc names the expression instead of saying "Object".
pub(crate) fn error_name_possibly_nullish(
    name: &str,
    has_null: bool,
    has_undefined: bool,
    span: Span,
) -> Diagnostic {
    let (code, what) = match (has_null, has_undefined) {
        (true, true) => (18049, "'null' or 'undefined'"),
        (true, false) => (18047, "'null'"),
        _ => (18048, "'undefined'"),
    };
    Diagnostic {
        code,
        message: format!("'{}' is possibly {}.", name, what),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

pub(crate) fn error_possibly_null(span: Span) -> Diagnostic {
    Diagnostic {
        code: 2531,
        message: "Object is possibly 'null'.".to_string(),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

pub(crate) fn error_possibly_undefined(span: Span) -> Diagnostic {
    Diagnostic {
        code: 2532,
        message: "Object is possibly 'undefined'.".to_string(),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

pub(crate) fn error_possibly_null_or_undefined(span: Span) -> Diagnostic {
    Diagnostic {
        code: 2533,
        message: "Object is possibly 'null' or 'undefined'.".to_string(),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

#[allow(dead_code)]
pub(crate) fn error_expected(what: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 1005,
        message: format!("'{}' expected.", what),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

pub(crate) fn error_cannot_create_abstract(_class_name: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 2511,
        message: "Cannot create an instance of an abstract class.".to_string(),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

pub(crate) fn error_not_constructable(type_name: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 2351,
        message: format!(
            "This expression is not constructable.\n  Type '{}' has no construct signatures.",
            type_name
        ),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

pub(crate) fn error_excess_property(prop: &str, target_type: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 2353,
        message: format!(
            "Object literal may only specify known properties, and '{}' does not exist in type '{}'.",
            prop, target_type
        ),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS2561: like TS2353, but the excess property is a near-miss spelling of a
/// known property, so tsc suggests the intended name.
pub(crate) fn error_excess_property_suggestion(
    prop: &str,
    target_type: &str,
    suggestion: &str,
    span: Span,
) -> Diagnostic {
    Diagnostic {
        code: 2561,
        message: format!(
            "Object literal may only specify known properties, but '{}' does not exist in type '{}'. Did you mean to write '{}'?",
            prop, target_type, suggestion
        ),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS2720: `class C implements SomeClass` where members are missing —
/// tsc suggests extending instead.
/// TS2729: a class STATIC initializer reads a property of a value declared
/// later in the file.
pub(crate) fn error_property_used_before_init(prop: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 2729,
        message: format!("Property '{}' is used before its initialization.", prop),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS18050: `null` / `undefined` used as an arithmetic or bitwise operand
/// (under strictNullChecks).
pub(crate) fn error_value_cannot_be_used_here(value: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 18050,
        message: format!("The value '{}' cannot be used here.", value),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

pub(crate) fn error_class_incorrectly_implements_class(
    class_name: &str,
    base_name: &str,
    span: Span,
) -> Diagnostic {
    Diagnostic {
        code: 2720,
        message: format!(
            "Class '{}' incorrectly implements class '{}'. Did you mean to extend '{}' and inherit its members as a subclass?",
            class_name, base_name, base_name
        ),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

pub(crate) fn error_class_incorrectly_implements(
    class_name: &str,
    iface_name: &str,
    prop_name: &str,
    span: Span,
) -> Diagnostic {
    Diagnostic {
        code: 2420,
        message: format!(
            "Class '{}' incorrectly implements interface '{}'.\n  Property '{}' is missing in type '{}' but required in type '{}'.",
            class_name, iface_name, prop_name, class_name, iface_name
        ),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

pub(crate) fn error_property_not_assignable_to_base(
    prop_name: &str,
    class_name: &str,
    iface_name: &str,
    span: Span,
) -> Diagnostic {
    Diagnostic {
        code: 2416,
        message: format!(
            "Property '{}' in type '{}' is not assignable to the same property in base type '{}'.",
            prop_name, class_name, iface_name
        ),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS2415: Class '{class}' incorrectly extends base class '{base}'.
pub(crate) fn error_class_incorrectly_extends(
    class_name: &str,
    base_name: &str,
    elaboration: &str,
    span: Span,
) -> Diagnostic {
    Diagnostic {
        code: 2415,
        message: format!(
            "Class '{}' incorrectly extends base class '{}'.\n  {}",
            class_name, base_name, elaboration
        ),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS1117 / TS1118 / TS1119: object literal member name conflicts.
pub(crate) fn error_object_literal_name_conflict(code: u32, span: Span) -> Diagnostic {
    let message = match code {
        1118 => "An object literal cannot have multiple get/set accessors with the same name.",
        1119 => "An object literal cannot have property and accessor with the same name.",
        _ => "An object literal cannot have multiple properties with the same name.",
    };
    Diagnostic {
        code,
        message: message.to_string(),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS2341: Property '{name}' is private and only accessible within class '{class}'.
pub(crate) fn error_private_member_access(name: &str, class_name: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 2341,
        message: format!(
            "Property '{}' is private and only accessible within class '{}'.",
            name, class_name
        ),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS2451: Cannot redeclare block-scoped variable '{name}'.
pub(crate) fn error_cannot_redeclare_block_scoped(name: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 2451,
        message: format!("Cannot redeclare block-scoped variable '{}'.", name),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS2661: Cannot export '{name}'. Only local declarations can be exported from a module.
pub(crate) fn error_cannot_export_non_local(name: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 2661,
        message: format!(
            "Cannot export '{}'. Only local declarations can be exported from a module.",
            name
        ),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS1192: Module '"{module}"' has no default export.
pub(crate) fn error_module_no_default_export(module: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 1192,
        message: format!("Module '\"{}\"' has no default export.", module),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

pub(crate) fn error_function_implementation_missing(span: Span) -> Diagnostic {
    Diagnostic {
        code: 2391,
        message: "Function implementation is missing or not immediately following the declaration."
            .to_string(),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

pub(crate) fn error_constructor_implementation_missing(span: Span) -> Diagnostic {
    Diagnostic {
        code: 2390,
        message: "Constructor implementation is missing.".to_string(),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS2480: 'let' is not allowed to be used as a name in 'let' or 'const' declarations.
pub(crate) fn error_let_as_declaration_name(span: Span) -> Diagnostic {
    Diagnostic {
        code: 2480,
        message: "'let' is not allowed to be used as a name in 'let' or 'const' declarations."
            .to_string(),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS2302: Static members cannot reference class type parameters.
pub(crate) fn error_static_member_references_type_param(span: Span) -> Diagnostic {
    Diagnostic {
        code: 2302,
        message: "Static members cannot reference class type parameters.".to_string(),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

pub(crate) fn error_duplicate_identifier(name: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 2300,
        message: format!("Duplicate identifier '{}'.", name),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS2454: Variable '{name}' is used before being assigned.
pub(crate) fn error_used_before_assigned(name: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 2454,
        message: format!("Variable '{}' is used before being assigned.", name),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS2564: Property '{name}' has no initializer and is not definitely assigned in the constructor.
pub(crate) fn error_property_not_definitely_assigned(name: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 2564,
        message: format!(
            "Property '{}' has no initializer and is not definitely assigned in the constructor.",
            name
        ),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS6133: '{name}' is declared but its value is never read.
pub(crate) fn error_unused_local(name: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 6133,
        message: format!("'{}' is declared but its value is never read.", name),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS6196: '{name}' is declared but never used.
/// Used for classes, enums, interfaces, type aliases.
pub(crate) fn error_unused_decl(name: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 6196,
        message: format!("'{}' is declared but never used.", name),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS2307: Cannot find module '{module}' or its corresponding type declarations.
pub(crate) fn error_cannot_find_module(module: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 2307,
        message: format!(
            "Cannot find module \'{}\' or its corresponding type declarations.",
            module
        ),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS2314: Generic type '{name}' requires {count} type argument(s).
pub(crate) fn error_cannot_find_namespace(name: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 2503,
        message: format!("Cannot find namespace '{}'.", name),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS2303: an import-equals alias graph contains a cycle.
pub(crate) fn error_circular_import_alias(name: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 2303,
        message: format!("Circular definition of import alias '{}'.", name),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

pub(crate) fn error_namespace_no_exported_member(ns: &str, member: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code: 2694,
        message: format!("Namespace '{}' has no exported member '{}'.", ns, member),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

pub(crate) fn error_generic_type_requires_args(name: &str, count: usize, span: Span) -> Diagnostic {
    Diagnostic {
        code: 2314,
        message: format!(
            "Generic type \'{}\' requires {} type argument(s).",
            name, count
        ),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS7027: Unreachable code detected.
pub(crate) fn error_unreachable_code(span: Span) -> Diagnostic {
    Diagnostic {
        code: 7027,
        message: "Unreachable code detected.".to_string(),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS2369: A parameter property is only allowed in a constructor implementation.
pub(crate) fn error_parameter_property_outside_constructor(span: Span) -> Diagnostic {
    Diagnostic {
        code: 2369,
        message: "A parameter property is only allowed in a constructor implementation."
            .to_string(),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

/// TS2403: Subsequent variable declarations must have the same type.
/// Variable '{name}' must be of type '{first_type}', but here has type '{second_type}'.
pub(crate) fn error_subsequent_var_type(
    name: &str,
    first_type: &str,
    second_type: &str,
    span: Span,
    related_span: Option<Span>,
    related_file: Option<String>,
) -> Diagnostic {
    Diagnostic {
        code: 2403,
        message: format!(
            "Subsequent variable declarations must have the same type.  Variable '{}' must be of type '{}', but here has type '{}'.",
            name, first_type, second_type
        ),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: related_span.map(|rs| {
            vec![RelatedDiagnostic {
                code: 6203,
                message: format!("'{}' was also declared here.", name),
                file_name: related_file,
                span: Some(rs),
            }]
        }),
    }
}

/// TS2305: Module '{module}' has no exported member '{name}'.
pub(crate) fn error_module_no_exported_member(
    module: &str,
    member: &str,
    span: Span,
) -> Diagnostic {
    Diagnostic {
        code: 2305,
        message: format!(
            "Module '\"{}\"' has no exported member '{}'.",
            module, member
        ),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

// ---------------------------------------------------------------------------
// Compiler option diagnostics
// ---------------------------------------------------------------------------

use tsc_rs_ast::CompilerOptions;

/// A global diagnostic with an optional related-info follow-up message.
pub struct DeprecatedOptionDiag {
    pub diagnostic: Diagnostic,
    /// Optional related message, e.g. "  Visit https://aka.ms/ts6 for migration information."
    pub related: Option<String>,
    /// The compiler option name (e.g. "baseUrl", "target") for tsconfig.json location lookup.
    pub option_name: String,
    /// Whether a located tsconfig diagnostic points at the option value rather
    /// than its key.
    pub point_at_value: bool,
}

/// Check compiler options for invalid combinations and deprecated/removed
/// options, returning diagnostics without source locations. The harness adds
/// tsconfig locations when the option originated in a config file.
///
/// These correspond to:
/// - **TS5052**: An option's required companion option is disabled.
/// - **TS5053**: Mutually exclusive options are both enabled.
/// - **TS5095**, **TS5109**, **TS5110**: Module and resolution modes conflict.
/// - **TS5107**: Option is deprecated and will stop functioning in TypeScript 7.0.
/// - **TS5101**: Option is deprecated and will stop functioning in TypeScript 7.0.
/// - **TS5102**: Option has been removed.
pub fn check_deprecated_options(options: &CompilerOptions) -> Vec<DeprecatedOptionDiag> {
    // Check if ignoreDeprecations is set — if "6.0", suppress TS5107 warnings
    let ignore_deprecations = options
        .other
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("ignoredeprecations"))
        .map(|(_, v)| v.trim().trim_matches('"').to_string());

    let suppress_5107 = ignore_deprecations.as_deref().is_some_and(|v| v == "6.0");
    let suppress_5101 = ignore_deprecations
        .as_deref()
        .is_some_and(|v| v == "5.0" || v == "6.0");

    let mut diags = Vec::new();

    // --- TS5067 / TS5059: JSX factory names must be (qualified) identifiers ---
    let is_identifier = |text: &str| {
        let mut chars = text.chars();
        chars
            .next()
            .is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$')
            && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
    };
    let react_namespace = options
        .other
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("reactnamespace"))
        .map(|(_, v)| v.trim().to_string());
    if let Some(namespace) = &react_namespace {
        if !is_identifier(namespace) {
            diags.push(option_error(
                5059,
                "reactNamespace",
                true,
                &format!("Invalid value for '--reactNamespace'. '{namespace}' is not a valid identifier."),
            ));
        }
    }
    let jsx_factory = options
        .other
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("jsxfactory"))
        .map(|(_, v)| v.trim().to_string());
    if let Some(factory) = jsx_factory.as_deref() {
        if !factory.split('.').all(is_identifier) {
            diags.push(option_error(
                5067,
                "jsxFactory",
                true,
                &format!("Invalid value for 'jsxFactory'. '{factory}' is not a valid identifier or qualified-name."),
            ));
        }
    }

    // --- TS5053: mutually exclusive options ---
    // noLib prevents every default or explicitly requested library from
    // loading; specifying `lib` alongside it is therefore an error.
    if options.no_lib == Some(true) && !options.lib.is_empty() {
        diags.push(option_error(
            5053,
            "lib",
            false,
            "Option 'lib' cannot be specified with option 'noLib'.",
        ));
    }

    // Source map options have both conflicts and required companions.
    let map_root = options
        .other
        .iter()
        .any(|(name, value)| name.eq_ignore_ascii_case("mapRoot") && !value.is_empty());
    let declaration_map = options.other.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("declarationMap") && value.eq_ignore_ascii_case("true")
    });
    if options.inline_source_map == Some(true) {
        if options.source_map == Some(true) {
            diags.push(option_error(
                5053,
                "sourceMap",
                false,
                "Option 'sourceMap' cannot be specified with option 'inlineSourceMap'.",
            ));
        }
        if map_root {
            diags.push(option_error(
                5053,
                "mapRoot",
                false,
                "Option 'mapRoot' cannot be specified with option 'inlineSourceMap'.",
            ));
        }
    }
    let source_root = options
        .other
        .iter()
        .any(|(name, value)| name.eq_ignore_ascii_case("sourceRoot") && !value.is_empty());
    if source_root && options.source_map != Some(true) && options.inline_source_map != Some(true) {
        // tsc's message text lacks the closing quote after the option name.
        diags.push(option_error(
            5051,
            "sourceRoot",
            false,
            "Option 'sourceRoot can only be used when either option '--inlineSourceMap' or option '--sourceMap' is provided.",
        ));
    }
    if map_root && options.source_map != Some(true) && !declaration_map {
        diags.push(option_error(
            5069,
            "mapRoot",
            false,
            "Option 'mapRoot' cannot be specified without specifying option 'sourceMap' or option 'declarationMap'.",
        ));
    }

    // Declaration-related output options require declaration emit. Composite
    // projects imply it even when the declaration flag is omitted.
    let declaration_dir = options
        .other
        .iter()
        .any(|(name, value)| name.eq_ignore_ascii_case("declarationDir") && !value.is_empty());
    if declaration_dir && options.out_file.is_some() {
        diags.push(option_error(
            5053,
            "declarationDir",
            false,
            "Option 'declarationDir' cannot be specified with option 'outFile'.",
        ));
    }
    if options.declaration != Some(true) && options.composite != Some(true) {
        for (name, enabled) in [
            ("declarationDir", declaration_dir),
            ("declarationMap", declaration_map),
            (
                "emitDeclarationOnly",
                options.emit_declaration_only == Some(true),
            ),
            (
                "isolatedDeclarations",
                options.isolated_declarations == Some(true),
            ),
        ] {
            if enabled {
                diags.push(option_error(
                    5069,
                    name,
                    false,
                    &format!("Option '{name}' cannot be specified without specifying option 'declaration' or option 'composite'."),
                ));
            }
        }
    }

    // --- TS5052: dependent options ---
    // `checkJs` implies `allowJs` when the latter is omitted; only an
    // explicitly false `allowJs` conflicts.
    if options.check_js == Some(true) && options.allow_js == Some(false) {
        diags.push(option_error(
            5052,
            "checkJs",
            false,
            "Option 'checkJs' cannot be specified without specifying option 'allowJs'.",
        ));
    }
    if options.emit_decorator_metadata == Some(true)
        && options.experimental_decorators != Some(true)
    {
        diags.push(option_error(
            5052,
            "emitDecoratorMetadata",
            false,
            "Option 'emitDecoratorMetadata' cannot be specified without specifying option 'experimentalDecorators'.",
        ));
    }
    let strict_null_checks_enabled = options
        .strict_null_checks
        .unwrap_or(options.strict == Some(true));
    if options.exact_optional_property_types == Some(true) && !strict_null_checks_enabled {
        diags.push(option_error(
            5052,
            "exactOptionalPropertyTypes",
            false,
            "Option 'exactOptionalPropertyTypes' cannot be specified without specifying option 'strictNullChecks'.",
        ));
    }

    let module_resolution = options
        .module_resolution
        .as_deref()
        .map(|value| value.to_ascii_lowercase());

    // --- TS5070 / TS5071: resolveJsonModule needs a resolver and a module
    // kind that can emit JSON. TS 6 implies it for bundler / Node20 /
    // NodeNext resolution when the option is not given explicitly.
    let resolve_json_module = options.resolve_json_module.unwrap_or_else(|| {
        matches!(
            options.module,
            Some(tsc_rs_ast::ModuleKind::Node20 | tsc_rs_ast::ModuleKind::NodeNext)
        ) || module_resolution.as_deref() == Some("bundler")
    });
    if resolve_json_module {
        if module_resolution.as_deref() == Some("classic") {
            diags.push(option_error(
                5070,
                "resolveJsonModule",
                false,
                "Option '--resolveJsonModule' cannot be specified when 'moduleResolution' is set to 'classic'.",
            ));
        } else if matches!(
            options.module,
            Some(
                tsc_rs_ast::ModuleKind::None
                    | tsc_rs_ast::ModuleKind::System
                    | tsc_rs_ast::ModuleKind::UMD
            )
        ) {
            diags.push(option_error(
                5071,
                "resolveJsonModule",
                false,
                "Option '--resolveJsonModule' cannot be specified when 'module' is set to 'none', 'system', or 'umd'.",
            ));
        }
    }

    // --- TS5095: bundler module-resolution compatibility ---
    if module_resolution.as_deref() == Some("bundler")
        && options.module.is_some_and(|module| {
            !matches!(
                module,
                tsc_rs_ast::ModuleKind::CommonJS
                    | tsc_rs_ast::ModuleKind::ES2015
                    | tsc_rs_ast::ModuleKind::ES2020
                    | tsc_rs_ast::ModuleKind::ES2022
                    | tsc_rs_ast::ModuleKind::ESNext
                    | tsc_rs_ast::ModuleKind::Preserve
            )
        })
    {
        diags.push(option_error(
            5095,
            "moduleResolution",
            true,
            "Option 'bundler' can only be used when 'module' is set to 'preserve', 'commonjs', or 'es2015' or later.",
        ));
    }

    // --- TS5109: Node module kind with an incompatible explicit resolver ---
    let node_module_name = match options.module {
        Some(tsc_rs_ast::ModuleKind::Node16) => Some("Node16"),
        Some(tsc_rs_ast::ModuleKind::Node18) => Some("Node18"),
        Some(tsc_rs_ast::ModuleKind::Node20) => Some("Node20"),
        Some(tsc_rs_ast::ModuleKind::NodeNext) => Some("NodeNext"),
        _ => None,
    };
    if let (Some(module_name), Some(actual)) = (node_module_name, module_resolution.as_deref()) {
        if !matches!(actual.to_ascii_lowercase().as_str(), "node16" | "nodenext") {
            let required = if module_name == "NodeNext" {
                "NodeNext"
            } else {
                "Node16"
            };
            diags.push(option_error(
                5109,
                "moduleResolution",
                true,
                &format!(
                    "Option 'moduleResolution' must be set to '{required}' (or left unspecified) when option 'module' is set to '{module_name}'."
                ),
            ));
        }
    }

    // --- TS5110: Node resolver with an incompatible module kind ---
    if let Some(resolution) = module_resolution.as_deref() {
        let required_module = if resolution.eq_ignore_ascii_case("node16") {
            Some("Node16")
        } else if resolution.eq_ignore_ascii_case("nodenext") {
            Some("NodeNext")
        } else {
            None
        };
        if let Some(display) = required_module {
            if !matches!(
                options.module,
                Some(
                    tsc_rs_ast::ModuleKind::Node16
                        | tsc_rs_ast::ModuleKind::Node18
                        | tsc_rs_ast::ModuleKind::Node20
                        | tsc_rs_ast::ModuleKind::NodeNext
                )
            ) {
                diags.push(option_error(
                    5110,
                    "moduleResolution",
                    true,
                    &format!(
                        "Option 'module' must be set to '{display}' when option 'moduleResolution' is set to '{display}'."
                    ),
                ));
            }
        }
    }

    // --- TS6082: JavaScript bundles require AMD or System modules ---
    // An omitted/None module kind follows the separate source-file validation
    // path. Declaration-only bundles do not impose a JavaScript module kind.
    if options
        .out_file
        .as_deref()
        .is_some_and(|path| !path.is_empty())
        && options.emit_declaration_only != Some(true)
        && options.module.is_some_and(|module| {
            !matches!(
                module,
                tsc_rs_ast::ModuleKind::None
                    | tsc_rs_ast::ModuleKind::AMD
                    | tsc_rs_ast::ModuleKind::System
            )
        })
    {
        diags.push(option_error(
            6082,
            "outFile",
            false,
            "Only 'amd' and 'system' modules are supported alongside --outFile.",
        ));
    }

    // --- TS5101: deprecated, will stop in TypeScript 7.0 ---
    // (TS5101 diagnostics appear before TS5107 in TypeScript's output ordering)
    if !suppress_5101 {
        // outFile (when used with bundling modules like AMD/System)
        if options.out_file.is_some() {
            diags.push(deprecated_5101("outFile", false));
        }
        // downlevelIteration
        if options.down_level_iteration == Some(true) {
            diags.push(deprecated_5101("downlevelIteration", false));
        }
        // baseUrl (when set without paths)
        if options.base_url.is_some() && options.paths.is_none() {
            diags.push(deprecated_5101("baseUrl", true));
        }
    }

    // --- TS5107: deprecated, will stop in TypeScript 7.0 ---
    if !suppress_5107 {
        // target=ES3 — removed (TS5108), not just deprecated
        // target=ES5 uses TS5107 (deprecated); ES3 was fully removed
        // NOTE: TS5108 is emitted in the TS5107 section so ordering works
        if options.target == Some(tsc_rs_ast::ScriptTarget::ES3) {
            diags.push(removed_5108("target=ES3"));
        }
        // target=ES5
        if options.target == Some(tsc_rs_ast::ScriptTarget::ES5) {
            diags.push(deprecated_5107("target=ES5", false));
        }
        // module=AMD
        if options.module == Some(tsc_rs_ast::ModuleKind::AMD) {
            diags.push(deprecated_5107("module=AMD", false));
        }
        // module=UMD
        if options.module == Some(tsc_rs_ast::ModuleKind::UMD) {
            diags.push(deprecated_5107("module=UMD", false));
        }
        // module=System
        if options.module == Some(tsc_rs_ast::ModuleKind::System) {
            diags.push(deprecated_5107("module=System", false));
        }
        // module=None
        if options.module == Some(tsc_rs_ast::ModuleKind::None) {
            diags.push(deprecated_5107("module=None", false));
        }
        // moduleResolution=classic
        if options
            .module_resolution
            .as_deref()
            .is_some_and(|v| v.eq_ignore_ascii_case("classic"))
        {
            diags.push(deprecated_5107("moduleResolution=classic", false));
        }
        // moduleResolution=node10
        if options
            .module_resolution
            .as_deref()
            .is_some_and(|v| v.eq_ignore_ascii_case("node10") || v.eq_ignore_ascii_case("node"))
        {
            diags.push(deprecated_5107("moduleResolution=node10", true));
        }
        // esModuleInterop=false
        if options.es_module_interop == Some(false) {
            diags.push(deprecated_5107("esModuleInterop=false", false));
        }
        // allowSyntheticDefaultImports=false
        if options.allow_synthetic_default_imports == Some(false) {
            diags.push(deprecated_5107("allowSyntheticDefaultImports=false", false));
        }
    }

    // --- TS5102: removed options ---
    // These are emitted regardless of ignoreDeprecations.
    // noImplicitUseStrict
    if has_option_in_other(options, "noimplicitusestrict") {
        diags.push(removed_5102("noImplicitUseStrict"));
    }
    // keyofStringsOnly
    if has_option_in_other(options, "keyofstringsonly") {
        diags.push(removed_5102("keyofStringsOnly"));
    }
    // suppressExcessPropertyErrors
    if has_option_in_other(options, "suppressexcesspropertyerrors") {
        diags.push(removed_5102("suppressExcessPropertyErrors"));
    }
    // suppressImplicitAnyIndexErrors
    if has_option_in_other(options, "suppressimplicitanyindexerrors") {
        diags.push(removed_5102("suppressImplicitAnyIndexErrors"));
    }
    // noStrictGenericChecks
    if has_option_in_other(options, "nostrictgenericchecks") {
        diags.push(removed_5102("noStrictGenericChecks"));
    }
    // charset
    if has_option_in_other(options, "charset") {
        diags.push(removed_5102("charset"));
    }
    // out (the old --out flag, separate from outFile)
    if has_option_in_other(options, "out") {
        diags.push(removed_5102("out"));
    }
    // preserveValueImports
    if has_option_in_other(options, "preservevalueimports") {
        diags.push(removed_5102_with_related(
            "preserveValueImports",
            "  Use 'verbatimModuleSyntax' instead.",
        ));
    }
    // importsNotUsedAsValues
    if options.imports_not_used_as_values.is_some() {
        diags.push(removed_5102_with_related(
            "importsNotUsedAsValues",
            "  Use 'verbatimModuleSyntax' instead.",
        ));
    }

    // Sort TS5107 diagnostics alphabetically by option description.
    // TypeScript outputs them in alphabetical order.
    diags.sort_by(|a, b| {
        a.diagnostic
            .code
            .cmp(&b.diagnostic.code)
            .then_with(|| a.diagnostic.message.cmp(&b.diagnostic.message))
    });

    diags
}

const VISIT_MSG: &str = "  Visit https://aka.ms/ts6 for migration information.";

/// Extract the option name from an option description (e.g., "target=ES5" -> "target").
fn option_name_from_desc(option_desc: &str) -> String {
    option_desc
        .split('=')
        .next()
        .unwrap_or(option_desc)
        .to_string()
}

fn deprecated_5107(option_desc: &str, has_visit: bool) -> DeprecatedOptionDiag {
    DeprecatedOptionDiag {
        option_name: option_name_from_desc(option_desc),
        point_at_value: option_desc.contains('='),
        diagnostic: Diagnostic {
            code: 5107,
            message: format!(
                "Option '{}' is deprecated and will stop functioning in TypeScript 7.0. Specify compilerOption '\"ignoreDeprecations\": \"6.0\"' to silence this error.",
                option_desc
            ),
            category: DiagnosticCategory::Error,
            file_name: None,
            span: None,
            related: None,
        },
        related: if has_visit {
            Some(VISIT_MSG.to_string())
        } else {
            None
        },
    }
}

fn deprecated_5101(option_desc: &str, has_visit: bool) -> DeprecatedOptionDiag {
    DeprecatedOptionDiag {
        option_name: option_name_from_desc(option_desc),
        point_at_value: option_desc.contains('='),
        diagnostic: Diagnostic {
            code: 5101,
            message: format!(
                "Option '{}' is deprecated and will stop functioning in TypeScript 7.0. Specify compilerOption '\"ignoreDeprecations\": \"6.0\"' to silence this error.",
                option_desc
            ),
            category: DiagnosticCategory::Error,
            file_name: None,
            span: None,
            related: None,
        },
        related: if has_visit {
            Some(VISIT_MSG.to_string())
        } else {
            None
        },
    }
}

fn removed_5102(option_desc: &str) -> DeprecatedOptionDiag {
    DeprecatedOptionDiag {
        option_name: option_name_from_desc(option_desc),
        point_at_value: option_desc.contains('='),
        diagnostic: Diagnostic {
            code: 5102,
            message: format!(
                "Option '{}' has been removed. Please remove it from your configuration.",
                option_desc
            ),
            category: DiagnosticCategory::Error,
            file_name: None,
            span: None,
            related: None,
        },
        related: None,
    }
}

fn removed_5102_with_related(option_desc: &str, related_msg: &str) -> DeprecatedOptionDiag {
    DeprecatedOptionDiag {
        option_name: option_name_from_desc(option_desc),
        point_at_value: option_desc.contains('='),
        diagnostic: Diagnostic {
            code: 5102,
            message: format!(
                "Option '{}' has been removed. Please remove it from your configuration.",
                option_desc
            ),
            category: DiagnosticCategory::Error,
            file_name: None,
            span: None,
            related: None,
        },
        related: Some(related_msg.to_string()),
    }
}

/// TS5108: Option 'target=ES3' has been removed.
/// Same message format as TS5102 but code 5108, used for removed target values.
fn removed_5108(option_desc: &str) -> DeprecatedOptionDiag {
    DeprecatedOptionDiag {
        option_name: option_name_from_desc(option_desc),
        point_at_value: option_desc.contains('='),
        diagnostic: Diagnostic {
            code: 5108,
            message: format!(
                "Option '{}' has been removed. Please remove it from your configuration.",
                option_desc
            ),
            category: DiagnosticCategory::Error,
            file_name: None,
            span: None,
            related: None,
        },
        related: None,
    }
}

fn option_error(
    code: u32,
    option_name: &str,
    point_at_value: bool,
    message: &str,
) -> DeprecatedOptionDiag {
    DeprecatedOptionDiag {
        option_name: option_name.to_string(),
        point_at_value,
        diagnostic: Diagnostic {
            code,
            message: message.to_string(),
            category: DiagnosticCategory::Error,
            file_name: None,
            span: None,
            related: None,
        },
        related: None,
    }
}

/// Check if an option name (lowercased) exists in the `other` field.
fn has_option_in_other(options: &CompilerOptions, name_lower: &str) -> bool {
    options
        .other
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case(name_lower))
}

#[cfg(test)]
mod option_tests {
    use super::*;
    use tsc_rs_ast::ModuleKind;

    fn codes(options: &CompilerOptions) -> Vec<u32> {
        check_deprecated_options(options)
            .into_iter()
            .map(|diag| diag.diagnostic.code)
            .collect()
    }

    #[test]
    fn validates_out_file_module_kinds_and_declaration_only_exception() {
        let mut options = CompilerOptions {
            out_file: Some("bundle.js".to_string()),
            declaration: Some(true),
            other: vec![("ignoreDeprecations".to_string(), "6.0".to_string())],
            ..Default::default()
        };
        for module in [
            ModuleKind::CommonJS,
            ModuleKind::UMD,
            ModuleKind::ES2015,
            ModuleKind::ES2020,
            ModuleKind::ES2022,
            ModuleKind::ESNext,
            ModuleKind::Node16,
            ModuleKind::Node18,
            ModuleKind::Node20,
            ModuleKind::NodeNext,
            ModuleKind::Preserve,
        ] {
            options.module = Some(module);
            let diagnostics = check_deprecated_options(&options);
            assert_eq!(diagnostics.len(), 1, "module={module:?}");
            let diagnostic = &diagnostics[0];
            assert_eq!(diagnostic.diagnostic.code, 6082);
            assert_eq!(
                diagnostic.diagnostic.message,
                "Only 'amd' and 'system' modules are supported alongside --outFile."
            );
            assert_eq!(diagnostic.option_name, "outFile");
            assert!(!diagnostic.point_at_value);
            options.emit_declaration_only = Some(true);
            assert!(codes(&options).is_empty(), "module={module:?}");
            options.emit_declaration_only = None;
        }
        for module in [
            None,
            Some(ModuleKind::None),
            Some(ModuleKind::AMD),
            Some(ModuleKind::System),
        ] {
            options.module = module;
            assert!(codes(&options).is_empty(), "module={module:?}");
        }
        options.module = Some(ModuleKind::CommonJS);
        options.no_emit = Some(true);
        assert_eq!(codes(&options), vec![6082]);
        options.out_file = None;
        assert!(codes(&options).is_empty());
    }

    #[test]
    fn validates_source_map_conflicts_and_map_root_dependencies() {
        let mut options = CompilerOptions {
            inline_source_map: Some(true),
            source_map: Some(true),
            declaration: Some(true),
            other: vec![("mapRoot".to_string(), "maps".to_string())],
            ..Default::default()
        };
        let diagnostics = check_deprecated_options(&options);
        assert_eq!(codes(&options), vec![5053, 5053]);
        assert_eq!(diagnostics[0].option_name, "mapRoot");
        assert_eq!(diagnostics[1].option_name, "sourceMap");
        assert_eq!(
            diagnostics[0].diagnostic.message,
            "Option 'mapRoot' cannot be specified with option 'inlineSourceMap'."
        );
        assert_eq!(
            diagnostics[1].diagnostic.message,
            "Option 'sourceMap' cannot be specified with option 'inlineSourceMap'."
        );
        options.source_map = Some(false);
        assert_eq!(codes(&options), vec![5053, 5069]);
        options.inline_source_map = Some(false);
        assert_eq!(codes(&options), vec![5069]);
        options
            .other
            .push(("declarationMap".to_string(), "true".to_string()));
        assert!(codes(&options).is_empty());
        options.other[1].1 = "false".to_string();
        assert_eq!(codes(&options), vec![5069]);
        options.source_map = Some(true);
        assert!(codes(&options).is_empty());
        options.source_map = None;
        options.other[0].1.clear();
        assert!(codes(&options).is_empty());
    }

    #[test]
    fn declaration_options_require_declaration_or_composite() {
        let mut options = CompilerOptions {
            emit_declaration_only: Some(true),
            isolated_declarations: Some(true),
            other: vec![
                ("declarationDir".into(), "types".into()),
                ("declarationMap".into(), "true".into()),
            ],
            ..Default::default()
        };
        let diagnostics = check_deprecated_options(&options);
        assert_eq!(diagnostics.len(), 4);
        for (diagnostic, name) in diagnostics.iter().zip([
            "declarationDir",
            "declarationMap",
            "emitDeclarationOnly",
            "isolatedDeclarations",
        ]) {
            assert_eq!(diagnostic.diagnostic.code, 5069);
            assert_eq!(diagnostic.option_name, name);
            assert!(!diagnostic.point_at_value);
            assert_eq!(diagnostic.diagnostic.message, format!(
                "Option '{name}' cannot be specified without specifying option 'declaration' or option 'composite'."));
        }
        options.no_emit = Some(true);
        assert_eq!(codes(&options), vec![5069; 4]);
        options.declaration = Some(true);
        assert!(codes(&options).is_empty());
        options.declaration = None;
        options.composite = Some(true);
        assert!(codes(&options).is_empty());
        options.composite = Some(false);
        options.emit_declaration_only = Some(false);
        options.isolated_declarations = Some(false);
        options.other[0].1.clear();
        options.other[1].1 = "false".into();
        assert!(codes(&options).is_empty());
    }

    #[test]
    fn validates_dependent_options() {
        let options = CompilerOptions {
            check_js: Some(true),
            allow_js: Some(false),
            emit_decorator_metadata: Some(true),
            exact_optional_property_types: Some(true),
            strict: Some(false),
            ..Default::default()
        };
        assert_eq!(codes(&options), vec![5052, 5052, 5052]);
    }

    #[test]
    fn rejects_lib_with_no_lib() {
        let options = CompilerOptions {
            no_lib: Some(true),
            lib: vec!["es5".to_string()],
            ..Default::default()
        };
        let diagnostics = check_deprecated_options(&options);

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].diagnostic.code, 5053);
        assert_eq!(
            diagnostics[0].diagnostic.message,
            "Option 'lib' cannot be specified with option 'noLib'."
        );
    }

    #[test]
    fn accepts_satisfied_dependent_options() {
        let options = CompilerOptions {
            check_js: Some(true),
            allow_js: Some(true),
            emit_decorator_metadata: Some(true),
            experimental_decorators: Some(true),
            exact_optional_property_types: Some(true),
            strict: Some(true),
            ..Default::default()
        };
        assert!(codes(&options).is_empty());

        let check_js_implies_allow_js = CompilerOptions {
            check_js: Some(true),
            ..Default::default()
        };
        assert!(codes(&check_js_implies_allow_js).is_empty());
    }

    #[test]
    fn validates_bundler_and_node_module_resolution_combinations() {
        let bundler = CompilerOptions {
            module: Some(ModuleKind::NodeNext),
            module_resolution: Some("bundler".into()),
            ..Default::default()
        };
        assert_eq!(codes(&bundler), vec![5095, 5109]);

        let node16 = CompilerOptions {
            module: Some(ModuleKind::CommonJS),
            module_resolution: Some("node16".into()),
            ..Default::default()
        };
        assert_eq!(codes(&node16), vec![5110]);

        let valid = CompilerOptions {
            module: Some(ModuleKind::NodeNext),
            module_resolution: Some("nodenext".into()),
            ..Default::default()
        };
        assert!(codes(&valid).is_empty());

        for module in [
            ModuleKind::Node16,
            ModuleKind::Node18,
            ModuleKind::Node20,
            ModuleKind::NodeNext,
        ] {
            for resolution in ["node16", "nodenext"] {
                let valid_node_pair = CompilerOptions {
                    module: Some(module),
                    module_resolution: Some(resolution.into()),
                    ..Default::default()
                };
                assert!(
                    codes(&valid_node_pair).is_empty(),
                    "{module:?} should accept {resolution} resolution"
                );
            }
        }

        for module in [ModuleKind::Node18, ModuleKind::Node20] {
            let invalid_node_pair = CompilerOptions {
                module: Some(module),
                module_resolution: Some("bundler".into()),
                ..Default::default()
            };
            assert_eq!(codes(&invalid_node_pair), vec![5095, 5109]);
        }
    }
}
