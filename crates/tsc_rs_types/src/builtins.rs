use std::sync::Arc;

// ---------------------------------------------------------------------------
// Built-in type declarations (minimal lib.d.ts)
// ---------------------------------------------------------------------------

use super::*;

#[derive(Clone)]
pub(crate) struct BuiltinTypes {
    pub(crate) instance_members: HashMap<std::string::String, ObjectTypeInfo>,
    pub(crate) global_values: HashMap<std::string::String, Type>,
    pub(crate) static_members: HashMap<std::string::String, ObjectTypeInfo>,
}

fn builtin_method(params: &[(&str, Type)], ret: Type) -> Type {
    Type::Function(FunctionType {
        type_param_constraints: Vec::new(),
        params: params
            .iter()
            .map(|(n, t)| (n.to_string(), t.clone()))
            .collect(),
        return_type: Arc::new(ret),
        type_params: Vec::new(),

        type_param_defaults: Vec::new(),
        type_predicate: None,
    })
}

fn bprop(name: &str, ty: Type) -> (std::string::String, Arc<Type>) {
    (name.to_string(), Arc::new(ty))
}

impl BuiltinTypes {
    pub(crate) fn new() -> Self {
        let mut instance_members = HashMap::new();
        let mut global_values = HashMap::new();
        let mut static_members = HashMap::new();

        instance_members.insert(
            "String".to_string(),
            ObjectTypeInfo::new(ObjectTypeData {
                properties: vec![
                    bprop("length", Type::Number),
                    bprop(
                        "charAt",
                        builtin_method(&[("pos", Type::Number)], Type::String),
                    ),
                    bprop(
                        "charCodeAt",
                        builtin_method(&[("index", Type::Number)], Type::Number),
                    ),
                    bprop(
                        "substring",
                        builtin_method(
                            &[("start", Type::Number), ("end", Type::Number)],
                            Type::String,
                        ),
                    ),
                    bprop(
                        "slice",
                        builtin_method(&[("start", Type::Number)], Type::String),
                    ),
                    bprop(
                        "indexOf",
                        builtin_method(&[("searchString", Type::String)], Type::Number),
                    ),
                    bprop(
                        "includes",
                        builtin_method(&[("searchString", Type::String)], Type::Boolean),
                    ),
                    bprop(
                        "startsWith",
                        builtin_method(&[("searchString", Type::String)], Type::Boolean),
                    ),
                    bprop(
                        "endsWith",
                        builtin_method(&[("searchString", Type::String)], Type::Boolean),
                    ),
                    bprop(
                        "split",
                        builtin_method(
                            // Real signature: `split(separator: string | RegExp, limit?: number)`.
                            // Modeling the union here unblocks `"a;b".split(/;/)`
                            // patterns that were rejected with `RegExp` not
                            // assignable to `string`.
                            &[
                                (
                                    "separator",
                                    Type::Union(Arc::from([
                                        Type::String,
                                        Type::TypeReference(
                                            "RegExp".to_string(),
                                            Arc::from([] as [Type; 0]),
                                        ),
                                    ])),
                                ),
                                ("limit", Type::Optional(Arc::new(Type::Number))),
                            ],
                            Type::Array(Arc::new(Type::String)),
                        ),
                    ),
                    bprop("trim", builtin_method(&[], Type::String)),
                    bprop(
                        "replace",
                        builtin_method(
                            &[("searchValue", Type::Any), ("replaceValue", Type::Any)],
                            Type::String,
                        ),
                    ),
                    bprop(
                        "match",
                        builtin_method(
                            &[("regexp", Type::Any)],
                            Type::Union(
                                vec![
                                    Type::TypeReference(
                                        "RegExpMatchArray".to_string(),
                                        Arc::from([] as [Type; 0]),
                                    ),
                                    Type::Null,
                                ]
                                .into(),
                            ),
                        ),
                    ),
                    bprop(
                        "search",
                        builtin_method(&[("regexp", Type::Any)], Type::Number),
                    ),
                    bprop("toUpperCase", builtin_method(&[], Type::String)),
                    bprop("toLowerCase", builtin_method(&[], Type::String)),
                    bprop(
                        "concat",
                        builtin_method(&[("str", Type::String)], Type::String),
                    ),
                    bprop(
                        "repeat",
                        builtin_method(&[("count", Type::Number)], Type::String),
                    ),
                    bprop(
                        "padStart",
                        builtin_method(&[("maxLength", Type::Number)], Type::String),
                    ),
                    bprop(
                        "padEnd",
                        builtin_method(&[("maxLength", Type::Number)], Type::String),
                    ),
                    bprop("toString", builtin_method(&[], Type::String)),
                    bprop("valueOf", builtin_method(&[], Type::String)),
                    bprop(
                        "replaceAll",
                        builtin_method(
                            &[("searchValue", Type::Any), ("replaceValue", Type::Any)],
                            Type::String,
                        ),
                    ),
                    bprop(
                        "matchAll",
                        builtin_method(&[("regexp", Type::Any)], Type::Any),
                    ),
                    bprop("trimStart", builtin_method(&[], Type::String)),
                    bprop("trimEnd", builtin_method(&[], Type::String)),
                    bprop(
                        "normalize",
                        builtin_method(&[("form", Type::String)], Type::String),
                    ),
                    bprop(
                        "at",
                        builtin_method(
                            &[("index", Type::Number)],
                            Type::Union(vec![Type::String, Type::Undefined].into()),
                        ),
                    ),
                    bprop(
                        "codePointAt",
                        builtin_method(
                            &[("pos", Type::Number)],
                            Type::Union(vec![Type::Number, Type::Undefined].into()),
                        ),
                    ),
                ],
                call_signatures: Vec::new(),
                construct_signatures: Vec::new(),
                index_signature: None,
                index_signature_name: None,
                method_names: Vec::new(),
            }),
        );

        instance_members.insert(
            "Number".to_string(),
            ObjectTypeInfo::new(ObjectTypeData {
                properties: vec![
                    bprop(
                        "toFixed",
                        builtin_method(&[("fractionDigits", Type::Number)], Type::String),
                    ),
                    bprop("toString", builtin_method(&[], Type::String)),
                    bprop("valueOf", builtin_method(&[], Type::Number)),
                    bprop(
                        "toPrecision",
                        builtin_method(&[("precision", Type::Number)], Type::String),
                    ),
                    bprop(
                        "toExponential",
                        builtin_method(&[("fractionDigits", Type::Number)], Type::String),
                    ),
                ],
                call_signatures: Vec::new(),
                construct_signatures: Vec::new(),
                index_signature: None,
                index_signature_name: None,
                method_names: Vec::new(),
            }),
        );

        instance_members.insert(
            "Boolean".to_string(),
            ObjectTypeInfo::new(ObjectTypeData {
                properties: vec![
                    bprop("valueOf", builtin_method(&[], Type::Boolean)),
                    bprop("toString", builtin_method(&[], Type::String)),
                ],
                call_signatures: Vec::new(),
                construct_signatures: Vec::new(),
                index_signature: None,
                index_signature_name: None,
                method_names: Vec::new(),
            }),
        );

        instance_members.insert(
            "Array".to_string(),
            ObjectTypeInfo::new(ObjectTypeData {
                properties: vec![
                    bprop("length", Type::Number),
                    bprop("push", builtin_method(&[("item", Type::Any)], Type::Number)),
                    bprop("pop", builtin_method(&[], Type::Any)),
                    bprop(
                        "map",
                        Type::Function(FunctionType {
                            type_param_constraints: Vec::new(),
                            params: vec![(
                                "callbackfn".to_string(),
                                Type::Function(FunctionType {
                                    type_param_constraints: Vec::new(),
                                    params: vec![
                                        ("value".to_string(), Type::Any),
                                        ("index".to_string(), Type::Number),
                                    ],
                                    return_type: Arc::new(Type::Any),
                                    type_params: Vec::new(),
                                    type_param_defaults: Vec::new(),
                                    type_predicate: None,
                                }),
                            )],
                            return_type: Arc::new(Type::Array(Arc::new(Type::Any))),
                            type_params: Vec::new(),

                            type_param_defaults: Vec::new(),
                            type_predicate: None,
                        }),
                    ),
                    bprop(
                        "filter",
                        Type::Function(FunctionType {
                            type_param_constraints: Vec::new(),
                            params: vec![(
                                "predicate".to_string(),
                                Type::Function(FunctionType {
                                    type_param_constraints: Vec::new(),
                                    params: vec![
                                        ("value".to_string(), Type::Any),
                                        ("index".to_string(), Type::Number),
                                    ],
                                    return_type: Arc::new(Type::Unknown),
                                    type_params: Vec::new(),
                                    type_param_defaults: Vec::new(),
                                    type_predicate: None,
                                }),
                            )],
                            return_type: Arc::new(Type::Array(Arc::new(Type::Any))),
                            type_params: Vec::new(),

                            type_param_defaults: Vec::new(),
                            type_predicate: None,
                        }),
                    ),
                    bprop(
                        "reduce",
                        builtin_method(
                            &[("callbackfn", Type::Any), ("initialValue", Type::Any)],
                            Type::Any,
                        ),
                    ),
                    bprop(
                        "forEach",
                        Type::Function(FunctionType {
                            type_param_constraints: Vec::new(),
                            params: vec![(
                                "callbackfn".to_string(),
                                Type::Function(FunctionType {
                                    type_param_constraints: Vec::new(),
                                    params: vec![
                                        ("value".to_string(), Type::Any),
                                        ("index".to_string(), Type::Number),
                                    ],
                                    return_type: Arc::new(Type::Void),
                                    type_params: Vec::new(),
                                    type_param_defaults: Vec::new(),
                                    type_predicate: None,
                                }),
                            )],
                            return_type: Arc::new(Type::Void),
                            type_params: Vec::new(),

                            type_param_defaults: Vec::new(),
                            type_predicate: None,
                        }),
                    ),
                    bprop(
                        "find",
                        Type::Function(FunctionType {
                            type_param_constraints: Vec::new(),
                            params: vec![(
                                "predicate".to_string(),
                                Type::Function(FunctionType {
                                    type_param_constraints: Vec::new(),
                                    params: vec![("value".to_string(), Type::Any)],
                                    return_type: Arc::new(Type::Unknown),
                                    type_params: Vec::new(),
                                    type_param_defaults: Vec::new(),
                                    type_predicate: None,
                                }),
                            )],
                            return_type: Arc::new(Type::Any),
                            type_params: Vec::new(),

                            type_param_defaults: Vec::new(),
                            type_predicate: None,
                        }),
                    ),
                    bprop(
                        "findIndex",
                        Type::Function(FunctionType {
                            type_param_constraints: Vec::new(),
                            params: vec![(
                                "predicate".to_string(),
                                Type::Function(FunctionType {
                                    type_param_constraints: Vec::new(),
                                    params: vec![("value".to_string(), Type::Any)],
                                    return_type: Arc::new(Type::Unknown),
                                    type_params: Vec::new(),
                                    type_param_defaults: Vec::new(),
                                    type_predicate: None,
                                }),
                            )],
                            return_type: Arc::new(Type::Number),
                            type_params: Vec::new(),

                            type_param_defaults: Vec::new(),
                            type_predicate: None,
                        }),
                    ),
                    bprop(
                        "indexOf",
                        builtin_method(&[("searchElement", Type::Any)], Type::Number),
                    ),
                    bprop(
                        "includes",
                        builtin_method(&[("searchElement", Type::Any)], Type::Boolean),
                    ),
                    bprop(
                        "slice",
                        builtin_method(
                            &[("start", Type::Number)],
                            Type::Array(Arc::new(Type::Any)),
                        ),
                    ),
                    bprop(
                        "concat",
                        builtin_method(&[("items", Type::Any)], Type::Array(Arc::new(Type::Any))),
                    ),
                    bprop(
                        "join",
                        builtin_method(&[("separator", Type::String)], Type::String),
                    ),
                    bprop(
                        "sort",
                        builtin_method(&[], Type::Array(Arc::new(Type::Any))),
                    ),
                    bprop(
                        "reverse",
                        builtin_method(&[], Type::Array(Arc::new(Type::Any))),
                    ),
                    bprop(
                        "splice",
                        builtin_method(
                            &[("start", Type::Number), ("deleteCount", Type::Number)],
                            Type::Array(Arc::new(Type::Any)),
                        ),
                    ),
                    bprop(
                        "some",
                        Type::Function(FunctionType {
                            type_param_constraints: Vec::new(),
                            params: vec![(
                                "predicate".to_string(),
                                Type::Function(FunctionType {
                                    type_param_constraints: Vec::new(),
                                    params: vec![("value".to_string(), Type::Any)],
                                    return_type: Arc::new(Type::Unknown),
                                    type_params: Vec::new(),
                                    type_param_defaults: Vec::new(),
                                    type_predicate: None,
                                }),
                            )],
                            return_type: Arc::new(Type::Boolean),
                            type_params: Vec::new(),

                            type_param_defaults: Vec::new(),
                            type_predicate: None,
                        }),
                    ),
                    bprop(
                        "every",
                        Type::Function(FunctionType {
                            type_param_constraints: Vec::new(),
                            params: vec![(
                                "predicate".to_string(),
                                Type::Function(FunctionType {
                                    type_param_constraints: Vec::new(),
                                    params: vec![("value".to_string(), Type::Any)],
                                    return_type: Arc::new(Type::Unknown),
                                    type_params: Vec::new(),
                                    type_param_defaults: Vec::new(),
                                    type_predicate: None,
                                }),
                            )],
                            return_type: Arc::new(Type::Boolean),
                            type_params: Vec::new(),

                            type_param_defaults: Vec::new(),
                            type_predicate: None,
                        }),
                    ),
                    bprop(
                        "flat",
                        builtin_method(&[], Type::Array(Arc::new(Type::Any))),
                    ),
                    bprop(
                        "flatMap",
                        Type::Function(FunctionType {
                            type_param_constraints: Vec::new(),
                            params: vec![(
                                "callback".to_string(),
                                Type::Function(FunctionType {
                                    type_param_constraints: Vec::new(),
                                    params: vec![("value".to_string(), Type::Any)],
                                    return_type: Arc::new(Type::Array(Arc::new(Type::Any))),
                                    type_params: Vec::new(),
                                    type_param_defaults: Vec::new(),
                                    type_predicate: None,
                                }),
                            )],
                            return_type: Arc::new(Type::Array(Arc::new(Type::Any))),
                            type_params: Vec::new(),

                            type_param_defaults: Vec::new(),
                            type_predicate: None,
                        }),
                    ),
                ],
                call_signatures: Vec::new(),
                construct_signatures: Vec::new(),
                index_signature: None,
                index_signature_name: None,
                method_names: Vec::new(),
            }),
        );

        instance_members.insert(
            "Promise".to_string(),
            ObjectTypeInfo::new(ObjectTypeData {
                properties: vec![
                    bprop(
                        "then",
                        Type::Function(FunctionType {
                            type_param_constraints: Vec::new(),
                            params: vec![(
                                "onfulfilled".to_string(),
                                Type::Function(FunctionType {
                                    type_param_constraints: Vec::new(),
                                    params: vec![("value".to_string(), Type::Any)],
                                    return_type: Arc::new(Type::Any),
                                    type_params: Vec::new(),
                                    type_param_defaults: Vec::new(),
                                    type_predicate: None,
                                }),
                            )],
                            return_type: Arc::new(Type::TypeReference(
                                "Promise".to_string(),
                                vec![Type::Any].into(),
                            )),
                            type_params: Vec::new(),

                            type_param_defaults: Vec::new(),
                            type_predicate: None,
                        }),
                    ),
                    bprop(
                        "catch",
                        Type::Function(FunctionType {
                            type_param_constraints: Vec::new(),
                            params: vec![(
                                "onrejected".to_string(),
                                Type::Function(FunctionType {
                                    type_param_constraints: Vec::new(),
                                    params: vec![("reason".to_string(), Type::Any)],
                                    return_type: Arc::new(Type::Any),
                                    type_params: Vec::new(),
                                    type_param_defaults: Vec::new(),
                                    type_predicate: None,
                                }),
                            )],
                            return_type: Arc::new(Type::TypeReference(
                                "Promise".to_string(),
                                vec![Type::Any].into(),
                            )),
                            type_params: Vec::new(),

                            type_param_defaults: Vec::new(),
                            type_predicate: None,
                        }),
                    ),
                    bprop(
                        "finally",
                        Type::Function(FunctionType {
                            type_param_constraints: Vec::new(),
                            params: vec![(
                                "onfinally".to_string(),
                                Type::Function(FunctionType {
                                    type_param_constraints: Vec::new(),
                                    params: Vec::new(),
                                    return_type: Arc::new(Type::Void),
                                    type_params: Vec::new(),
                                    type_param_defaults: Vec::new(),
                                    type_predicate: None,
                                }),
                            )],
                            return_type: Arc::new(Type::TypeReference(
                                "Promise".to_string(),
                                vec![Type::Any].into(),
                            )),
                            type_params: Vec::new(),

                            type_param_defaults: Vec::new(),
                            type_predicate: None,
                        }),
                    ),
                ],
                call_signatures: Vec::new(),
                construct_signatures: Vec::new(),
                index_signature: None,
                index_signature_name: None,
                method_names: Vec::new(),
            }),
        );

        instance_members.insert(
            "Map".to_string(),
            ObjectTypeInfo::new(ObjectTypeData {
                properties: vec![
                    bprop("size", Type::Number),
                    bprop("get", builtin_method(&[("key", Type::Any)], Type::Any)),
                    bprop(
                        "set",
                        builtin_method(&[("key", Type::Any), ("value", Type::Any)], Type::Any),
                    ),
                    bprop("has", builtin_method(&[("key", Type::Any)], Type::Boolean)),
                    bprop(
                        "delete",
                        builtin_method(&[("key", Type::Any)], Type::Boolean),
                    ),
                    bprop("clear", builtin_method(&[], Type::Void)),
                    bprop("keys", builtin_method(&[], Type::Any)),
                    bprop("values", builtin_method(&[], Type::Any)),
                    bprop("entries", builtin_method(&[], Type::Any)),
                    bprop(
                        "forEach",
                        Type::Function(FunctionType {
                            type_param_constraints: Vec::new(),
                            params: vec![(
                                "callbackfn".to_string(),
                                Type::Function(FunctionType {
                                    type_param_constraints: Vec::new(),
                                    params: vec![
                                        ("value".to_string(), Type::Any),
                                        ("key".to_string(), Type::Any),
                                    ],
                                    return_type: Arc::new(Type::Void),
                                    type_params: Vec::new(),
                                    type_param_defaults: Vec::new(),
                                    type_predicate: None,
                                }),
                            )],
                            return_type: Arc::new(Type::Void),
                            type_params: Vec::new(),

                            type_param_defaults: Vec::new(),
                            type_predicate: None,
                        }),
                    ),
                ],
                call_signatures: Vec::new(),
                construct_signatures: Vec::new(),
                index_signature: None,
                index_signature_name: None,
                method_names: Vec::new(),
            }),
        );

        instance_members.insert(
            "Set".to_string(),
            ObjectTypeInfo::new(ObjectTypeData {
                properties: vec![
                    bprop("size", Type::Number),
                    bprop("add", builtin_method(&[("value", Type::Any)], Type::Any)),
                    bprop(
                        "has",
                        builtin_method(&[("value", Type::Any)], Type::Boolean),
                    ),
                    bprop(
                        "delete",
                        builtin_method(&[("value", Type::Any)], Type::Boolean),
                    ),
                    bprop("clear", builtin_method(&[], Type::Void)),
                    bprop("keys", builtin_method(&[], Type::Any)),
                    bprop("values", builtin_method(&[], Type::Any)),
                    bprop("entries", builtin_method(&[], Type::Any)),
                    bprop(
                        "forEach",
                        Type::Function(FunctionType {
                            type_param_constraints: Vec::new(),
                            params: vec![(
                                "callbackfn".to_string(),
                                Type::Function(FunctionType {
                                    type_param_constraints: Vec::new(),
                                    params: vec![("value".to_string(), Type::Any)],
                                    return_type: Arc::new(Type::Void),
                                    type_params: Vec::new(),
                                    type_param_defaults: Vec::new(),
                                    type_predicate: None,
                                }),
                            )],
                            return_type: Arc::new(Type::Void),
                            type_params: Vec::new(),

                            type_param_defaults: Vec::new(),
                            type_predicate: None,
                        }),
                    ),
                ],
                call_signatures: Vec::new(),
                construct_signatures: Vec::new(),
                index_signature: None,
                index_signature_name: None,
                method_names: Vec::new(),
            }),
        );

        instance_members.insert(
            "RegExp".to_string(),
            ObjectTypeInfo::new(ObjectTypeData {
                properties: vec![
                    bprop("source", Type::String),
                    bprop("flags", Type::String),
                    bprop("global", Type::Boolean),
                    bprop("ignoreCase", Type::Boolean),
                    bprop("multiline", Type::Boolean),
                    bprop(
                        "test",
                        builtin_method(&[("string", Type::String)], Type::Boolean),
                    ),
                    bprop(
                        "exec",
                        builtin_method(
                            &[("string", Type::String)],
                            Type::Union(
                                vec![
                                    Type::TypeReference(
                                        "RegExpExecArray".to_string(),
                                        Arc::from([] as [Type; 0]),
                                    ),
                                    Type::Null,
                                ]
                                .into(),
                            ),
                        ),
                    ),
                ],
                call_signatures: Vec::new(),
                construct_signatures: Vec::new(),
                index_signature: None,
                index_signature_name: None,
                method_names: Vec::new(),
            }),
        );

        instance_members.insert(
            "Date".to_string(),
            ObjectTypeInfo::new(ObjectTypeData {
                properties: vec![
                    bprop("getTime", builtin_method(&[], Type::Number)),
                    bprop("getFullYear", builtin_method(&[], Type::Number)),
                    bprop("getMonth", builtin_method(&[], Type::Number)),
                    bprop("getDate", builtin_method(&[], Type::Number)),
                    bprop("getHours", builtin_method(&[], Type::Number)),
                    bprop("toISOString", builtin_method(&[], Type::String)),
                    bprop("toString", builtin_method(&[], Type::String)),
                ],
                call_signatures: Vec::new(),
                construct_signatures: Vec::new(),
                index_signature: None,
                index_signature_name: None,
                method_names: Vec::new(),
            }),
        );

        instance_members.insert(
            "Error".to_string(),
            ObjectTypeInfo::new(ObjectTypeData {
                properties: vec![
                    bprop("message", Type::String),
                    bprop("name", Type::String),
                    bprop(
                        "stack",
                        Type::Union(vec![Type::String, Type::Undefined].into()),
                    ),
                ],
                call_signatures: Vec::new(),
                construct_signatures: Vec::new(),
                index_signature: None,
                index_signature_name: None,
                method_names: Vec::new(),
            }),
        );

        // Object.prototype instance methods — inherited by all objects
        instance_members.insert(
            "Object".to_string(),
            ObjectTypeInfo::new(ObjectTypeData {
                properties: vec![
                    bprop(
                        "hasOwnProperty",
                        builtin_method(&[("v", Type::String)], Type::Boolean),
                    ),
                    bprop("toString", builtin_method(&[], Type::String)),
                    bprop("valueOf", builtin_method(&[], Type::Any)),
                    bprop("toLocaleString", builtin_method(&[], Type::String)),
                    bprop(
                        "isPrototypeOf",
                        builtin_method(&[("v", Type::Any)], Type::Boolean),
                    ),
                    bprop(
                        "propertyIsEnumerable",
                        builtin_method(&[("v", Type::String)], Type::Boolean),
                    ),
                    bprop("constructor", Type::Any),
                ],
                call_signatures: Vec::new(),
                construct_signatures: Vec::new(),
                index_signature: None,
                index_signature_name: None,
                method_names: Vec::new(),
            }),
        );

        global_values.insert(
            "Math".to_string(),
            Type::ObjectType(ObjectTypeInfo::new(ObjectTypeData {
                properties: vec![
                    bprop("PI", Type::Number),
                    bprop("E", Type::Number),
                    bprop("abs", builtin_method(&[("x", Type::Number)], Type::Number)),
                    bprop("ceil", builtin_method(&[("x", Type::Number)], Type::Number)),
                    bprop(
                        "floor",
                        builtin_method(&[("x", Type::Number)], Type::Number),
                    ),
                    bprop(
                        "round",
                        builtin_method(&[("x", Type::Number)], Type::Number),
                    ),
                    bprop(
                        "max",
                        builtin_method(&[("a", Type::Number), ("b", Type::Number)], Type::Number),
                    ),
                    bprop(
                        "min",
                        builtin_method(&[("a", Type::Number), ("b", Type::Number)], Type::Number),
                    ),
                    bprop("random", builtin_method(&[], Type::Number)),
                    bprop("sqrt", builtin_method(&[("x", Type::Number)], Type::Number)),
                    bprop(
                        "pow",
                        builtin_method(
                            &[("base", Type::Number), ("exponent", Type::Number)],
                            Type::Number,
                        ),
                    ),
                    bprop("log", builtin_method(&[("x", Type::Number)], Type::Number)),
                    bprop("sin", builtin_method(&[("x", Type::Number)], Type::Number)),
                    bprop("cos", builtin_method(&[("x", Type::Number)], Type::Number)),
                    bprop("tan", builtin_method(&[("x", Type::Number)], Type::Number)),
                    bprop(
                        "atan2",
                        builtin_method(&[("y", Type::Number), ("x", Type::Number)], Type::Number),
                    ),
                    bprop("asin", builtin_method(&[("x", Type::Number)], Type::Number)),
                    bprop("acos", builtin_method(&[("x", Type::Number)], Type::Number)),
                    bprop("atan", builtin_method(&[("x", Type::Number)], Type::Number)),
                    bprop("exp", builtin_method(&[("x", Type::Number)], Type::Number)),
                    bprop("log2", builtin_method(&[("x", Type::Number)], Type::Number)),
                    bprop(
                        "log10",
                        builtin_method(&[("x", Type::Number)], Type::Number),
                    ),
                    bprop(
                        "trunc",
                        builtin_method(&[("x", Type::Number)], Type::Number),
                    ),
                    bprop("sign", builtin_method(&[("x", Type::Number)], Type::Number)),
                    bprop("cbrt", builtin_method(&[("x", Type::Number)], Type::Number)),
                    bprop(
                        "hypot",
                        builtin_method(&[("a", Type::Number), ("b", Type::Number)], Type::Number),
                    ),
                    bprop(
                        "fround",
                        builtin_method(&[("x", Type::Number)], Type::Number),
                    ),
                    bprop(
                        "clz32",
                        builtin_method(&[("x", Type::Number)], Type::Number),
                    ),
                    bprop(
                        "imul",
                        builtin_method(&[("a", Type::Number), ("b", Type::Number)], Type::Number),
                    ),
                    // ES2015 Math methods
                    bprop(
                        "log1p",
                        builtin_method(&[("x", Type::Number)], Type::Number),
                    ),
                    bprop(
                        "expm1",
                        builtin_method(&[("x", Type::Number)], Type::Number),
                    ),
                    bprop("cosh", builtin_method(&[("x", Type::Number)], Type::Number)),
                    bprop("sinh", builtin_method(&[("x", Type::Number)], Type::Number)),
                    bprop("tanh", builtin_method(&[("x", Type::Number)], Type::Number)),
                    bprop(
                        "acosh",
                        builtin_method(&[("x", Type::Number)], Type::Number),
                    ),
                    bprop(
                        "asinh",
                        builtin_method(&[("x", Type::Number)], Type::Number),
                    ),
                    bprop(
                        "atanh",
                        builtin_method(&[("x", Type::Number)], Type::Number),
                    ),
                    bprop("LN2", Type::Number),
                    bprop("LN10", Type::Number),
                    bprop("LOG2E", Type::Number),
                    bprop("LOG10E", Type::Number),
                    bprop("SQRT2", Type::Number),
                    bprop("SQRT1_2", Type::Number),
                ],
                call_signatures: Vec::new(),
                construct_signatures: Vec::new(),
                index_signature: None,
                index_signature_name: None,
                method_names: Vec::new(),
            })),
        );

        global_values.insert(
            "JSON".to_string(),
            Type::ObjectType(ObjectTypeInfo::new(ObjectTypeData {
                properties: vec![
                    bprop(
                        "parse",
                        builtin_method(&[("text", Type::String)], Type::Any),
                    ),
                    bprop(
                        "stringify",
                        builtin_method(&[("value", Type::Any)], Type::String),
                    ),
                ],
                call_signatures: Vec::new(),
                construct_signatures: Vec::new(),
                index_signature: None,
                index_signature_name: None,
                method_names: Vec::new(),
            })),
        );

        global_values.insert(
            "console".to_string(),
            Type::ObjectType(ObjectTypeInfo::new(ObjectTypeData {
                properties: vec![
                    bprop("log", builtin_method(&[("message", Type::Any)], Type::Void)),
                    bprop(
                        "error",
                        builtin_method(&[("message", Type::Any)], Type::Void),
                    ),
                    bprop(
                        "warn",
                        builtin_method(&[("message", Type::Any)], Type::Void),
                    ),
                    bprop(
                        "info",
                        builtin_method(&[("message", Type::Any)], Type::Void),
                    ),
                    bprop(
                        "debug",
                        builtin_method(&[("message", Type::Any)], Type::Void),
                    ),
                    bprop(
                        "trace",
                        builtin_method(&[("message", Type::Any)], Type::Void),
                    ),
                ],
                call_signatures: Vec::new(),
                construct_signatures: Vec::new(),
                index_signature: None,
                index_signature_name: None,
                method_names: Vec::new(),
            })),
        );

        static_members.insert(
            "Object".to_string(),
            ObjectTypeInfo::new(ObjectTypeData {
                properties: vec![
                    bprop(
                        "keys",
                        builtin_method(&[("o", Type::Any)], Type::Array(Arc::new(Type::String))),
                    ),
                    bprop(
                        "values",
                        builtin_method(&[("o", Type::Any)], Type::Array(Arc::new(Type::Any))),
                    ),
                    bprop(
                        "entries",
                        builtin_method(&[("o", Type::Any)], Type::Array(Arc::new(Type::Any))),
                    ),
                    bprop(
                        "assign",
                        builtin_method(&[("target", Type::Any), ("source", Type::Any)], Type::Any),
                    ),
                    bprop("freeze", builtin_method(&[("o", Type::Any)], Type::Any)),
                    bprop("seal", builtin_method(&[("o", Type::Any)], Type::Any)),
                    bprop("create", builtin_method(&[("o", Type::Any)], Type::Any)),
                    bprop(
                        "defineProperty",
                        builtin_method(
                            &[
                                ("o", Type::Any),
                                ("p", Type::String),
                                ("attributes", Type::Any),
                            ],
                            Type::Any,
                        ),
                    ),
                    bprop(
                        "getOwnPropertyNames",
                        builtin_method(&[("o", Type::Any)], Type::Array(Arc::new(Type::String))),
                    ),
                    bprop(
                        "fromEntries",
                        builtin_method(&[("entries", Type::Any)], Type::Any),
                    ),
                    bprop(
                        "hasOwn",
                        builtin_method(&[("o", Type::Any), ("p", Type::String)], Type::Boolean),
                    ),
                    bprop(
                        "groupBy",
                        builtin_method(&[("items", Type::Any), ("keyFn", Type::Any)], Type::Any),
                    ),
                    bprop("prototype", Type::Any),
                    bprop(
                        "getOwnPropertyDescriptor",
                        builtin_method(&[("o", Type::Any), ("p", Type::String)], Type::Any),
                    ),
                    bprop(
                        "getPrototypeOf",
                        builtin_method(&[("o", Type::Any)], Type::Any),
                    ),
                    bprop(
                        "is",
                        builtin_method(&[("a", Type::Any), ("b", Type::Any)], Type::Boolean),
                    ),
                    bprop(
                        "setPrototypeOf",
                        builtin_method(&[("o", Type::Any), ("proto", Type::Any)], Type::Any),
                    ),
                    bprop(
                        "defineProperties",
                        builtin_method(&[("o", Type::Any), ("props", Type::Any)], Type::Any),
                    ),
                ],
                call_signatures: Vec::new(),
                construct_signatures: vec![crate::ConstructorType {
                    is_abstract: false,
                    params: vec![("?value".to_string(), Type::Any)],
                    return_type: Arc::new(Type::TypeReference(
                        "Object".to_string(),
                        Arc::from(Vec::<Type>::new()),
                    )),
                    type_params: Vec::new(),
                    type_param_constraints: Vec::new(),
                    type_param_defaults: Vec::new(),
                }],
                index_signature: None,
                index_signature_name: None,
                method_names: Vec::new(),
            }),
        );
        static_members.insert(
            "Date".to_string(),
            ObjectTypeInfo::new(ObjectTypeData {
                properties: vec![
                    bprop("now", builtin_method(&[], Type::Number)),
                    bprop(
                        "parse",
                        builtin_method(&[("s", Type::String)], Type::Number),
                    ),
                    bprop(
                        "UTC",
                        builtin_method(&[("year", Type::Number)], Type::Number),
                    ),
                ],
                call_signatures: Vec::new(),
                construct_signatures: Vec::new(),
                index_signature: None,
                index_signature_name: None,
                method_names: Vec::new(),
            }),
        );
        static_members.insert(
            "Array".to_string(),
            ObjectTypeInfo::new(ObjectTypeData {
                properties: vec![
                    bprop(
                        "isArray",
                        builtin_method(&[("arg", Type::Any)], Type::Boolean),
                    ),
                    bprop(
                        "from",
                        builtin_method(
                            &[("arrayLike", Type::Any)],
                            Type::Array(Arc::new(Type::Any)),
                        ),
                    ),
                    bprop(
                        "of",
                        builtin_method(&[("items", Type::Any)], Type::Array(Arc::new(Type::Any))),
                    ),
                    bprop(
                        "fromAsync",
                        builtin_method(
                            &[("asyncItems", Type::Any)],
                            Type::TypeReference(
                                "Promise".to_string(),
                                vec![Type::Array(Arc::new(Type::Any))].into(),
                            ),
                        ),
                    ),
                ],
                call_signatures: Vec::new(),
                construct_signatures: Vec::new(),
                index_signature: None,
                index_signature_name: None,
                method_names: Vec::new(),
            }),
        );
        static_members.insert(
            "Promise".to_string(),
            ObjectTypeInfo::new(ObjectTypeData {
                properties: vec![
                    bprop(
                        "resolve",
                        builtin_method(
                            &[("value", Type::Any)],
                            Type::TypeReference("Promise".to_string(), vec![Type::Any].into()),
                        ),
                    ),
                    bprop(
                        "reject",
                        builtin_method(
                            &[("reason", Type::Any)],
                            Type::TypeReference("Promise".to_string(), vec![Type::Never].into()),
                        ),
                    ),
                    bprop(
                        "all",
                        builtin_method(
                            &[("values", Type::Any)],
                            Type::TypeReference("Promise".to_string(), vec![Type::Any].into()),
                        ),
                    ),
                    bprop(
                        "race",
                        builtin_method(
                            &[("values", Type::Any)],
                            Type::TypeReference("Promise".to_string(), vec![Type::Any].into()),
                        ),
                    ),
                    bprop(
                        "allSettled",
                        builtin_method(
                            &[("values", Type::Any)],
                            Type::TypeReference("Promise".to_string(), vec![Type::Any].into()),
                        ),
                    ),
                    bprop(
                        "any",
                        builtin_method(
                            &[("values", Type::Any)],
                            Type::TypeReference("Promise".to_string(), vec![Type::Any].into()),
                        ),
                    ),
                    bprop("withResolvers", builtin_method(&[], Type::Any)),
                    bprop(
                        "try",
                        builtin_method(
                            &[(
                                "callbackfn",
                                Type::Function(FunctionType {
                                    type_param_constraints: Vec::new(),
                                    params: Vec::new(),
                                    return_type: Arc::new(Type::Any),
                                    type_params: Vec::new(),
                                    type_param_defaults: Vec::new(),
                                    type_predicate: None,
                                }),
                            )],
                            Type::TypeReference("Promise".to_string(), vec![Type::Any].into()),
                        ),
                    ),
                ],
                call_signatures: Vec::new(),
                construct_signatures: Vec::new(),
                index_signature: None,
                index_signature_name: None,
                method_names: Vec::new(),
            }),
        );

        // Every constructor function exposes `prototype` (the instance shape).
        for (name, info) in static_members.iter_mut() {
            if info.properties.iter().any(|(n, _)| n == "prototype") {
                continue;
            }
            let instance = if name == "Array" {
                Type::Array(Arc::new(Type::Any))
            } else {
                Type::TypeReference(name.clone(), Arc::from(Vec::<Type>::new()))
            };
            info.properties.push(bprop("prototype", instance));
        }

        BuiltinTypes {
            instance_members,
            global_values,
            static_members,
        }
    }

    pub(crate) fn lookup_instance_property(&self, type_name: &str, property: &str) -> Option<Type> {
        self.instance_members.get(type_name).and_then(|info| {
            info.properties
                .iter()
                .find(|(n, _)| n == property)
                .map(|(_, ty)| Type::clone(&ty))
        })
    }

    #[allow(dead_code)]
    pub(crate) fn lookup_static_property(&self, type_name: &str, property: &str) -> Option<Type> {
        self.static_members.get(type_name).and_then(|info| {
            info.properties
                .iter()
                .find(|(n, _)| n == property)
                .map(|(_, ty)| Type::clone(&ty))
        })
    }

    /// Lookup an array method with the actual element type substituted in.
    /// Returns properly-typed callback signatures for map/filter/find/etc.
    pub(crate) fn lookup_array_method_typed(&self, elem: &Type, method: &str) -> Option<Type> {
        let callback_params = || {
            vec![
                ("value".to_string(), elem.clone()),
                ("index".to_string(), Type::Number),
                ("array".to_string(), Type::Array(Arc::new(elem.clone()))),
            ]
        };
        let callback = |ret: Type| -> Type {
            Type::Function(FunctionType {
                type_param_constraints: Vec::new(),
                params: vec![(
                    "callbackfn".to_string(),
                    Type::Function(FunctionType {
                        type_param_constraints: Vec::new(),
                        params: callback_params(),
                        return_type: Arc::new(ret),
                        type_params: Vec::new(),
                        type_param_defaults: Vec::new(),
                        type_predicate: None,
                    }),
                )],
                return_type: Arc::new(Type::Array(Arc::new(Type::Any))),
                type_params: Vec::new(),
                type_param_defaults: Vec::new(),
                type_predicate: None,
            })
        };
        match method {
            // Property
            "length" => Some(Type::Number),
            // Methods with elem-typed callbacks
            "forEach" => Some(Type::Function(FunctionType {
                type_param_constraints: Vec::new(),
                params: vec![(
                    "callbackfn".to_string(),
                    Type::Function(FunctionType {
                        type_param_constraints: Vec::new(),
                        params: callback_params(),
                        return_type: Arc::new(Type::Void),
                        type_params: Vec::new(),
                        type_param_defaults: Vec::new(),
                        type_predicate: None,
                    }),
                )],
                return_type: Arc::new(Type::Void),
                type_params: Vec::new(),
                type_param_defaults: Vec::new(),
                type_predicate: None,
            })),
            "map" => Some(callback(Type::Any)),
            "filter" => Some(Type::Function(FunctionType {
                type_param_constraints: Vec::new(),
                params: vec![(
                    "predicate".to_string(),
                    Type::Function(FunctionType {
                        type_param_constraints: Vec::new(),
                        params: callback_params(),
                        return_type: Arc::new(Type::Unknown),
                        type_params: Vec::new(),
                        type_param_defaults: Vec::new(),
                        type_predicate: None,
                    }),
                )],
                return_type: Arc::new(Type::Array(Arc::new(elem.clone()))),
                type_params: Vec::new(),
                type_param_defaults: Vec::new(),
                type_predicate: None,
            })),
            "find" | "findLast" => Some(Type::Function(FunctionType {
                type_param_constraints: Vec::new(),
                params: vec![(
                    "predicate".to_string(),
                    Type::Function(FunctionType {
                        type_param_constraints: Vec::new(),
                        params: callback_params(),
                        return_type: Arc::new(Type::Unknown),
                        type_params: Vec::new(),
                        type_param_defaults: Vec::new(),
                        type_predicate: None,
                    }),
                )],
                return_type: Arc::new(Type::Union(vec![elem.clone(), Type::Undefined].into())),
                type_params: Vec::new(),
                type_param_defaults: Vec::new(),
                type_predicate: None,
            })),
            "findIndex" | "findLastIndex" => Some(Type::Function(FunctionType {
                type_param_constraints: Vec::new(),
                params: vec![(
                    "predicate".to_string(),
                    Type::Function(FunctionType {
                        type_param_constraints: Vec::new(),
                        params: callback_params(),
                        return_type: Arc::new(Type::Unknown),
                        type_params: Vec::new(),
                        type_param_defaults: Vec::new(),
                        type_predicate: None,
                    }),
                )],
                return_type: Arc::new(Type::Number),
                type_params: Vec::new(),
                type_param_defaults: Vec::new(),
                type_predicate: None,
            })),
            "some" | "every" => Some(Type::Function(FunctionType {
                type_param_constraints: Vec::new(),
                params: vec![(
                    "predicate".to_string(),
                    Type::Function(FunctionType {
                        type_param_constraints: Vec::new(),
                        params: callback_params(),
                        return_type: Arc::new(Type::Unknown),
                        type_params: Vec::new(),
                        type_param_defaults: Vec::new(),
                        type_predicate: None,
                    }),
                )],
                return_type: Arc::new(Type::Boolean),
                type_params: Vec::new(),
                type_param_defaults: Vec::new(),
                type_predicate: None,
            })),
            "reduce" => Some(Type::Function(FunctionType {
                type_param_constraints: Vec::new(),
                params: vec![
                    (
                        "callbackfn".to_string(),
                        Type::Function(FunctionType {
                            type_param_constraints: Vec::new(),
                            params: vec![
                                ("acc".to_string(), Type::Any),
                                ("value".to_string(), elem.clone()),
                                ("index".to_string(), Type::Number),
                                ("array".to_string(), Type::Array(Arc::new(elem.clone()))),
                            ],
                            return_type: Arc::new(Type::Any),
                            type_params: Vec::new(),
                            type_param_defaults: Vec::new(),
                            type_predicate: None,
                        }),
                    ),
                    ("initialValue".to_string(), Type::Any),
                ],
                return_type: Arc::new(Type::Any),
                type_params: Vec::new(),
                type_param_defaults: Vec::new(),
                type_predicate: None,
            })),
            "includes" => Some(builtin_method(
                &[("searchElement", elem.clone())],
                Type::Boolean,
            )),
            "indexOf" | "lastIndexOf" => Some(builtin_method(
                &[("searchElement", elem.clone())],
                Type::Number,
            )),
            "push" | "unshift" => Some(builtin_method(&[("item", elem.clone())], Type::Number)),
            "pop" | "shift" => Some(builtin_method(
                &[],
                Type::Union(vec![elem.clone(), Type::Undefined].into()),
            )),
            "slice" | "reverse" | "sort" | "toReversed" | "toSorted" => {
                Some(builtin_method(&[], Type::Array(Arc::new(elem.clone()))))
            }
            "concat" => Some(builtin_method(
                &[("items", Type::Any)],
                Type::Array(Arc::new(elem.clone())),
            )),
            "flat" => Some(builtin_method(&[], Type::Array(Arc::new(Type::Any)))),
            "join" => Some(builtin_method(&[], Type::String)),
            "at" => Some(builtin_method(
                &[("index", Type::Number)],
                Type::Union(vec![elem.clone(), Type::Undefined].into()),
            )),
            "fill" => Some(builtin_method(
                &[("value", elem.clone())],
                Type::Array(Arc::new(elem.clone())),
            )),
            _ => None, // Fall through to generic Array builtins
        }
    }
}
