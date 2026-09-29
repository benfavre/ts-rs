use tsc_rs_ast::{CompilerOptions, Diagnostic};
use tsc_rs_types::TypeChecker;

fn check(source: &str) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse("uncalled_function_truthiness.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    TypeChecker::new()
        .check_with_options(
            &file,
            &symbols,
            &CompilerOptions {
                strict_null_checks: Some(true),
                ..CompilerOptions::default()
            },
        )
        .diagnostics
}

#[test]
fn ts2774_follows_typescript_syntax_and_callable_presence_rules() {
    let source = r#"
function forms<T extends () => void>(
    generic: T,
    fn: () => void,
    obj: { cb: () => void; nested: { cb: () => void } },
    nullable: { cb: () => void } | undefined,
    optional: { cb?: () => void },
) {
    if (fn) {}
    if (!fn) {}
    if (fn as () => void) {}
    fn && 1;
    fn || 1;
    fn ?? 1;
    if (obj["cb"]) {}
    if (obj["nested"].cb) {}
    if (obj.nested["cb"]) {}
    if (obj?.cb) {}
    if (nullable?.cb) {}
    if (optional?.cb) {}
    if (generic) {}
}

declare function returnsFunction(value: string): () => void;
if (returnsFunction(123)) {}
"#;

    let diagnostics = check(source);
    let actual: Vec<_> = diagnostics
        .iter()
        .map(|diagnostic| {
            let span = diagnostic.span.expect("diagnostic should have a span");
            (
                diagnostic.code,
                &source[span.start as usize..span.end as usize],
            )
        })
        .collect();
    assert_eq!(
        actual,
        [
            (2774, "fn"),
            (2774, "fn"),
            (2774, "obj[\"nested\"].cb"),
            (2774, "obj?.cb"),
            (2774, "generic"),
            (2345, "123"),
        ],
        "diagnostics: {diagnostics:#?}",
    );
}

#[test]
fn ts2774_respects_optional_class_methods_and_block_lexical_shadows() {
    let source = r#"
declare class Container {
    method?(): void;
    static staticMethod?(): void;
}
declare const container: Container;
if (container.method) {}
if (Container.staticMethod) {}

function outer(fn: () => void) {
    if (fn) {
        {
            class fn {}
            fn;
        }
    }
}

interface Asserted {
    always(): true;
}
function assertedReceiver(value: unknown) {
    if ((value as Asserted).always) {}
}

class RequiredMethods {
    optional?: () => void;
    required() {}
    test() {
        if (this.required) {}
        if (this.optional) {}
        this.required && 1 && this.required();
    }
}
"#;

    let diagnostics = check(source);
    let actual: Vec<_> = diagnostics
        .iter()
        .map(|diagnostic| {
            let span = diagnostic.span.expect("diagnostic should have a span");
            (
                diagnostic.code,
                &source[span.start as usize..span.end as usize],
            )
        })
        .collect();
    assert_eq!(
        actual,
        [(2774, "fn"), (2774, "this.required")],
        "diagnostics: {diagnostics:#?}"
    );
}
