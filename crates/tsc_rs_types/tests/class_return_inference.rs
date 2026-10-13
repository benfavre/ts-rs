use tsc_rs_ast::CompilerOptions;
use tsc_rs_types::TypeChecker;

// Original TypeScript 6.0.3 TypeWriterWalker text and synthetic-node masks.
fn assert_methods(source: &str, strict: bool, expected: &[(&str, usize, &str, &str)]) {
    let file = tsc_rs_parser::parse("case.ts", source);
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    let symbols = tsc_rs_symbols::bind(&file);
    let options = CompilerOptions {
        strict: Some(strict),
        ..Default::default()
    };
    let mut checker = TypeChecker::new();
    checker.enable_expression_type_spans();
    let output = checker.check_with_options(&file, &symbols, &options);
    for &(anchor, offset, text, underline) in expected {
        let span = (offset as u32, (offset + anchor.len()) as u32);
        let actual = output
            .expression_type_spans
            .get(&span)
            .unwrap_or_else(|| panic!("missing {anchor} at {offset}: {source}"));
        assert_eq!(actual.node_text, text, "{anchor} at {offset}: {source}");
        assert_eq!(
            actual.underline, underline,
            "{anchor} at {offset}: {source}"
        );
        assert_eq!(
            actual.node_text.encode_utf16().count(),
            actual.underline.len()
        );
    }
    let plain = TypeChecker::new().check_with_options(&file, &symbols, &options);
    assert_eq!(output.expression_types, plain.expression_types);
    assert_eq!(
        format!("{:?}", output.diagnostics),
        format!("{:?}", plain.diagnostics)
    );
}

#[test]
fn instance_field() {
    assert_methods(
        "class C { value = 1; read() { return this.value; } }",
        true,
        &[("read", 21, "() => number", "^^^^^^^^^^^^")],
    );
}

#[test]
fn later_field() {
    assert_methods(
        "class C { read() { return this.value; } value = \"text\"; }",
        true,
        &[("read", 10, "() => string", "^^^^^^^^^^^^")],
    );
}

#[test]
fn array_predicate() {
    assert_methods(
        "class C { values: number[] = []; all() { return this.values.every(x => x > 0); } }",
        true,
        &[("all", 33, "() => boolean", "^^^^^^^^^^^^^")],
    );
}

#[test]
fn later_method() {
    assert_methods(
        "class C { first() { return this.second(); } second() { return this.value; } value = 1; }",
        true,
        &[
            ("first", 10, "() => number", "^^^^^^^^^^^^"),
            ("second", 44, "() => number", "^^^^^^^^^^^^"),
        ],
    );
}

#[test]
fn receiver_alias() {
    assert_methods(
        "class C { value = 1; read() { const self = this; return self.value; } }",
        true,
        &[("read", 21, "() => number", "^^^^^^^^^^^^")],
    );
}

#[test]
fn static_field() {
    assert_methods(
        "class C { static value = 1; static read() { return this.value; } }",
        true,
        &[("read", 35, "() => number", "^^^^^^^^^^^^")],
    );
}

#[test]
fn polymorphic_and_static_this() {
    assert_methods(
        "class C { self() { return this; } static self() { return this; } }",
        true,
        &[
            ("self", 10, "() => this", "^^^^^^^^^^"),
            ("self", 41, "() => typeof C", "^^^^^^^^^^^^^^"),
        ],
    );
}

#[test]
fn generic_base() {
    assert_methods("class Base<T> { value!: T; } class Child extends Base<string> { read() { return this.value; } }", true, &[
        ("read", 64, "() => string", "^^^^^^^^^^^^"),
    ]);
}

#[test]
fn static_base() {
    assert_methods("class Base { static value = 1; } class Child extends Base { static read() { return this.value; } }", true, &[
        ("read", 67, "() => number", "^^^^^^^^^^^^"),
    ]);
}

#[test]
fn generic_method() {
    assert_methods("class C<T> { value!: T; read() { return this.value; } generic<U>(value: U) { return value; } }", true, &[
        ("read", 24, "() => T", "^^^^^^^"),
        ("generic", 54, "<U>(value: U) => U", "^ ^^     ^^ ^^^^^^"),
    ]);
}

#[test]
fn lexical_arrow() {
    assert_methods(
        "class C { value = 1; read() { return () => this.value; } }",
        true,
        &[("read", 21, "() => () => number", "^^^^^^^^^^^^^^^^^^")],
    );
}

#[test]
fn ordinary_function() {
    assert_methods(
        "class C { value = 1; read() { return function() { return this.value; }; } }",
        false,
        &[("read", 21, "() => () => any", "^^^^^^^^^^^^^^^")],
    );
}

#[test]
fn explicit_function_this() {
    assert_methods("class C { value = 1; read() { return function(this:{value:string}) { return this.value; }; } }", true, &[
        ("read", 21, "() => (this: { value: string; }) => string", "^^^^^^^                        ^^^^^^^^^^^"),
    ]);
}

#[test]
fn getter() {
    assert_methods(
        "class C { value = 1; get size() { return this.value; } read() { return this.size; } }",
        true,
        &[
            ("size", 25, "number", "^^^^^^"),
            ("read", 55, "() => number", "^^^^^^^^^^^^"),
        ],
    );
}

#[test]
fn async_method() {
    assert_methods(
        "class C { async read() { return this.value; } value = 1; }",
        true,
        &[("read", 16, "() => Promise<number>", "^^^^^^^^^^^^^^^^^^^^^")],
    );
}

#[test]
fn optional_property() {
    assert_methods(
        "class C { value?: number; read() { return this.value; } }",
        true,
        &[(
            "read",
            26,
            "() => number | undefined",
            "^^^^^^^^^^^^^^^^^^^^^^^^",
        )],
    );
}

#[test]
fn direct_cycle() {
    assert_methods(
        "class C { read() { return this.read(); } }",
        true,
        &[("read", 10, "() => any", "^^^^^^^^^")],
    );
}

#[test]
fn mutual_cycle() {
    assert_methods(
        "class C { first() { return this.second(); } second() { return this.first(); } }",
        true,
        &[
            ("first", 10, "() => any", "^^^^^^^^^"),
            ("second", 44, "() => any", "^^^^^^^^^"),
        ],
    );
}

#[test]
fn conditional_cycle() {
    assert_methods(
        "class C { f(n:number) { return n ? 0 : this.f(n-1); } }",
        true,
        &[("f", 10, "(n: number) => any", "^ ^^      ^^^^^^^^")],
    );
}

#[test]
fn operator_cycle() {
    assert_methods(
        "class C { f() { return this.f() === 2; } }",
        true,
        &[("f", 10, "() => any", "^^^^^^^^^")],
    );
}

#[test]
fn parameter_property() {
    assert_methods(
        "class C { constructor(public value: number) {} read() { return this.value; } }",
        true,
        &[("read", 47, "() => number", "^^^^^^^^^^^^")],
    );
}

#[test]
fn literal_index() {
    assert_methods(
        "class C { value = 1; read() { return this[\"value\"]; } }",
        true,
        &[("read", 21, "() => number", "^^^^^^^^^^^^")],
    );
}

#[test]
fn captured_alias() {
    assert_methods("class C { value = 1; read() { const self = this; return function() { return self.value; }; } }", true, &[
        ("read", 21, "() => () => number", "^^^^^^^^^^^^^^^^^^"),
    ]);
}

#[test]
fn static_later_method() {
    assert_methods("class C { static first() { return this.second(); } static second() { return this.value; } static value = 1; }", true, &[
        ("first", 17, "() => number", "^^^^^^^^^^^^"),
        ("second", 58, "() => number", "^^^^^^^^^^^^"),
    ]);
}

#[test]
fn overload_call() {
    assert_methods("class C { value = 1; read() { return this.pick(1); } pick(x: number): string; pick(x: string): number; pick(x: any) { return x; } }", true, &[
        ("read", 21, "() => string", "^^^^^^^^^^^^"),
    ]);
}

#[test]
fn getter_chain() {
    assert_methods("class C { read() { return this.size; } get size() { return this.other; } get other() { return this.value; } value = 1; }", true, &[
        ("read", 10, "() => number", "^^^^^^^^^^^^"),
        ("size", 43, "number", "^^^^^^"),
        ("other", 77, "number", "^^^^^^"),
    ]);
}

#[test]
fn scoped_receivers() {
    assert_methods("class C { value = 1; read() { return this.value; } } class D { value = \"text\"; read() { return this.value; } }", true, &[
        ("read", 21, "() => number", "^^^^^^^^^^^^"),
        ("read", 79, "() => string", "^^^^^^^^^^^^"),
    ]);
}

#[test]
fn recursive_shape() {
    assert_methods(
        "class C { read() { return { value: this.read() }; } }",
        true,
        &[("read", 10, "() => any", "^^^^^^^^^")],
    );
}

#[test]
fn getter_with_setter_annotation() {
    assert_methods("class C { set size(value: number) {} get size() { return this.value; } value = 1; read() { return this.size; } }", true, &[
        ("size", 14, "number", "^^^^^^"),
        ("size", 41, "number", "^^^^^^"),
        ("read", 82, "() => number", "^^^^^^^^^^^^"),
    ]);
}

#[test]
fn explicit_method_receiver() {
    assert_methods(
        "class C { f(this: C) { return this; } }",
        true,
        &[("f", 10, "(this: C) => C", "^    ^^ ^^^^^^")],
    );
}

#[test]
fn declared_function_receiver_provenance() {
    assert_methods(
        "function f(this:void):void {}",
        true,
        &[("f", 9, "(this: void) => void", "^    ^^    ^^^^^    ")],
    );
}

#[test]
fn function_expression_receiver_provenance() {
    assert_methods(
        "const f = function(this:void):void {};",
        true,
        &[("f", 6, "(this: void) => void", "^    ^^    ^^^^^    ")],
    );
}

#[test]
fn sibling_parameter_scopes() {
    assert_methods("declare let value: string; class C { first(value: number) { return this.second(); } second() { return value; } }", true, &[("first", 37, "(value: number) => string", "^     ^^      ^^^^^^^^^^^"), ("second", 84, "() => string", "^^^^^^^^^^^^")]);
}

#[test]
fn sibling_type_parameter_scopes() {
    assert_methods("type T = string; class C { first<T>(value: T) { return this.second(); } second() { return null as T; } }", true, &[("first", 27, "<T>(value: T) => string", "^ ^^     ^^ ^^^^^^^^^^^"), ("second", 72, "() => T", "^^^^^^ ")]);
}

#[test]
fn namespace_receivers_do_not_share_class_metadata() {
    let source = "namespace A { class C { value = 1; self() { return this; } } const c = new C(); c.self().value = 2; } namespace B { class C<T> { constructor(public value:T) {} self() { return this; } } const c = new C(\"\"); c.self().value = \"ok\"; }";
    let file = tsc_rs_parser::parse("case.ts", source);
    assert!(file.diagnostics.is_empty());
    let symbols = tsc_rs_symbols::bind(&file);
    let mut checker = TypeChecker::new();
    checker.enable_expression_type_spans();
    let result = checker.check_with_options(
        &file,
        &symbols,
        &CompilerOptions {
            strict: Some(true),
            ..Default::default()
        },
    );
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    for ((offset, text), expected) in source
        .match_indices("c.self().value")
        .zip(["number", "string"])
    {
        let span = (offset as u32, (offset + text.len()) as u32);
        assert_eq!(result.expression_type_spans[&span].node_text, expected);
    }
}

#[test]
fn inherited_namespace_receivers_do_not_share_class_metadata() {
    let source = "namespace A { class C { value = 1; self() { return this; } } class D extends C {} const c = new D(); c.self().value = 2; } namespace B { class C<T> { constructor(public value:T) {} self() { return this; } } class D<T> extends C<T> {} const c = new D<string>(\"\"); c.self().value = \"ok\"; }";
    let file = tsc_rs_parser::parse("case.ts", source);
    assert!(file.diagnostics.is_empty());
    let symbols = tsc_rs_symbols::bind(&file);
    let mut checker = TypeChecker::new();
    checker.enable_expression_type_spans();
    let result = checker.check_with_options(
        &file,
        &symbols,
        &CompilerOptions {
            strict: Some(true),
            ..Default::default()
        },
    );
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    for ((offset, text), expected) in source
        .match_indices("c.self().value")
        .zip(["number", "string"])
    {
        let span = (offset as u32, (offset + text.len()) as u32);
        assert_eq!(result.expression_type_spans[&span].node_text, expected);
    }
}

#[test]
fn returned_primitive_assertion_retains_source() {
    assert_methods(
        "class C { f() { return 1 as number; } }",
        true,
        &[("f", 10, "() => number", "^^^^^^      ")],
    );
}

#[test]
fn returned_any_assertion_retains_source() {
    assert_methods(
        "class C { f() { return 1 as any; } }",
        true,
        &[("f", 10, "() => any", "^^^^^^   ")],
    );
}

#[test]
fn inherited_setter_instantiates_each_heritage_edge() {
    let source = "class Base<T> { get value(): any { return 0; } set value(v: T) {} self() { return this; } } class Mid<U> extends Base<U[]> {} class Derived extends Mid<string> {} const d = new Derived(); d.self().value = [\"ok\"]; d.self().value = [1];";
    let file = tsc_rs_parser::parse("case.ts", source);
    assert!(file.diagnostics.is_empty());
    let symbols = tsc_rs_symbols::bind(&file);
    let result = TypeChecker::new().check_with_options(
        &file,
        &symbols,
        &CompilerOptions {
            strict: Some(true),
            ..Default::default()
        },
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|d| d.code)
            .collect::<Vec<_>>(),
        [2322],
        "{:?}",
        result.diagnostics
    );
    let final_assignment = source[..source.len() - 1].rfind(';').unwrap() + 1;
    assert!(result.diagnostics[0].span.unwrap().start as usize >= final_assignment);
}

#[test]
fn inherited_static_setter_uses_write_type() {
    let source = "class Base { static get value(): any { return 0; } static set value(v: string) {} } class Derived extends Base {} Derived.value = \"ok\"; Derived.value = 1;";
    let file = tsc_rs_parser::parse("case.ts", source);
    assert!(file.diagnostics.is_empty());
    let symbols = tsc_rs_symbols::bind(&file);
    let result = TypeChecker::new().check_with_options(
        &file,
        &symbols,
        &CompilerOptions {
            strict: Some(true),
            ..Default::default()
        },
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|d| d.code)
            .collect::<Vec<_>>(),
        [2322],
        "{:?}",
        result.diagnostics
    );
    let final_assignment = source[..source.len() - 1].rfind(';').unwrap() + 1;
    assert!(result.diagnostics[0].span.unwrap().start as usize >= final_assignment);
}

#[test]
fn nested_namespace_class_registration_survives_scope_exit() {
    let source = "namespace Outer { export namespace Inner { export class C { value = 1; self() { return this; } } } } const c = new Outer.Inner.C(); const value: number = c.self().value;";
    let file = tsc_rs_parser::parse("case.ts", source);
    assert!(file.diagnostics.is_empty());
    let symbols = tsc_rs_symbols::bind(&file);
    let result = TypeChecker::new().check_with_options(
        &file,
        &symbols,
        &CompilerOptions {
            strict: Some(true),
            ..Default::default()
        },
    );
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
}

#[test]
fn qualified_base_class_arity_ignores_local_same_name() {
    let source = "namespace A { export class C { value = 1; } } namespace B { export class C<T> extends A.C { other!: T; } }";
    let file = tsc_rs_parser::parse("case.ts", source);
    assert!(file.diagnostics.is_empty());
    let symbols = tsc_rs_symbols::bind(&file);
    let result = TypeChecker::new().check_with_options(
        &file,
        &symbols,
        &CompilerOptions {
            strict: Some(true),
            ..Default::default()
        },
    );
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
}

#[test]
fn private_class_in_reopened_namespace_preserves_exported_shape() {
    let source = "namespace A { export class C { value = 1; } } namespace A { class C { read(p: A.C) { return p.value; } } } var c: { value: number }; var c: A.C;";
    let file = tsc_rs_parser::parse("case.ts", source);
    assert!(file.diagnostics.is_empty());
    let symbols = tsc_rs_symbols::bind(&file);
    let result = TypeChecker::new().check_with_options(
        &file,
        &symbols,
        &CompilerOptions {
            strict: Some(true),
            ..Default::default()
        },
    );
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
}

#[test]
fn private_namespace_class_does_not_replace_dotted_export() {
    let source = "namespace A.B.C { export class Value { size = 1; } } namespace A { export namespace B { namespace C { export class Value { name = \"\"; } } } } var value: { size: number }; var value: A.B.C.Value;";
    let file = tsc_rs_parser::parse("case.ts", source);
    assert!(file.diagnostics.is_empty());
    let symbols = tsc_rs_symbols::bind(&file);
    let result = TypeChecker::new().check_with_options(
        &file,
        &symbols,
        &CompilerOptions {
            strict: Some(true),
            ..Default::default()
        },
    );
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
}

#[test]
fn namespace_class_relations_use_current_shapes() {
    let source = "namespace A { class Base {} class Child extends Base {} function check() { const values: { item: Base }[] = [{ item: new Child() }, { item: new Base() }]; } } namespace B { class Base { value = 1; } class Child extends Base { other = 2; } function check() { const values: { item: Base }[] = [{ item: new Child() }, { item: new Base() }]; } }";
    let file = tsc_rs_parser::parse("case.ts", source);
    assert!(file.diagnostics.is_empty());
    let symbols = tsc_rs_symbols::bind(&file);
    let result = TypeChecker::new().check_with_options(
        &file,
        &symbols,
        &CompilerOptions {
            strict: Some(true),
            ..Default::default()
        },
    );
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
}

#[test]
fn namespace_implements_uses_lexical_interface_in_external_module() {
    let source = "export {}; interface I { value: string; } namespace A { interface I { value: number; } class C implements I { value = 1; } class D implements I { value = \"wrong\"; } }";
    let file = tsc_rs_parser::parse("case.ts", source);
    assert!(file.diagnostics.is_empty());
    let symbols = tsc_rs_symbols::bind(&file);
    let result = TypeChecker::new().check_with_options(
        &file,
        &symbols,
        &CompilerOptions {
            strict: Some(true),
            ..Default::default()
        },
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|d| d.code)
            .collect::<Vec<_>>(),
        [2416],
        "{:?}",
        result.diagnostics
    );
    assert!(result.diagnostics[0].span.unwrap().start as usize >= source.find("class D").unwrap());
}
