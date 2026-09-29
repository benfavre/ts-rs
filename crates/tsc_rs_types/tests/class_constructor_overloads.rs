// Diagnostic expectations checked against TypeScript 6.0.3.
use tsc_rs_ast::CompilerOptions;
use tsc_rs_types::TypeChecker;
fn check(source: &str, expected: &[(u32, &str, usize, &str)]) {
    let file = tsc_rs_parser::parse("constructor.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    let result = TypeChecker::new().check_with_options(
        &file,
        &symbols,
        &CompilerOptions {
            strict: Some(false),
            ..CompilerOptions::default()
        },
    );
    let actual: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| [2322, 2345, 2554, 2575, 2769].contains(&d.code))
        .map(|d| {
            let span = d.span.unwrap();
            (
                d.code,
                d.message.as_str(),
                span.start as usize,
                &source[span.start as usize..span.end as usize],
            )
        })
        .collect();
    assert_eq!(actual, expected, "{source}");
}

#[test]
fn implementation_not_assignable() {
    check(
        r####"class C {constructor(x:string);constructor(x:any){}} let c:new(x:number)=>void=C;"####,
        &[(
            2322,
            r####"Type 'typeof C' is not assignable to type 'new (x: number) => void'.
  Types of construct signatures are incompatible.
    Type 'new (x: string) => C' is not assignable to type 'new (x: number) => void'.
      Types of parameters 'x' and 'x' are incompatible.
        Type 'number' is not assignable to type 'string'."####,
            57,
            r####"c"####,
        )],
    );
}

#[test]
fn overloads_assignable() {
    check(
        r####"class C {constructor(x:string);constructor(x:number);constructor(x:any){}} let c:new(x:number)=>void=C;"####,
        &[],
    );
}

#[test]
fn overloads_assignable_first() {
    check(
        r####"class C {constructor(x:string);constructor(x:number);constructor(x:any){}} let c:new(x:string)=>void=C;"####,
        &[],
    );
}

#[test]
fn overload_assignment_mismatch() {
    check(
        r####"class C {constructor(x:string);constructor(x:number);constructor(x:any){}} let c:new(x:boolean)=>void=C;"####,
        &[(
            2322,
            r####"Type 'typeof C' is not assignable to type 'new (x: boolean) => void'.
  Types of parameters 'x' and 'x' are incompatible.
    Type 'boolean' is not assignable to type 'string'."####,
            79,
            r####"c"####,
        )],
    );
}

#[test]
fn ambient_overloads() {
    check(
        r####"declare class C {constructor(x:string);constructor(x:number);} let c:new(x:number)=>void=C;"####,
        &[],
    );
}

#[test]
fn inherited_overloads() {
    check(
        r####"class B {constructor(x:string);constructor(x:number);constructor(x:any){}} class C extends B {} let c:new(x:number)=>void=C;"####,
        &[],
    );
}

#[test]
fn inherited_mismatch() {
    check(
        r####"class B {constructor(x:string);constructor(x:number);constructor(x:any){}} class C extends B {} let c:new(x:boolean)=>void=C;"####,
        &[(
            2322,
            r####"Type 'typeof C' is not assignable to type 'new (x: boolean) => void'.
  Types of parameters 'x' and 'x' are incompatible.
    Type 'boolean' is not assignable to type 'string'."####,
            100,
            r####"c"####,
        )],
    );
}

#[test]
fn inherited_generic() {
    check(
        r####"class B<T> {constructor(x:T){}} class C extends B<string> {} let c:new(x:number)=>void=C;"####,
        &[(
            2322,
            r####"Type 'typeof C' is not assignable to type 'new (x: number) => void'.
  Types of construct signatures are incompatible.
    Type 'new (x: string) => C' is not assignable to type 'new (x: number) => void'.
      Types of parameters 'x' and 'x' are incompatible.
        Type 'number' is not assignable to type 'string'."####,
            65,
            r####"c"####,
        )],
    );
}

#[test]
fn inherited_generic_valid() {
    check(
        r####"class B<T> {constructor(x:T){}} class C extends B<string> {} let c:new(x:string)=>void=C;"####,
        &[],
    );
}

#[test]
fn own_replaces_overloads() {
    check(
        r####"class B {constructor(x:string);constructor(x:number);constructor(x:any){}} class C extends B {constructor(){super(0);}} let c:new()=>void=C;"####,
        &[],
    );
}

#[test]
fn single_new_mismatch() {
    check(
        r####"class C {constructor(x:string);constructor(x:any){}} new C(1);"####,
        &[(
            2345,
            r####"Argument of type 'number' is not assignable to parameter of type 'string'."####,
            59,
            r####"1"####,
        )],
    );
}

#[test]
fn overloaded_new() {
    check(
        r####"class C {constructor(x:string);constructor(x:number);constructor(x:any){}} new C(true);"####,
        &[(
            2769,
            r####"No overload matches this call.
  Overload 1 of 2, '(x: string): C', gave the following error.
    Argument of type 'boolean' is not assignable to parameter of type 'string'.
  Overload 2 of 2, '(x: number): C', gave the following error.
    Argument of type 'boolean' is not assignable to parameter of type 'number'."####,
            81,
            r####"true"####,
        )],
    );
}

#[test]
fn overloaded_new_valid() {
    check(
        r####"class C {constructor(x:string);constructor(x:number);constructor(x:any){}} new C(1);new C("x");"####,
        &[],
    );
}

#[test]
fn alias_new() {
    check(
        r####"class C {constructor(x:string);constructor(x:number);constructor(x:any){}} const Alias=C; new Alias(true);"####,
        &[(
            2769,
            r####"No overload matches this call.
  Overload 1 of 2, '(x: string): C', gave the following error.
    Argument of type 'boolean' is not assignable to parameter of type 'string'.
  Overload 2 of 2, '(x: number): C', gave the following error.
    Argument of type 'boolean' is not assignable to parameter of type 'number'."####,
            100,
            r####"true"####,
        )],
    );
}

#[test]
fn arity_gap() {
    check(
        r####"class C {constructor(x:string);constructor(x:string,y:number,z:number);constructor(...x:any[]){}} new C("x",1);"####,
        &[(
            2575,
            r####"No overload expects 2 arguments, but overloads do exist that expect either 1 or 3 arguments."####,
            98,
            r####"new C("x",1)"####,
        )],
    );
}

#[test]
fn arity_empty() {
    check(
        r####"class C {constructor(x:string);constructor(x:string,y:number);constructor(...x:any[]){}} new C();"####,
        &[(
            2554,
            r####"Expected 1-2 arguments, but got 0."####,
            89,
            r####"new C()"####,
        )],
    );
}

#[test]
fn arity_candidates() {
    check(
        r####"class C {constructor(x:string);constructor(x:number,y:number);constructor(...x:any[]){}} new C(true,1);"####,
        &[(
            2345,
            r####"Argument of type 'boolean' is not assignable to parameter of type 'number'."####,
            95,
            r####"true"####,
        )],
    );
}

#[test]
fn ambient_new() {
    check(
        r####"declare class C {constructor(x:string);constructor(x:number);} new C(1);"####,
        &[],
    );
}

#[test]
fn inherited_new() {
    check(
        r####"class B {constructor(x:string);constructor(x:number);constructor(x:any){}} class C extends B {} new C(true);"####,
        &[(
            2769,
            r####"No overload matches this call.
  Overload 1 of 2, '(x: string): C', gave the following error.
    Argument of type 'boolean' is not assignable to parameter of type 'string'.
  Overload 2 of 2, '(x: number): C', gave the following error.
    Argument of type 'boolean' is not assignable to parameter of type 'number'."####,
            102,
            r####"true"####,
        )],
    );
}

#[test]
fn inherited_generic_new() {
    check(
        r####"class B<T> {constructor(x:T){}} class C extends B<string> {} new C(1);"####,
        &[(
            2345,
            r####"Argument of type 'number' is not assignable to parameter of type 'string'."####,
            67,
            r####"1"####,
        )],
    );
}

#[test]
fn generic_overload() {
    check(
        r####"class C<T> {value:T;constructor(x:T);constructor(x:T[]);constructor(x:any){}} let c:C<number>=new C(["x"]);"####,
        &[(
            2322,
            r####"Type 'C<string[]>' is not assignable to type 'C<number>'.
  Type 'string[]' is not assignable to type 'number'."####,
            82,
            r####"c"####,
        )],
    );
}

#[test]
fn generic_overload_valid() {
    check(
        r####"class C<T> {value:T;constructor(x:T);constructor(x:T[]);constructor(x:any){}} let c:C<number>=new C([1]);"####,
        &[(
            2322,
            r####"Type 'C<number[]>' is not assignable to type 'C<number>'.
  Type 'number[]' is not assignable to type 'number'."####,
            82,
            r####"c"####,
        )],
    );
}

#[test]
fn defaults_required() {
    check(
        r####"class C {constructor(x=0,y:number){}} let c:new()=>void=C;"####,
        &[(
            2322,
            r####"Type 'typeof C' is not assignable to type 'new () => void'.
  Types of construct signatures are incompatible.
    Type 'new (x: number, y: number) => C' is not assignable to type 'new () => void'.
      Target signature provides too few arguments. Expected 2 or more, but got 0."####,
            42,
            r####"c"####,
        )],
    );
}

#[test]
fn defaults_optional() {
    check(
        r####"class C {constructor(x=0){}} let c:new()=>void=C;"####,
        &[],
    );
}

#[test]
fn rest_overloads() {
    check(
        r####"class C {constructor(...x:string[]);constructor(...x:number[]);constructor(...x:any[]){}} new C(true);"####,
        &[(
            2769,
            r####"No overload matches this call.
  Overload 1 of 2, '(...x: string[]): C', gave the following error.
    Argument of type 'boolean' is not assignable to parameter of type 'string'.
  Overload 2 of 2, '(...x: number[]): C', gave the following error.
    Argument of type 'boolean' is not assignable to parameter of type 'number'."####,
            96,
            r####"true"####,
        )],
    );
}

#[test]
fn target_all_overloads() {
    check(
        r####"class C {constructor(x:string){}} let c:{new(x:string):C;new(x:number):C}=C;"####,
        &[(
            2322,
            r####"Type 'typeof C' is not assignable to type '{ new (x: string): C; new (x: number): C; }'.
  Types of parameters 'x' and 'x' are incompatible.
    Type 'number' is not assignable to type 'string'."####,
            38,
            r####"c"####,
        )],
    );
}

#[test]
fn super_overload_invalid() {
    check(
        r####"class B {constructor(x:string);constructor(x:number);constructor(x:any){}} class C extends B {constructor(){super(true);}}"####,
        &[(
            2769,
            r####"No overload matches this call.
  Overload 1 of 2, '(x: string): B', gave the following error.
    Argument of type 'boolean' is not assignable to parameter of type 'string'.
  Overload 2 of 2, '(x: number): B', gave the following error.
    Argument of type 'boolean' is not assignable to parameter of type 'number'."####,
            114,
            r####"true"####,
        )],
    );
}

#[test]
fn super_overload_valid() {
    check(
        r####"class B {constructor(x:string);constructor(x:number);constructor(x:any){}} class C extends B {constructor(){super(1);}}"####,
        &[],
    );
}

#[test]
fn super_generic_invalid() {
    check(
        r####"class B<T> {constructor(x:T){}} class C extends B<string> {constructor(){super(1);}}"####,
        &[(
            2345,
            r####"Argument of type 'number' is not assignable to parameter of type 'string'."####,
            79,
            r####"1"####,
        )],
    );
}

#[test]
fn super_generic_valid() {
    check(
        r####"class B<T> {constructor(x:T){}} class C extends B<string> {constructor(){super("x");}}"####,
        &[],
    );
}

#[test]
fn inherited_grandparent() {
    check(
        r####"class A<T> {constructor(x:T);constructor(x:T[]);constructor(x:any){}} class B<U> extends A<U[]> {} class C extends B<string> {} new C(1);"####,
        &[(
            2769,
            r####"No overload matches this call.
  Overload 1 of 2, '(x: string[]): C', gave the following error.
    Argument of type 'number' is not assignable to parameter of type 'string[]'.
  Overload 2 of 2, '(x: string[][]): C', gave the following error.
    Argument of type 'number' is not assignable to parameter of type 'string[][]'."####,
            134,
            r####"1"####,
        )],
    );
}

#[test]
fn rest_arguments_valid() {
    check(
        r####"class C {constructor(...x:string[]);constructor(...x:number[]);constructor(...x:any[]){}} new C(1,2,3);"####,
        &[],
    );
}

#[test]
fn literal_constructor() {
    check(
        r####"class C {constructor(x:"yes");constructor(x:any){}} new C("no");"####,
        &[(
            2345,
            r####"Argument of type '"no"' is not assignable to parameter of type '"yes"'."####,
            58,
            r####""no""####,
        )],
    );
}

#[test]
fn bodyless_single() {
    check(
        r####"declare class C {constructor(x:string);} new C(1);"####,
        &[(
            2345,
            r####"Argument of type 'number' is not assignable to parameter of type 'string'."####,
            47,
            r####"1"####,
        )],
    );
}

#[test]
fn return_derived() {
    check(
        r####"class B {constructor(x:string);constructor(x:number);constructor(x:any){}} class C extends B {value:number} let c:C=new C(1);"####,
        &[],
    );
}

#[test]
fn inherited_default() {
    check(
        r####"class B<T=string> {constructor(x:T){}} class C extends B {} new C(1);"####,
        &[(
            2345,
            r####"Argument of type 'number' is not assignable to parameter of type 'string'."####,
            66,
            r####"1"####,
        )],
    );
}

#[test]
fn inherited_default_valid() {
    check(
        r####"class B<T=string> {constructor(x:T){}} class C extends B {} new C("x");"####,
        &[],
    );
}

#[test]
fn inherited_default_dependency() {
    check(
        r####"class B<T=string,U=T[]> {constructor(x:U){}} class C extends B {} new C([1]);"####,
        &[(
            2322,
            r####"Type 'number' is not assignable to type 'string'."####,
            73,
            r####"1"####,
        )],
    );
}

#[test]
fn bodyless_callback_context() {
    check(
        r####"declare class C {constructor(callback:(x:number)=>void);} new C(x=>{let s:string=x;});"####,
        &[(
            2322,
            r####"Type 'number' is not assignable to type 'string'."####,
            72,
            r####"s"####,
        )],
    );
}

#[test]
fn explicit_generic_callback_context() {
    check(
        r####"class C<T> {constructor(callback:(x:T)=>void){}} new C<number>(x=>{let s:string=x;});"####,
        &[(
            2322,
            r####"Type 'number' is not assignable to type 'string'."####,
            71,
            r####"s"####,
        )],
    );
}
