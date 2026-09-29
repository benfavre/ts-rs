// Diagnostic text, offsets and spans verified with TypeScript 6.0.3.
use tsc_rs_ast::CompilerOptions;
use tsc_rs_types::TypeChecker;
fn check(source: &str, strict: bool, expected: &[(&str, usize, &str)]) {
    let file = tsc_rs_parser::parse("instance.ts", source);
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
        .filter(|d| d.code == 2416)
        .map(|d| {
            let span = d.span.unwrap();
            (
                d.message.as_str(),
                span.start as usize,
                &source[span.start as usize..span.end as usize],
            )
        })
        .collect();
    assert_eq!(actual, expected, "{source}");
}

#[test]
fn required_parameters() {
    check(
        r####"interface I { f():void } class C implements I { f(a=0,b:number):void {} }"####,
        false,
        &[(
            r####"Property 'f' in type 'C' is not assignable to the same property in base type 'I'.
  Type '(a: number, b: number) => void' is not assignable to type '() => void'.
    Target signature provides too few arguments. Expected 2 or more, but got 0."####,
            48,
            r####"f"####,
        )],
    );
}

#[test]
fn short_callback() {
    check(
        r####"interface I { f(a:number,b:string):void } class C implements I { f():void {} }"####,
        false,
        &[],
    );
}

#[test]
fn optional_parameters() {
    check(
        r####"interface I { f():void } class C implements I { f(a?:number):void {} }"####,
        false,
        &[],
    );
}

#[test]
fn overload_missing() {
    check(
        r####"class B { f(a:string):string; f(a:number):number; f(a:any):any {} } class D extends B { f(a:string):string {return a;} }"####,
        false,
        &[(
            r####"Property 'f' in type 'D' is not assignable to the same property in base type 'B'.
  Type '(a: string) => string' is not assignable to type '{ (a: string): string; (a: number): number; }'.
    Types of parameters 'a' and 'a' are incompatible.
      Type 'number' is not assignable to type 'string'."####,
            88,
            r####"f"####,
        )],
    );
}

#[test]
fn overload_valid() {
    check(
        r####"class B { f(a:string):string; f(a:number):number; f(a:any):any {} } class D extends B { f(a:string):string; f(a:number):number; f(a:any):any {} }"####,
        false,
        &[],
    );
}

#[test]
fn overload_source() {
    check(
        r####"class B { f(a:boolean):boolean {return a;} } class D extends B { f(a:string):string; f(a:number):number; f(a:any):any {} }"####,
        false,
        &[
            (
                r####"Property 'f' in type 'D' is not assignable to the same property in base type 'B'.
  Type '{ (a: string): string; (a: number): number; }' is not assignable to type '(a: boolean) => boolean'.
    Types of parameters 'a' and 'a' are incompatible.
      Type 'boolean' is not assignable to type 'string'."####,
                65,
                r####"f"####,
            ),
            (
                r####"Property 'f' in type 'D' is not assignable to the same property in base type 'B'.
  Type '{ (a: string): string; (a: number): number; }' is not assignable to type '(a: boolean) => boolean'.
    Types of parameters 'a' and 'a' are incompatible.
      Type 'boolean' is not assignable to type 'string'."####,
                85,
                r####"f"####,
            ),
            (
                r####"Property 'f' in type 'D' is not assignable to the same property in base type 'B'.
  Type '{ (a: string): string; (a: number): number; }' is not assignable to type '(a: boolean) => boolean'.
    Types of parameters 'a' and 'a' are incompatible.
      Type 'boolean' is not assignable to type 'string'."####,
                105,
                r####"f"####,
            ),
        ],
    );
}

#[test]
fn generic_constraint() {
    check(
        r####"class A {a:number} class B {b:number} interface I { f<T extends A>():T } class C implements I { f<T extends B>():T {return null;} }"####,
        false,
        &[(
            r####"Property 'f' in type 'C' is not assignable to the same property in base type 'I'.
  Type '<T extends B>() => T' is not assignable to type '<T extends A>() => T'.
    Type 'B' is not assignable to type 'T'.
      'T' could be instantiated with an arbitrary type which could be unrelated to 'B'."####,
            96,
            r####"f"####,
        )],
    );
}

#[test]
fn generic_rename() {
    check(
        r####"interface I { f<T>(a:T):T } class C implements I { f<U>(a:U):U {return a;} }"####,
        false,
        &[],
    );
}

#[test]
fn generic_target() {
    check(
        r####"interface I<T> { f(a:T):T } class C implements I<number> { f(a:string):string {return a;} }"####,
        false,
        &[(
            r####"Property 'f' in type 'C' is not assignable to the same property in base type 'I<number>'.
  Type '(a: string) => string' is not assignable to type '(a: number) => number'.
    Types of parameters 'a' and 'a' are incompatible.
      Type 'number' is not assignable to type 'string'."####,
            59,
            r####"f"####,
        )],
    );
}

#[test]
fn generic_target_valid() {
    check(
        r####"interface I<T> { f(a:T):T } class C implements I<number> { f(a:number):number {return a;} }"####,
        false,
        &[],
    );
}

#[test]
fn generic_base() {
    check(
        r####"class B<T> {f(a:T):T {return a;}} class C extends B<number> {f(a:string):string {return a;}}"####,
        false,
        &[(
            r####"Property 'f' in type 'C' is not assignable to the same property in base type 'B<number>'.
  Type '(a: string) => string' is not assignable to type '(a: number) => number'.
    Types of parameters 'a' and 'a' are incompatible.
      Type 'number' is not assignable to type 'string'."####,
            61,
            r####"f"####,
        )],
    );
}

#[test]
fn generic_base_valid() {
    check(
        r####"class B<T> {f(a:T):T {return a;}} class C extends B<number> {f(a:number):number {return a;}}"####,
        false,
        &[],
    );
}

#[test]
fn rest_invalid() {
    check(
        r####"class B { f(...a:string[]):void {} } class C extends B { f(...a:number[]):void {} }"####,
        false,
        &[(
            r####"Property 'f' in type 'C' is not assignable to the same property in base type 'B'.
  Type '(...a: number[]) => void' is not assignable to type '(...a: string[]) => void'.
    Types of parameters 'a' and 'a' are incompatible.
      Type 'string' is not assignable to type 'number'."####,
            57,
            r####"f"####,
        )],
    );
}

#[test]
fn union_invalid() {
    check(
        r####"class B { f(a:string|boolean):void {} } class C extends B { f(a:number):void {} }"####,
        false,
        &[(
            r####"Property 'f' in type 'C' is not assignable to the same property in base type 'B'.
  Type '(a: number) => void' is not assignable to type '(a: string | boolean) => void'.
    Types of parameters 'a' and 'a' are incompatible.
      Type 'string | boolean' is not assignable to type 'number'.
        Type 'string' is not assignable to type 'number'."####,
            60,
            r####"f"####,
        )],
    );
}

#[test]
fn union_bivariant() {
    check(
        r####"class B { f(a:string|boolean):void {} } class C extends B { f(a:string):void {} }"####,
        false,
        &[],
    );
}

#[test]
fn return_invalid() {
    check(
        r####"class B { f():number {return 0;} } class C extends B { f():string {return "";} }"####,
        false,
        &[(
            r####"Property 'f' in type 'C' is not assignable to the same property in base type 'B'.
  Type '() => string' is not assignable to type '() => number'.
    Type 'string' is not assignable to type 'number'."####,
            55,
            r####"f"####,
        )],
    );
}

#[test]
fn return_void() {
    check(
        r####"class B { f():void {} } class C extends B { f():string {return "";} }"####,
        false,
        &[],
    );
}

#[test]
fn method_property() {
    check(
        r####"class B { f:(x:string)=>void } class C extends B { f(x:"a"):void {} }"####,
        true,
        &[(
            r####"Property 'f' in type 'C' is not assignable to the same property in base type 'B'.
  Type '(x: "a") => void' is not assignable to type '(x: string) => void'.
    Types of parameters 'x' and 'x' are incompatible.
      Type 'string' is not assignable to type '"a"'."####,
            51,
            r####"f"####,
        )],
    );
}

#[test]
fn property_method() {
    check(
        r####"class B { f(x:string):void {} } class C extends B { f:(x:"a")=>void }"####,
        true,
        &[],
    );
}

#[test]
fn implements_property() {
    check(
        r####"interface I { f:(x:string)=>void } class C implements I { f(x:"a"):void {} }"####,
        true,
        &[(
            r####"Property 'f' in type 'C' is not assignable to the same property in base type 'I'.
  Type '(x: "a") => void' is not assignable to type '(x: string) => void'.
    Types of parameters 'x' and 'x' are incompatible.
      Type 'string' is not assignable to type '"a"'."####,
            58,
            r####"f"####,
        )],
    );
}

#[test]
fn implements_method() {
    check(
        r####"interface I { f(x:string):void } class C implements I { f(x:"a"):void {} }"####,
        true,
        &[],
    );
}

#[test]
fn alias_intersection() {
    check(
        r####"class A {x:string} class B {y:string} type I=A&B; class C implements I {x:number;y:string}"####,
        false,
        &[(
            r####"Property 'x' in type 'C' is not assignable to the same property in base type 'A & B'.
  Type 'number' is not assignable to type 'string'."####,
            72,
            r####"x"####,
        )],
    );
}

#[test]
fn optional_mismatch() {
    check(
        r####"interface I { f?:number } class C implements I { f:string }"####,
        false,
        &[(
            r####"Property 'f' in type 'C' is not assignable to the same property in base type 'I'.
  Type 'string' is not assignable to type 'number'."####,
            49,
            r####"f"####,
        )],
    );
}

#[test]
fn optional_absent() {
    check(
        r####"interface I { f?:number } class C implements I {}"####,
        false,
        &[],
    );
}

#[test]
fn class_constraint_invalid() {
    check(
        r####"interface I { f:(x:{a:number})=>void } class C<T extends {a:string}> implements I { f(x:T):void {} }"####,
        true,
        &[(
            r####"Property 'f' in type 'C<T>' is not assignable to the same property in base type 'I'.
  Type '(x: T) => void' is not assignable to type '(x: { a: number; }) => void'.
    Types of parameters 'x' and 'x' are incompatible.
      Type '{ a: number; }' is not assignable to type 'T'.
        'T' could be instantiated with an arbitrary type which could be unrelated to '{ a: number; }'."####,
            84,
            r####"f"####,
        )],
    );
}

#[test]
fn class_constraint_valid() {
    check(
        r####"interface I { f(x:{a:number}):void } class C<T extends {a:number}> implements I { f(x:T):void {} }"####,
        true,
        &[],
    );
}

#[test]
fn class_self_generic() {
    check(
        r####"class B<T> { f(x:T):T {return x;} } class C<T> extends B<T> { f(x:T):T {return x;} }"####,
        true,
        &[],
    );
}

#[test]
fn class_shadowed_generic() {
    check(
        r####"class B<T> { f<U>(x:U):U {return x;} } class C<T> extends B<T> { f<T>(x:T):T {return x;} }"####,
        true,
        &[],
    );
}

#[test]
fn class_recursive_constraint() {
    check(
        r####"interface I<T> {value:T} class B<T extends I<T>> { f(x:T):T {return x;} } class C<T extends I<T>> extends B<T> { f(x:T):T {return x;} }"####,
        true,
        &[],
    );
}

#[test]
fn inherited_generic_interface() {
    check(
        r####"interface B<T> {f(x:T):T} interface I<T> extends B<T[]> {} class C implements I<number> {f(x:string[]):string[] {return x;}}"####,
        false,
        &[(
            r####"Property 'f' in type 'C' is not assignable to the same property in base type 'I<number>'.
  Type '(x: string[]) => string[]' is not assignable to type '(x: number[]) => number[]'.
    Types of parameters 'x' and 'x' are incompatible.
      Type 'number[]' is not assignable to type 'string[]'.
        Type 'number' is not assignable to type 'string'."####,
            89,
            r####"f"####,
        )],
    );
}

#[test]
fn implements_alias() {
    check(
        r####"interface I {f(x:number):void} type Alias=I; class C implements Alias {f(x:string):void{}}"####,
        false,
        &[(
            r####"Property 'f' in type 'C' is not assignable to the same property in base type 'I'.
  Type '(x: string) => void' is not assignable to type '(x: number) => void'.
    Types of parameters 'x' and 'x' are incompatible.
      Type 'number' is not assignable to type 'string'."####,
            71,
            r####"f"####,
        )],
    );
}

#[test]
fn overload_implements() {
    check(
        r####"interface I {f(a:boolean):boolean} class D implements I { f(a:string):string; f(a:number):number; f(a:any):any {} }"####,
        false,
        &[
            (
                r####"Property 'f' in type 'D' is not assignable to the same property in base type 'I'.
  Type '{ (a: string): string; (a: number): number; }' is not assignable to type '(a: boolean) => boolean'.
    Types of parameters 'a' and 'a' are incompatible.
      Type 'boolean' is not assignable to type 'string'."####,
                58,
                r####"f"####,
            ),
            (
                r####"Property 'f' in type 'D' is not assignable to the same property in base type 'I'.
  Type '{ (a: string): string; (a: number): number; }' is not assignable to type '(a: boolean) => boolean'.
    Types of parameters 'a' and 'a' are incompatible.
      Type 'boolean' is not assignable to type 'string'."####,
                78,
                r####"f"####,
            ),
            (
                r####"Property 'f' in type 'D' is not assignable to the same property in base type 'I'.
  Type '{ (a: string): string; (a: number): number; }' is not assignable to type '(a: boolean) => boolean'.
    Types of parameters 'a' and 'a' are incompatible.
      Type 'boolean' is not assignable to type 'string'."####,
                98,
                r####"f"####,
            ),
        ],
    );
}

#[test]
fn same_name_static() {
    check(
        r####"class B {f:number} class C extends B {static f:boolean; f:string}"####,
        false,
        &[(
            r####"Property 'f' in type 'C' is not assignable to the same property in base type 'B'.
  Type 'string' is not assignable to type 'number'."####,
            56,
            r####"f"####,
        )],
    );
}

#[test]
fn optional_strict() {
    check(
        r####"interface I {f?:number} class C implements I {f:string}"####,
        true,
        &[(
            r####"Property 'f' in type 'C' is not assignable to the same property in base type 'I'.
  Type 'string' is not assignable to type 'number'."####,
            46,
            r####"f"####,
        )],
    );
}

#[test]
fn optional_valid() {
    check(
        r####"interface I {f?:number} class C implements I {f:number}"####,
        true,
        &[],
    );
}

#[test]
fn abstract_any_return() {
    check(
        r####"abstract class B {abstract f();} class C extends B {f():number {return 0;}}"####,
        false,
        &[],
    );
}

#[test]
fn merged_conflicting_bases() {
    check(
        r####"class B<T> {a:T} interface I extends B<string> {} interface I extends B<number> {} class C implements I {a:string}"####,
        false,
        &[],
    );
}

#[test]
fn bivariant_return_failure() {
    check(
        r####"class B {f(x:string):number {return 0;}} class C extends B {f(x:"a"):string {return "";}}"####,
        true,
        &[(
            r####"Property 'f' in type 'C' is not assignable to the same property in base type 'B'.
  Type '(x: "a") => string' is not assignable to type '(x: string) => number'.
    Type 'string' is not assignable to type 'number'."####,
            60,
            r####"f"####,
        )],
    );
}

#[test]
fn implements_bivariant_return() {
    check(
        r####"interface I {f(x:string):number} class C implements I {f(x:"a"):string {return "";}}"####,
        true,
        &[(
            r####"Property 'f' in type 'C' is not assignable to the same property in base type 'I'.
  Type '(x: "a") => string' is not assignable to type '(x: string) => number'.
    Type 'string' is not assignable to type 'number'."####,
            55,
            r####"f"####,
        )],
    );
}

#[test]
fn inherited_method_variance() {
    check(
        r####"class A {f(x:string):void {}} class B extends A {} class C extends B {f(x:"a"):void {}}"####,
        true,
        &[],
    );
}

#[test]
fn inherited_property_variance() {
    check(
        r####"class A {f:(x:string)=>void} class B extends A {} class C extends B {f(x:"a"):void {}}"####,
        true,
        &[(
            r####"Property 'f' in type 'C' is not assignable to the same property in base type 'B'.
  Type '(x: "a") => void' is not assignable to type '(x: string) => void'.
    Types of parameters 'x' and 'x' are incompatible.
      Type 'string' is not assignable to type '"a"'."####,
            69,
            r####"f"####,
        )],
    );
}

#[test]
fn property_overrides_method_variance() {
    check(
        r####"class A {f(x:string):void {}} class B extends A {f:(x:string)=>void} class C extends B {f(x:"a"):void {}}"####,
        true,
        &[(
            r####"Property 'f' in type 'C' is not assignable to the same property in base type 'B'.
  Type '(x: "a") => void' is not assignable to type '(x: string) => void'.
    Types of parameters 'x' and 'x' are incompatible.
      Type 'string' is not assignable to type '"a"'."####,
            88,
            r####"f"####,
        )],
    );
}
