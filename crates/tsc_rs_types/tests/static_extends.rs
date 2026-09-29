// Expected messages and name spans checked against TypeScript 6.0.3.
use tsc_rs_ast::CompilerOptions;
use tsc_rs_types::TypeChecker;

fn check(source: &str, strict: bool, expected: &[(&str, &str)]) {
    let file = tsc_rs_parser::parse("static.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    let result = TypeChecker::new().check_with_options(
        &file,
        &symbols,
        &CompilerOptions {
            strict: Some(strict),
            ..CompilerOptions::default()
        },
    );
    let actual: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == 2417)
        .map(|d| {
            let span = d.span.unwrap();
            (
                d.message.as_str(),
                &source[span.start as usize..span.end as usize],
            )
        })
        .collect();
    assert_eq!(actual, expected, "{source}");
}

#[test]
fn constrained_generic() {
    check(
        r###"class A { static f<T extends string>(x:T):T {return x;} } class B extends A { static f<T extends number>(x:T):T {return x;} }"###,
        true,
        &[(
            r###"Class static side 'typeof B' incorrectly extends base class static side 'typeof A'.
  Types of property 'f' are incompatible.
    Type '<T extends number>(x: T) => T' is not assignable to type '<T extends string>(x: T) => T'.
      Types of parameters 'x' and 'x' are incompatible.
        Type 'T' is not assignable to type 'number'.
          Type 'string' is not assignable to type 'number'."###,
            r###"B"###,
        )],
    );
}

#[test]
fn valid_generic_constraint() {
    check(
        r###"class A { static f<T extends string>(x:T):T {return x;} } class B extends A { static f<T>(x:T):T {return x;} }"###,
        true,
        &[],
    );
}

#[test]
fn method_return_with_parameters() {
    check(
        r###"class A { static f(x:string):number {return 1;} } class B extends A { static f(x:"a"):string {return "";} }"###,
        true,
        &[(
            r###"Class static side 'typeof B' incorrectly extends base class static side 'typeof A'.
  The types returned by 'f(...)' are incompatible between these types.
    Type 'string' is not assignable to type 'number'."###,
            r###"B"###,
        )],
    );
}

#[test]
fn method_over_function_property() {
    check(
        r###"class A { static f: (x: string) => void; } class B extends A { static f(x: "a"): void {} }"###,
        true,
        &[(
            r###"Class static side 'typeof B' incorrectly extends base class static side 'typeof A'.
  Types of property 'f' are incompatible.
    Type '(x: "a") => void' is not assignable to type '(x: string) => void'.
      Types of parameters 'x' and 'x' are incompatible.
        Type 'string' is not assignable to type '"a"'."###,
            r###"B"###,
        )],
    );
}

#[test]
fn function_property_over_method() {
    check(
        r###"class A { static f(x:string):void {} } class B extends A { static f: (x: "a") => void; }"###,
        true,
        &[],
    );
}

#[test]
fn property() {
    check(
        r###"class A { static x: number; } class B extends A { static x: string; }"###,
        true,
        &[(
            r###"Class static side 'typeof B' incorrectly extends base class static side 'typeof A'.
  Types of property 'x' are incompatible.
    Type 'string' is not assignable to type 'number'."###,
            r###"B"###,
        )],
    );
}

#[test]
fn return_type() {
    check(
        r###"class A { static f(): number { return 1; } } class B extends A { static f(): string { return "a"; } }"###,
        true,
        &[(
            r###"Class static side 'typeof B' incorrectly extends base class static side 'typeof A'.
  The types returned by 'f()' are incompatible between these types.
    Type 'string' is not assignable to type 'number'."###,
            r###"B"###,
        )],
    );
}

#[test]
fn parameter() {
    check(
        r###"class A { static f(x: number): void {} } class B extends A { static f(x: string): void {} }"###,
        true,
        &[(
            r###"Class static side 'typeof B' incorrectly extends base class static side 'typeof A'.
  Types of property 'f' are incompatible.
    Type '(x: string) => void' is not assignable to type '(x: number) => void'.
      Types of parameters 'x' and 'x' are incompatible.
        Type 'number' is not assignable to type 'string'."###,
            r###"B"###,
        )],
    );
}

#[test]
fn arity() {
    check(
        r###"class A { static f(): number { return 1; } } class B extends A { static f(x: number): number { return 1; } }"###,
        true,
        &[(
            r###"Class static side 'typeof B' incorrectly extends base class static side 'typeof A'.
  Types of property 'f' are incompatible.
    Type '(x: number) => number' is not assignable to type '() => number'.
      Target signature provides too few arguments. Expected 1 or more, but got 0."###,
            r###"B"###,
        )],
    );
}

#[test]
fn generic() {
    check(
        r###"class A { static f<T>(x: T): T { return x; } } class B extends A { static f(x: string): string { return x; } }"###,
        true,
        &[(
            r###"Class static side 'typeof B' incorrectly extends base class static side 'typeof A'.
  Types of property 'f' are incompatible.
    Type '(x: string) => string' is not assignable to type '<T>(x: T) => T'.
      Types of parameters 'x' and 'x' are incompatible.
        Type 'T' is not assignable to type 'string'."###,
            r###"B"###,
        )],
    );
}

#[test]
fn nested_property() {
    check(
        r###"class A { static x: { a: number }; } class B extends A { static x: { a: string }; }"###,
        true,
        &[(
            r###"Class static side 'typeof B' incorrectly extends base class static side 'typeof A'.
  The types of 'x.a' are incompatible between these types.
    Type 'string' is not assignable to type 'number'."###,
            r###"B"###,
        )],
    );
}

#[test]
fn accessor_method() {
    check(
        r###"class A { static f(): string { return ""; } } class B extends A { static get f(): string { return ""; } }"###,
        true,
        &[(
            r###"Class static side 'typeof B' incorrectly extends base class static side 'typeof A'.
  Types of property 'f' are incompatible.
    Type 'string' is not assignable to type '() => string'."###,
            r###"B"###,
        )],
    );
}

#[test]
fn setter_only() {
    check(
        r###"class A { static set x(value: number) {} } class B extends A { static set x(value: string) {} }"###,
        true,
        &[(
            r###"Class static side 'typeof B' incorrectly extends base class static side 'typeof A'.
  Types of property 'x' are incompatible.
    Type 'string' is not assignable to type 'number'."###,
            r###"B"###,
        )],
    );
}

#[test]
fn getter_after_setter() {
    check(
        r###"class A { static set x(value: string | number) {} static get x(): number { return 1; } } class B extends A { static x: string; }"###,
        true,
        &[(
            r###"Class static side 'typeof B' incorrectly extends base class static side 'typeof A'.
  Types of property 'x' are incompatible.
    Type 'string' is not assignable to type 'number'."###,
            r###"B"###,
        )],
    );
}

#[test]
fn separate_private() {
    check(
        r###"class A { private static x: number; } class B extends A { private static x: number; }"###,
        true,
        &[(
            r###"Class static side 'typeof B' incorrectly extends base class static side 'typeof A'.
  Types have separate declarations of a private property 'x'."###,
            r###"B"###,
        )],
    );
}

#[test]
fn private_base() {
    check(
        r###"class A { private static x: number; } class B extends A { static x: number; }"###,
        true,
        &[(
            r###"Class static side 'typeof B' incorrectly extends base class static side 'typeof A'.
  Property 'x' is private in type 'typeof A' but not in type 'typeof B'."###,
            r###"B"###,
        )],
    );
}

#[test]
fn private_derived() {
    check(
        r###"class A { static x: number; } class B extends A { private static x: number; }"###,
        true,
        &[(
            r###"Class static side 'typeof B' incorrectly extends base class static side 'typeof A'.
  Property 'x' is private in type 'typeof B' but not in type 'typeof A'."###,
            r###"B"###,
        )],
    );
}

#[test]
fn protected_derived() {
    check(
        r###"class A { static x: number; } class B extends A { protected static x: number; }"###,
        true,
        &[(
            r###"Class static side 'typeof B' incorrectly extends base class static side 'typeof A'.
  Property 'x' is protected in type 'typeof B' but public in type 'typeof A'."###,
            r###"B"###,
        )],
    );
}

#[test]
fn private_accessor() {
    check(
        r###"class A { private static get x(): number { return 1; } } class B extends A { static x: number; }"###,
        true,
        &[(
            r###"Class static side 'typeof B' incorrectly extends base class static side 'typeof A'.
  Property 'x' is private in type 'typeof A' but not in type 'typeof B'."###,
            r###"B"###,
        )],
    );
}

#[test]
fn optional_strict() {
    check(
        r###"class A { static x: number; } class B extends A { static x?: number; }"###,
        true,
        &[(
            r###"Class static side 'typeof B' incorrectly extends base class static side 'typeof A'.
  Types of property 'x' are incompatible.
    Type 'number | undefined' is not assignable to type 'number'.
      Type 'undefined' is not assignable to type 'number'."###,
            r###"B"###,
        )],
    );
}

#[test]
fn optional_loose() {
    check(
        r###"class A { static x: number; } class B extends A { static x?: number; }"###,
        false,
        &[(
            r###"Class static side 'typeof B' incorrectly extends base class static side 'typeof A'.
  Property 'x' is optional in type 'typeof B' but required in type 'typeof A'."###,
            r###"B"###,
        )],
    );
}

#[test]
fn inherited() {
    check(
        r###"class A { static x: number; } class B extends A {} class C extends B { static x: string; }"###,
        true,
        &[(
            r###"Class static side 'typeof C' incorrectly extends base class static side 'typeof B'.
  Types of property 'x' are incompatible.
    Type 'string' is not assignable to type 'number'."###,
            r###"C"###,
        )],
    );
}

#[test]
fn qualified() {
    check(
        r###"namespace N { export class A { static x: number; } } class B extends N.A { static x: string; }"###,
        true,
        &[(
            r###"Class static side 'typeof B' incorrectly extends base class static side 'typeof A'.
  Types of property 'x' are incompatible.
    Type 'string' is not assignable to type 'number'."###,
            r###"B"###,
        )],
    );
}

#[test]
fn alias() {
    check(
        r###"class A { static x: number; } const Alias = A; class B extends Alias { static x: string; }"###,
        true,
        &[(
            r###"Class static side 'typeof B' incorrectly extends base class static side 'typeof A'.
  Types of property 'x' are incompatible.
    Type 'string' is not assignable to type 'number'."###,
            r###"B"###,
        )],
    );
}

#[test]
fn class_expression() {
    check(
        r###"class A { static x: number; } const B = class extends A { static x: string; };"###,
        true,
        &[(
            r###"Class static side 'typeof B' incorrectly extends base class static side 'typeof A'.
  Types of property 'x' are incompatible.
    Type 'string' is not assignable to type 'number'."###,
            r###"class"###,
        )],
    );
}

#[test]
fn named_expression() {
    check(
        r###"class A { static x: number; } const B = class Named extends A { static x: string; };"###,
        true,
        &[(
            r###"Class static side 'typeof Named' incorrectly extends base class static side 'typeof A'.
  Types of property 'x' are incompatible.
    Type 'string' is not assignable to type 'number'."###,
            r###"Named"###,
        )],
    );
}

#[test]
fn anonymous_expression() {
    check(
        r###"class A { static x: number; } (class extends A { static x: string; });"###,
        true,
        &[(
            r###"Class static side 'typeof (Anonymous class)' incorrectly extends base class static side 'typeof A'.
  Types of property 'x' are incompatible.
    Type 'string' is not assignable to type 'number'."###,
            r###"class"###,
        )],
    );
}

#[test]
fn first_member() {
    check(
        r###"class A { static f(): number { return 1; } static x: number; } class B extends A { static x: string; static f(): string { return ""; } }"###,
        true,
        &[(
            r###"Class static side 'typeof B' incorrectly extends base class static side 'typeof A'.
  The types returned by 'f()' are incompatible between these types.
    Type 'string' is not assignable to type 'number'."###,
            r###"B"###,
        )],
    );
}

#[test]
fn overload_mismatch() {
    check(
        r###"declare class A { static f(x: number): number; static f(x: string): string; } class B extends A { static f(x: number): number; static f(x: any): any { return x; } }"###,
        true,
        &[(
            r###"Class static side 'typeof B' incorrectly extends base class static side 'typeof A'.
  Types of property 'f' are incompatible.
    Type '(x: number) => number' is not assignable to type '{ (x: number): number; (x: string): string; }'.
      Types of parameters 'x' and 'x' are incompatible.
        Type 'string' is not assignable to type 'number'."###,
            r###"B"###,
        )],
    );
}

#[test]
fn valid_overload() {
    check(
        r###"declare class A { static f(x: number): number; static f(x: string): string; } class B extends A { static f(x: number): number; static f(x: string): string; static f(x: any): any { return x; } }"###,
        true,
        &[],
    );
}

#[test]
fn valid_method_variance() {
    check(
        r###"class A { static f(x: string): void {} } class B extends A { static f(x: "a"): void {} }"###,
        true,
        &[],
    );
}

#[test]
fn invalid_function_variance() {
    check(
        r###"class A { static f: (x: string) => void; } class B extends A { static f: (x: "a") => void; }"###,
        true,
        &[(
            r###"Class static side 'typeof B' incorrectly extends base class static side 'typeof A'.
  Types of property 'f' are incompatible.
    Type '(x: "a") => void' is not assignable to type '(x: string) => void'.
      Types of parameters 'x' and 'x' are incompatible.
        Type 'string' is not assignable to type '"a"'."###,
            r###"B"###,
        )],
    );
}

#[test]
fn valid_protected() {
    check(
        r###"class A { protected static x: number; } class B extends A { static x: number; }"###,
        true,
        &[],
    );
}

#[test]
fn valid_private_identifiers() {
    check(
        r###"class A { static #x: number; } class B extends A { static #x: string; }"###,
        true,
        &[],
    );
}

#[test]
fn valid_constructor_difference() {
    check(
        r###"class A { constructor(x: number) {} x: number; } class B extends A { constructor() { super(1); } x: string; }"###,
        true,
        &[],
    );
}

#[test]
fn valid_inheritance() {
    check(
        r###"class A { private static x: number; protected static y: number; static f(x: number): number { return x; } } class B extends A {}"###,
        true,
        &[],
    );
}

#[test]
fn arity_precedes_return_mismatch() {
    check(
        r#"class A { static f():number {return 1;} } class B extends A { static f(x:string):string {return "";} }"#,
        true,
        &[(
            r#"Class static side 'typeof B' incorrectly extends base class static side 'typeof A'.
  Types of property 'f' are incompatible.
    Type '(x: string) => string' is not assignable to type '() => number'.
      Target signature provides too few arguments. Expected 1 or more, but got 0."#,
            "B",
        )],
    );
}

#[test]
fn property_parameter_mismatch_precedes_return() {
    check(
        r#"class A { static f: (x:string)=>number; } class B extends A { static f: (x:"a")=>string; }"#,
        true,
        &[(
            r#"Class static side 'typeof B' incorrectly extends base class static side 'typeof A'.
  Types of property 'f' are incompatible.
    Type '(x: "a") => string' is not assignable to type '(x: string) => number'.
      Types of parameters 'x' and 'x' are incompatible.
        Type 'string' is not assignable to type '"a"'."#,
            "B",
        )],
    );
}

#[test]
fn valid_overloaded_method_variance() {
    check(
        r#"declare class A { static f(x:string):void; static f(x:number):void; } declare class B extends A { static f(x:"a"):void; static f(x:1):void; }"#,
        true,
        &[],
    );
}
