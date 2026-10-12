// Exact types and diagnostics checked against TypeScript 6.0.3.
use tsc_rs_ast::{CompilerOptions, ScriptTarget};
use tsc_rs_types::TypeChecker;

fn verify_options(
    source: &str,
    unchecked: bool,
    strict: bool,
    expected_type: &str,
    expected: &[(u32, &str, u32, u32)],
) {
    let file = tsc_rs_parser::parse("subject.ts", source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let output = TypeChecker::new().check_with_options(
        &file,
        &tsc_rs_symbols::bind(&file),
        &CompilerOptions {
            target: Some(ScriptTarget::ES2015),
            strict: Some(strict),
            no_unchecked_indexed_access: Some(unchecked),
            ..CompilerOptions::default()
        },
    );
    let mut actual = output
        .diagnostics
        .iter()
        .map(|d| {
            let span = d.span.unwrap();
            (
                d.code,
                d.message.as_str(),
                span.start,
                span.end - span.start,
            )
        })
        .collect::<Vec<_>>();
    actual.sort_by_key(|d| (d.2, d.0));
    assert_eq!(actual, expected, "{source}");
    let position = source.rfind("result;").unwrap() as u32;
    assert_eq!(
        output.expression_types.get(&position).map(String::as_str),
        Some(expected_type),
        "{source}: {:?}",
        output.expression_types
    );
}

#[test]
fn binding_past() {
    verify_options(
        "declare const tuple:[string];const [first,result]=tuple; result;",
        false,
        true,
        "undefined",
        &[(
            2493,
            "Tuple type '[string]' of length '1' has no element at index '1'.",
            42,
            6,
        )],
    );
}

#[test]
fn binding_default() {
    verify_options(
        "declare const tuple:[string];const [,result=\"fallback\"]=tuple; result;",
        false,
        true,
        "\"fallback\"",
        &[],
    );
}

#[test]
fn binding_nested() {
    verify_options(
        "declare const tuple:[[string]];const [[first,result]]=tuple; result;",
        false,
        true,
        "undefined",
        &[(
            2493,
            "Tuple type '[string]' of length '1' has no element at index '1'.",
            45,
            6,
        )],
    );
}

#[test]
fn binding_optional() {
    verify_options(
        "declare const tuple:[string,number?];const [first,result]=tuple; result;",
        false,
        true,
        "number | undefined",
        &[],
    );
}

#[test]
fn binding_rest() {
    verify_options(
        "declare const tuple:[string,...number[]];const [first,result]=tuple; result;",
        false,
        true,
        "number",
        &[],
    );
}

#[test]
fn binding_rest_empty() {
    verify_options(
        "declare const tuple:[string];const [first,...result]=tuple; result;",
        false,
        true,
        "[]",
        &[],
    );
}

#[test]
fn binding_rest_after_end() {
    verify_options(
        "declare const tuple:[string];const [first,second,...result]=tuple; result;",
        false,
        true,
        "[]",
        &[(
            2493,
            "Tuple type '[string]' of length '1' has no element at index '1'.",
            42,
            6,
        )],
    );
}

#[test]
fn binding_parameter() {
    verify_options(
        "function f([first,second]:[string]){return second;}const result=f([\"\"]); result;",
        false,
        true,
        "undefined",
        &[(
            2493,
            "Tuple type '[string]' of length '1' has no element at index '1'.",
            18,
            6,
        )],
    );
}

#[test]
fn binding_assignment() {
    verify_options(
        "declare const tuple:[string];let result:number;[,result]=tuple; result;",
        false,
        true,
        "number",
        &[
            (
                2322,
                "Type 'undefined' is not assignable to type 'number'.",
                49,
                6,
            ),
            (
                2493,
                "Tuple type '[string]' of length '1' has no element at index '1'.",
                49,
                6,
            ),
        ],
    );
}

#[test]
fn binding_union() {
    verify_options(
        "declare const tuple:[string]|[number,boolean];const [first,result]=tuple; result;",
        false,
        true,
        "boolean | undefined",
        &[],
    );
}

#[test]
fn readonly_shadow() {
    verify_options(
        "declare let tuple:readonly [string,number];function f(){let tuple:[string,number]=[\"\",0];tuple[1]=1;return tuple[1];}const result=f(); result;",
        false,
        true,
        "number",
        &[
        ],
    );
}

#[test]
fn named_shadow() {
    verify_options(
        "declare const tuple:[first:string,second:number];function f(tuple:[boolean]){return tuple[1];}const result=f([true]); result;",
        false,
        true,
        "undefined",
        &[
            (2493, "Tuple type '[boolean]' of length '1' has no element at index '1'.", 90, 1),
        ],
    );
}

#[test]
fn alias_binding() {
    verify_options(
        "type Tuple=[string,number?];declare const tuple:Tuple;const [,result]=tuple; result;",
        false,
        true,
        "number | undefined",
        &[],
    );
}

#[test]
fn alias_binding_past() {
    verify_options(
        "type Tuple=[string];declare const tuple:Tuple;const [,result]=tuple; result;",
        false,
        true,
        "undefined",
        &[(
            2493,
            "Tuple type 'Tuple' of length '1' has no element at index '1'.",
            54,
            6,
        )],
    );
}

#[test]
fn named_binding_past() {
    verify_options(
        "declare const tuple:[first:string];const [,result]=tuple; result;",
        false,
        true,
        "undefined",
        &[(
            2493,
            "Tuple type '[first: string]' of length '1' has no element at index '1'.",
            43,
            6,
        )],
    );
}

#[test]
fn named_parameter_past() {
    verify_options(
        "function f([first,second]:[first:string]){return second;}const result=f([\"\"]); result;",
        false,
        true,
        "undefined",
        &[(
            2493,
            "Tuple type '[first: string]' of length '1' has no element at index '1'.",
            18,
            6,
        )],
    );
}

#[test]
fn rest_array() {
    verify_options(
        "declare const tuple:[string,...number[]];const [first,...result]=tuple; result;",
        false,
        true,
        "number[]",
        &[],
    );
}

#[test]
fn rest_array_after() {
    verify_options(
        "declare const tuple:[string,...number[]];const [first,second,...result]=tuple; result;",
        false,
        true,
        "number[]",
        &[],
    );
}

#[test]
fn rest_optional() {
    verify_options(
        "declare const tuple:[string,number?];const [first,...result]=tuple; result;",
        false,
        true,
        "[(number | undefined)?]",
        &[],
    );
}

#[test]
fn rest_union() {
    verify_options(
        "declare const tuple:[string]|[number,boolean];const [first,...result]=tuple; result;",
        false,
        true,
        "[] | [boolean]",
        &[],
    );
}

#[test]
fn rest_alias() {
    verify_options(
        "type Tuple=[string,number];declare const tuple:Tuple;const [first,...result]=tuple; result;",
        false,
        true,
        "[number]",
        &[
        ],
    );
}

#[test]
fn rest_middle() {
    verify_options(
        "declare const tuple:[string,...number[],boolean];const [first,second,...result]=tuple; result;",
        false,
        true,
        "(number | boolean)[]",
        &[
        ],
    );
}

#[test]
fn assignment_default() {
    verify_options(
        "declare const tuple:[string];let result:number;[,result=1]=tuple; result;",
        false,
        true,
        "number",
        &[],
    );
}

#[test]
fn assignment_default_invalid() {
    verify_options(
        "declare const tuple:[number];let result:number;[result=\"bad\"]=tuple; result;",
        false,
        true,
        "number",
        &[(
            2322,
            "Type 'string' is not assignable to type 'number'.",
            48,
            6,
        )],
    );
}

#[test]
fn assignment_nested() {
    verify_options(
        "declare const tuple:[[string]];let result:number;[[,result]]=tuple; result;",
        false,
        true,
        "number",
        &[
            (
                2322,
                "Type 'undefined' is not assignable to type 'number'.",
                52,
                6,
            ),
            (
                2493,
                "Tuple type '[string]' of length '1' has no element at index '1'.",
                52,
                6,
            ),
        ],
    );
}

#[test]
fn assignment_valid() {
    verify_options(
        "declare const tuple:[string,number];let result:number;[,result]=tuple; result;",
        false,
        true,
        "number",
        &[],
    );
}

#[test]
fn assignment_wrong() {
    verify_options(
        "declare const tuple:[string,number];let result:string;[,result]=tuple; result;",
        false,
        true,
        "string",
        &[(
            2322,
            "Type 'number' is not assignable to type 'string'.",
            56,
            6,
        )],
    );
}

#[test]
fn assignment_rest() {
    verify_options(
        "declare const tuple:[string,number];let result:string[];[,...result]=tuple; result;",
        false,
        true,
        "string[]",
        &[
            (2322, "Type '[number]' is not assignable to type 'string[]'.\n  Type 'number' is not assignable to type 'string'.", 61, 6),
        ],
    );
}

#[test]
fn assignment_optional() {
    verify_options(
        "declare const tuple:[string,number?];let result:number;[,result]=tuple; result;",
        false,
        true,
        "number",
        &[
            (2322, "Type 'number | undefined' is not assignable to type 'number'.\n  Type 'undefined' is not assignable to type 'number'.", 57, 6),
        ],
    );
}

#[test]
fn assignment_constant() {
    verify_options(
        "declare const tuple:[number];const result=0;[result]=tuple; result;",
        false,
        true,
        "0",
        &[(
            2588,
            "Cannot assign to 'result' because it is a constant.",
            45,
            6,
        )],
    );
}

#[test]
fn array_unchecked() {
    verify_options(
        "declare const tuple:number[];const [result]=tuple; result;",
        true,
        true,
        "number | undefined",
        &[],
    );
}

#[test]
fn fixed_unchecked() {
    verify_options(
        "declare const tuple:[string,number];const [,result]=tuple; result;",
        true,
        true,
        "number",
        &[],
    );
}

#[test]
fn rest_unchecked() {
    verify_options(
        "declare const tuple:[string,...number[]];const [,result]=tuple; result;",
        true,
        true,
        "number | undefined",
        &[],
    );
}

#[test]
fn generic_binding() {
    verify_options(
        "function f<T extends [string,number?]>(tuple:T){const [,result]=tuple; return result;} const result=f([\"\"]); result;",
        false,
        true,
        "number | undefined",
        &[
        ],
    );
}

#[test]
fn union_array_binding() {
    verify_options(
        "declare const tuple:[string,number]|boolean[];const [,result]=tuple; result;",
        false,
        true,
        "number | boolean",
        &[],
    );
}

#[test]
fn array_parameter_unchecked() {
    verify_options(
        "function f([result]:number[]){return result;}const result=f([]); result;",
        true,
        true,
        "number | undefined",
        &[],
    );
}

#[test]
fn nonstrict_optional() {
    verify_options(
        "declare const tuple:[string,number?];const [,result]=tuple; result;",
        false,
        false,
        "number",
        &[],
    );
}

#[test]
fn optional_default() {
    verify_options(
        "declare const tuple:[string,number?];const [,result=0]=tuple; result;",
        false,
        true,
        "number",
        &[],
    );
}

#[test]
fn elisions_past() {
    verify_options(
        "declare const tuple:[string];const [,,...result]=tuple; result;",
        false,
        true,
        "[]",
        &[],
    );
}

#[test]
fn assignment_literal_valid() {
    verify_options(
        "let result:number;[result]=[1]; result;",
        false,
        true,
        "number",
        &[],
    );
}

#[test]
fn assignment_literal_wrong() {
    verify_options(
        "let result:number;[result]=[\"bad\"]; result;",
        false,
        true,
        "number",
        &[(
            2322,
            "Type 'string' is not assignable to type 'number'.",
            19,
            6,
        )],
    );
}

#[test]
fn assignment_rhs_read() {
    verify_options(
        "let result:number;[result]=[result]; result;",
        false,
        true,
        "number",
        &[(
            2454,
            "Variable 'result' is used before being assigned.",
            28,
            6,
        )],
    );
}

#[test]
fn assignment_object_nested() {
    verify_options(
        "declare const tuple:[{value:string}];let result:number;[{value:result}]=tuple; result;",
        false,
        true,
        "number",
        &[(
            2322,
            "Type 'string' is not assignable to type 'number'.",
            63,
            6,
        )],
    );
}

#[test]
fn assignment_member() {
    verify_options("declare const tuple:[number];let object:{value:number}={value:0};[object.value]=tuple;const result=object.value; result;",false,true,"number",&[]);
}

#[test]
fn assignment_readonly() {
    verify_options("declare const tuple:[number];declare const target:readonly [number];[target[0]]=tuple;const result=target[0]; result;",false,true,"number",&[(2540,"Cannot assign to '0' because it is a read-only property.",76,1)]);
}

#[test]
fn assignment_optional_chain() {
    verify_options("declare const tuple:[number];declare let object:{value:number}|undefined;[object?.value]=tuple;const result=0; result;",false,true,"0",&[(2779,"The left-hand side of an assignment expression may not be an optional property access.",74,13)]);
}

#[test]
fn assignment_noniterable() {
    verify_options(
        "let result:number;[result]=1; result;",
        false,
        true,
        "number",
        &[(
            2488,
            "Type '1' must have a '[Symbol.iterator]()' method that returns an iterator.",
            18,
            8,
        )],
    );
}

#[test]
fn assignment_null() {
    verify_options(
        "let result:number;[result]=null; result;",
        false,
        true,
        "number",
        &[(
            2488,
            "Type 'null' must have a '[Symbol.iterator]()' method that returns an iterator.",
            18,
            8,
        )],
    );
}

#[test]
fn generic_parameter() {
    verify_options("function f<T extends [string,number?]>([first,result]:T){return result;} const result=f([\"\"]); result;",false,true,"number | undefined",&[]);
}

#[test]
fn generic_parameter_past() {
    verify_options("function f<T extends [string]>([first,second]:T){return second;} const result=f([\"\"]); result;",false,true,"undefined",&[(2493,"Tuple type '[string]' of length '1' has no element at index '1'.",38,6)]);
}

#[test]
fn named_nested_binding() {
    verify_options(
        "declare const tuple:[[first:string]];const [[first,result]]=tuple; result;",
        false,
        true,
        "undefined",
        &[(
            2493,
            "Tuple type '[first: string]' of length '1' has no element at index '1'.",
            51,
            6,
        )],
    );
}

#[test]
fn parameter_default() {
    verify_options(
        "function f([first,second=1]:[string]){return second;}const result=f([\"\"]); result;",
        false,
        true,
        "never",
        &[(2322, "Type '1' is not assignable to type 'never'.", 18, 6)],
    );
}

#[test]
fn parameter_default_reference() {
    verify_options("function f([first,second=first]:[number,number?]){return second;}const result=f([1]); result;",false,true,"number",&[]);
}

#[test]
fn assignment_default_union() {
    verify_options("declare const tuple:[number?];let result:number|string;[result=\"fallback\"]=tuple; result;",false,true,"string | number",&[]);
}

#[test]
fn assignment_wrapper_satisfies() {
    verify_options(
        "let result:number;[(result satisfies number)]=[1]; result;",
        false,
        true,
        "number",
        &[
            (
                2454,
                "Variable 'result' is used before being assigned.",
                20,
                6,
            ),
            (
                2454,
                "Variable 'result' is used before being assigned.",
                51,
                6,
            ),
        ],
    );
}

#[test]
fn assignment_wrapper_as() {
    verify_options(
        "let result:number;[(result as number)]=[1]; result;",
        false,
        true,
        "number",
        &[
            (
                2454,
                "Variable 'result' is used before being assigned.",
                20,
                6,
            ),
            (
                2454,
                "Variable 'result' is used before being assigned.",
                44,
                6,
            ),
        ],
    );
}

#[test]
fn assignment_wrapper_nonnull() {
    verify_options(
        "let result:number;[(result!)]=[1]; result;",
        false,
        true,
        "number",
        &[],
    );
}

#[test]
fn assignment_wrapper_parens() {
    verify_options(
        "let result:number;[(result)]=[1]; result;",
        false,
        true,
        "number",
        &[],
    );
}

#[test]
fn binding_spread_any() {
    verify_options(
        "declare const values:any;const [first,result]=[...values]; result;",
        false,
        true,
        "any",
        &[],
    );
}

#[test]
fn binding_spread_array() {
    verify_options(
        "declare const values:number[];const [first,result]=[...values]; result;",
        false,
        true,
        "number",
        &[],
    );
}

#[test]
fn records_assignment_target_types() {
    let source = "declare let a:number,b:string; declare const tuple:[number,string]; [a,b]=tuple; [a]=[1]; [a,...b]=tuple; []=tuple; [...missing]=misspelled;";
    let file = tsc_rs_parser::parse("subject.ts", source);
    let mut checker = TypeChecker::new();
    checker.enable_expression_type_spans();
    let output = checker.check_with_options(
        &file,
        &tsc_rs_symbols::bind(&file),
        &CompilerOptions {
            strict: Some(true),
            target: Some(ScriptTarget::ES2015),
            ..CompilerOptions::default()
        },
    );
    for (expression, ty) in [
        ("[a,b]", "[number, string]"),
        ("[a]", "[number]"),
        ("[a,...b]", "[number, ...string[]]"),
        ("...b", "string"),
        ("[]", "[]"),
        ("[...missing]", "any[]"),
        ("...missing", "any"),
    ] {
        let start = source.find(expression).unwrap() as u32;
        let end = start + expression.len() as u32;
        assert_eq!(
            output
                .expression_type_spans
                .get(&(start, end))
                .map(|t| t.type_string.as_str()),
            Some(ty),
            "{expression}"
        );
    }
}

#[test]
fn assignment_nested_rest_default() {
    verify_options(
        "let a:string,b:number;[...[a,b=0]]=[\"\",1];const result=b; result;",
        false,
        true,
        "number",
        &[],
    );
}
