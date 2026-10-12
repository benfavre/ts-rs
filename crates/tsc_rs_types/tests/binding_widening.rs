// Diagnostic expectations checked against TypeScript 6.0.3.
use tsc_rs_ast::{CompilerOptions, ScriptTarget};
use tsc_rs_types::TypeChecker;

fn verify(
    source: &str,
    strict_null_checks: bool,
    no_implicit_any: bool,
    expected: &[(u32, &str, u32, u32)],
) {
    let file = tsc_rs_parser::parse("subject.ts", source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let mut checker = TypeChecker::new();
    let mut output = checker.check_with_options(
        &file,
        &tsc_rs_symbols::bind(&file),
        &CompilerOptions {
            target: Some(ScriptTarget::ES2020),
            strict: Some(true),
            strict_null_checks: Some(strict_null_checks),
            no_implicit_any: Some(no_implicit_any),
            ..CompilerOptions::default()
        },
    );
    output
        .diagnostics
        .sort_by_key(|d| (d.span.map(|s| s.start), d.code));
    let actual = output
        .diagnostics
        .iter()
        .map(|d| {
            let s = d.span.unwrap();
            (d.code, d.message.as_str(), s.start, s.end - s.start)
        })
        .collect::<Vec<_>>();
    assert_eq!(actual, expected, "{source}");
}

#[test]
fn array_nullable_bindings() {
    verify(
        "const [a,b]=[undefined,null];",
        false,
        true,
        &[
            (
                7031,
                "Binding element 'a' implicitly has an 'any' type.",
                7,
                1,
            ),
            (
                7031,
                "Binding element 'b' implicitly has an 'any' type.",
                9,
                1,
            ),
        ],
    );
}

#[test]
fn array_mixed_positions() {
    verify(
        "const [a,b]=[null,1];",
        false,
        true,
        &[(
            7031,
            "Binding element 'a' implicitly has an 'any' type.",
            7,
            1,
        )],
    );
}

#[test]
fn array_const_assertion() {
    verify(
        "const [a,b]=[null,1] as const;",
        false,
        true,
        &[(
            7031,
            "Binding element 'a' implicitly has an 'any' type.",
            7,
            1,
        )],
    );
}

#[test]
fn array_regular_assertion() {
    verify("const [a,b]=[null as null,1];", false, true, &[]);
}

#[test]
fn array_annotated() {
    verify("const [a,b]:[any,number]=[null,1];", false, true, &[]);
}

#[test]
fn array_nested_binding() {
    verify(
        "const [[a]]= [[null]];",
        false,
        true,
        &[(
            7031,
            "Binding element 'a' implicitly has an 'any' type.",
            8,
            1,
        )],
    );
}

#[test]
fn array_object_binding() {
    verify(
        "const [{a}]=[{a:null}];",
        false,
        true,
        &[(
            7031,
            "Binding element 'a' implicitly has an 'any' type.",
            8,
            1,
        )],
    );
}

#[test]
fn object_array_binding() {
    verify(
        "const {a:[b]}={a:[null]};",
        false,
        true,
        &[(
            7031,
            "Binding element 'b' implicitly has an 'any' type.",
            10,
            1,
        )],
    );
}

#[test]
fn object_nested_binding() {
    verify(
        "const {a:{b}}={a:{b:null}};",
        false,
        true,
        &[(
            7031,
            "Binding element 'b' implicitly has an 'any' type.",
            10,
            1,
        )],
    );
}

#[test]
fn object_aliased_binding() {
    verify(
        "const {a:b}={a:null};",
        false,
        true,
        &[(
            7031,
            "Binding element 'b' implicitly has an 'any' type.",
            9,
            1,
        )],
    );
}

#[test]
fn array_bound_object() {
    verify(
        "const [a]=[{item:null}];",
        false,
        true,
        &[(
            7018,
            "Object literal's property 'item' implicitly has an 'any' type.",
            12,
            9,
        )],
    );
}

#[test]
fn array_rest_nullable() {
    verify(
        "const [...a]=[null];",
        false,
        true,
        &[(
            7031,
            "Binding element 'a' implicitly has an 'any[]' type.",
            10,
            1,
        )],
    );
}

#[test]
fn array_rest_after_binding() {
    verify(
        "const [a,...b]=[1,null];",
        false,
        true,
        &[(
            7031,
            "Binding element 'b' implicitly has an '[any]' type.",
            12,
            1,
        )],
    );
}

#[test]
fn array_rest_tuple() {
    verify(
        "const [a,...b]=[1,null] as const;",
        false,
        true,
        &[(
            7031,
            "Binding element 'b' implicitly has an '[any]' type.",
            12,
            1,
        )],
    );
}

#[test]
fn object_rest_nullable() {
    verify("const {...a}={item:null};", false, true, &[]);
}

#[test]
fn object_rest_after_binding() {
    verify("const {item,...a}={item:1,other:null};", false, true, &[]);
}

#[test]
fn array_default_null() {
    verify(
        "const [a=null]=[];",
        false,
        true,
        &[(
            7031,
            "Binding element 'a' implicitly has an 'any' type.",
            7,
            1,
        )],
    );
}

#[test]
fn array_default_undefined() {
    verify(
        "const [a=undefined]=[];",
        false,
        true,
        &[(
            7031,
            "Binding element 'a' implicitly has an 'any' type.",
            7,
            1,
        )],
    );
}

#[test]
fn array_default_concrete() {
    verify("const [a=1]=[undefined];", false, true, &[]);
}

#[test]
fn array_default_null_initializer() {
    verify("const [a=1]=[null];", false, true, &[]);
}

#[test]
fn array_default_null_and_value() {
    verify(
        "declare const data:(number|undefined)[];const [a=null]=data;",
        false,
        true,
        &[],
    );
}

#[test]
fn object_default_null() {
    verify(
        "const {a=null}={};",
        false,
        true,
        &[(
            7031,
            "Binding element 'a' implicitly has an 'any' type.",
            7,
            1,
        )],
    );
}

#[test]
fn object_default_concrete() {
    verify("const {a=1}={a:undefined};", false, true, &[]);
}

#[test]
fn object_default_null_initializer() {
    verify("const {a=1}={a:null};", false, true, &[]);
}

#[test]
fn object_alias_default() {
    verify(
        "const {a:b=null}={};",
        false,
        true,
        &[(
            7031,
            "Binding element 'b' implicitly has an 'any' type.",
            9,
            1,
        )],
    );
}

#[test]
fn object_nested_default() {
    verify(
        "const {a:{b}= {b:null}}={};",
        false,
        true,
        &[(
            7031,
            "Binding element 'b' implicitly has an 'any' type.",
            10,
            1,
        )],
    );
}

#[test]
fn array_nested_default() {
    verify(
        "const [[a]=[null]]=[];",
        false,
        true,
        &[(
            7031,
            "Binding element 'a' implicitly has an 'any' type.",
            8,
            1,
        )],
    );
}

#[test]
fn array_omitted_initializer() {
    verify(
        "const [a]=[,];",
        false,
        true,
        &[(
            7031,
            "Binding element 'a' implicitly has an 'any' type.",
            7,
            1,
        )],
    );
}

#[test]
fn array_binding_hole() {
    verify(
        "const [,a]=[1,null];",
        false,
        true,
        &[(
            7031,
            "Binding element 'a' implicitly has an 'any' type.",
            8,
            1,
        )],
    );
}

#[test]
fn array_explicit_any_source() {
    verify("const [a]=null as any;", false, true, &[]);
}

#[test]
fn parameter_array_initializer() {
    verify(
        "function f([a,b]=[undefined,null]){}",
        false,
        true,
        &[
            (
                7031,
                "Binding element 'a' implicitly has an 'any' type.",
                12,
                1,
            ),
            (
                7031,
                "Binding element 'b' implicitly has an 'any' type.",
                14,
                1,
            ),
        ],
    );
}

#[test]
fn parameter_nested_initializer() {
    verify(
        "function f({a:{b}}={a:{b:null}}){}",
        false,
        true,
        &[(
            7031,
            "Binding element 'b' implicitly has an 'any' type.",
            15,
            1,
        )],
    );
}

#[test]
fn parameter_array_default() {
    verify(
        "function f([a=null]){}",
        false,
        true,
        &[(
            7031,
            "Binding element 'a' implicitly has an 'any' type.",
            12,
            1,
        )],
    );
}

#[test]
fn parameter_object_default() {
    verify(
        "function f({a=null}){}",
        false,
        true,
        &[(
            7031,
            "Binding element 'a' implicitly has an 'any' type.",
            12,
            1,
        )],
    );
}

#[test]
fn parameter_array_double_default() {
    verify(
        "function f([a=null]=[]){}",
        false,
        true,
        &[(
            7031,
            "Binding element 'a' implicitly has an 'any' type.",
            12,
            1,
        )],
    );
}

#[test]
fn parameter_object_double_default() {
    verify(
        "function f({a=null}={}){}",
        false,
        true,
        &[(
            7031,
            "Binding element 'a' implicitly has an 'any' type.",
            12,
            1,
        )],
    );
}

#[test]
fn parameter_rest_initializer() {
    verify(
        "function f([...a]=[null]){}",
        false,
        true,
        &[(
            7031,
            "Binding element 'a' implicitly has an 'any[]' type.",
            15,
            1,
        )],
    );
}

#[test]
fn contextual_array_parameter() {
    verify(
        "const f: (arg:[any])=>void=([a]=[null])=>{};",
        false,
        true,
        &[],
    );
}

#[test]
fn contextual_default_parameter() {
    verify(
        "const f: (arg:[number?])=>void=([a=null])=>{};",
        false,
        true,
        &[],
    );
}

#[test]
fn contextual_object_parameter() {
    verify(
        "const f: (arg:{a:any})=>void=({a}={a:null})=>{};",
        false,
        true,
        &[],
    );
}

#[test]
fn array_no_initializer_default() {
    verify(
        "let [a=null];",
        false,
        true,
        &[
            (
                1182,
                "A destructuring declaration must have an initializer.",
                4,
                8,
            ),
            (
                7031,
                "Binding element 'a' implicitly has an 'any' type.",
                5,
                1,
            ),
        ],
    );
}

#[test]
fn object_no_initializer_default() {
    verify(
        "let {a=null};",
        false,
        true,
        &[
            (
                1182,
                "A destructuring declaration must have an initializer.",
                4,
                8,
            ),
            (
                7031,
                "Binding element 'a' implicitly has an 'any' type.",
                5,
                1,
            ),
        ],
    );
}

#[test]
fn array_nullable_bindings_strict() {
    verify("const [a,b]=[undefined,null];", true, true, &[]);
}

#[test]
fn array_nullable_bindings_disabled() {
    verify("const [a,b]=[undefined,null];", false, false, &[]);
}

#[test]
fn array_mixed_positions_strict() {
    verify("const [a,b]=[null,1];", true, true, &[]);
}

#[test]
fn array_mixed_positions_disabled() {
    verify("const [a,b]=[null,1];", false, false, &[]);
}

#[test]
fn array_rest_nullable_strict() {
    verify("const [...a]=[null];", true, true, &[]);
}

#[test]
fn array_rest_nullable_disabled() {
    verify("const [...a]=[null];", false, false, &[]);
}

#[test]
fn array_rest_after_binding_strict() {
    verify("const [a,...b]=[1,null];", true, true, &[]);
}

#[test]
fn array_rest_after_binding_disabled() {
    verify("const [a,...b]=[1,null];", false, false, &[]);
}

#[test]
fn array_default_null_strict() {
    verify("const [a=null]=[];", true, true, &[]);
}

#[test]
fn array_default_null_disabled() {
    verify("const [a=null]=[];", false, false, &[]);
}

#[test]
fn object_default_null_strict() {
    verify("const {a=null}={};", true, true, &[]);
}

#[test]
fn object_default_null_disabled() {
    verify("const {a=null}={};", false, false, &[]);
}

#[test]
fn parameter_array_default_strict() {
    verify("function f([a=null]){}", true, true, &[]);
}

#[test]
fn parameter_array_default_disabled() {
    verify("function f([a=null]){}", false, false, &[]);
}

#[test]
fn parameter_array_initializer_strict() {
    verify("function f([a,b]=[undefined,null]){}", true, true, &[]);
}

#[test]
fn parameter_array_initializer_disabled() {
    verify("function f([a,b]=[undefined,null]){}", false, false, &[]);
}

#[test]
fn object_nested_binding_strict() {
    verify("const {a:{b}}={a:{b:null}};", true, true, &[]);
}

#[test]
fn object_nested_binding_disabled() {
    verify("const {a:{b}}={a:{b:null}};", false, false, &[]);
}

#[test]
fn array_no_initializer_default_strict() {
    verify(
        "let [a=null];",
        true,
        true,
        &[(
            1182,
            "A destructuring declaration must have an initializer.",
            4,
            8,
        )],
    );
}

#[test]
fn array_no_initializer_default_disabled() {
    verify(
        "let [a=null];",
        false,
        false,
        &[(
            1182,
            "A destructuring declaration must have an initializer.",
            4,
            8,
        )],
    );
}

#[test]
fn conditional_same_objects() {
    verify(
        "const {a}=true?{a:null}:{a:null};",
        false,
        true,
        &[(
            7031,
            "Binding element 'a' implicitly has an 'any' type.",
            7,
            1,
        )],
    );
}

#[test]
fn conditional_nullable_objects() {
    verify(
        "const {a}=true?{a:null}:{a:undefined};",
        false,
        true,
        &[(
            7031,
            "Binding element 'a' implicitly has an 'any' type.",
            7,
            1,
        )],
    );
}

#[test]
fn conditional_regular_null_object() {
    verify(
        "const {a}=true?{a:null}:{a:null as null};",
        false,
        true,
        &[],
    );
}

#[test]
fn conditional_concrete_object() {
    verify("const {a}=true?{a:null}:{a:1};", false, true, &[]);
}

#[test]
fn conditional_arrays() {
    verify(
        "const [a]=true?[null]:[null];",
        false,
        true,
        &[(
            7031,
            "Binding element 'a' implicitly has an 'any' type.",
            7,
            1,
        )],
    );
}

#[test]
fn conditional_rest_arrays() {
    verify(
        "const [...a]=true?[null]:[null];",
        false,
        true,
        &[(
            7031,
            "Binding element 'a' implicitly has an 'any[]' type.",
            10,
            1,
        )],
    );
}

#[test]
fn conditional_asserted_tuples() {
    verify(
        "const [a,b]=true?[null,1] as const:[null,2] as const;",
        false,
        true,
        &[(
            7031,
            "Binding element 'a' implicitly has an 'any' type.",
            7,
            1,
        )],
    );
}

#[test]
fn conditional_nested_objects() {
    verify(
        "const {a:{b}}=true?{a:{b:null}}:{a:{b:null}};",
        false,
        true,
        &[(
            7031,
            "Binding element 'b' implicitly has an 'any' type.",
            10,
            1,
        )],
    );
}

#[test]
fn conditional_nested_default() {
    verify(
        "const {a:{b}={b:null}}=true?{a:undefined}:{};",
        false,
        true,
        &[(
            7031,
            "Binding element 'b' implicitly has an 'any' type.",
            10,
            1,
        )],
    );
}

#[test]
fn conditional_array_default() {
    verify(
        "const [a=null]=true?[undefined]:[];",
        false,
        true,
        &[(
            7031,
            "Binding element 'a' implicitly has an 'any' type.",
            7,
            1,
        )],
    );
}

#[test]
fn array_property_selection() {
    verify(
        "const {0:a}=[null,1];",
        false,
        true,
        &[(
            7031,
            "Binding element 'a' implicitly has an 'any' type.",
            9,
            1,
        )],
    );
}

#[test]
fn computed_object_key() {
    verify(
        "const key=\"item\";const {[key]:a}={item:null};",
        false,
        true,
        &[(
            7031,
            "Binding element 'a' implicitly has an 'any' type.",
            30,
            1,
        )],
    );
}

#[test]
fn rest_array_of_objects() {
    verify(
        "const [...a]=[{item:null}];",
        false,
        true,
        &[(
            7018,
            "Object literal's property 'item' implicitly has an 'any' type.",
            15,
            9,
        )],
    );
}

#[test]
fn rest_nested_pattern() {
    verify(
        "const [...[a]]=[null];",
        false,
        true,
        &[(
            7031,
            "Binding element 'a' implicitly has an 'any' type.",
            11,
            1,
        )],
    );
}

#[test]
fn rest_after_hole() {
    verify(
        "const [,...a]=[1,null];",
        false,
        true,
        &[(
            7031,
            "Binding element 'a' implicitly has an '[any]' type.",
            11,
            1,
        )],
    );
}

#[test]
fn nested_default_array_mixed() {
    verify(
        "const [[a,b]=[null,1]]=[];",
        false,
        true,
        &[(
            7031,
            "Binding element 'a' implicitly has an 'any' type.",
            8,
            1,
        )],
    );
}

#[test]
fn object_default_regular_source() {
    verify("const {a=null}={} as {a?:number};", false, true, &[]);
}

#[test]
fn array_default_regular_null() {
    verify("const [a=null]=[null as null];", false, true, &[]);
}

#[test]
fn array_spread_regular_any() {
    verify(
        "declare const values:any[];const [a]=[null,...values];",
        false,
        true,
        &[(
            7031,
            "Binding element 'a' implicitly has an 'any' type.",
            34,
            1,
        )],
    );
}

#[test]
fn array_rest_inferred_empty() {
    verify(
        "const [...a]=[];",
        false,
        true,
        &[(
            7031,
            "Binding element 'a' implicitly has an 'any[]' type.",
            10,
            1,
        )],
    );
}

#[test]
fn array_rest_inferred_tuple_empty() {
    verify("const [a,...b]=[1];", false, true, &[]);
}

#[test]
fn parameter_with_mixed_initializer() {
    verify(
        "function f([a,b]=[null,1]){}",
        false,
        true,
        &[(
            7031,
            "Binding element 'a' implicitly has an 'any' type.",
            12,
            1,
        )],
    );
}

#[test]
fn parameter_nested_object_array() {
    verify(
        "function f({a:[b]}={a:[null]}){}",
        false,
        true,
        &[(
            7031,
            "Binding element 'b' implicitly has an 'any' type.",
            15,
            1,
        )],
    );
}

#[test]
fn object_literal_bound_to_rest() {
    verify(
        "const [...a]=[[null,1] as const];",
        false,
        true,
        &[(
            7031,
            "Binding element 'a' implicitly has an '(readonly [any, 1])[]' type.",
            10,
            1,
        )],
    );
}
