use tsc_rs_ast::{CompilerOptions, Span};
use tsc_rs_types::TypeChecker;

#[test]
fn ts2403_compares_vars_only_within_the_same_namespace_body() {
    let source = r#"
namespace First {
    var shared: number;
}

namespace Second {
    var shared: string;
}

namespace Third {
    var conflict: number;
    var conflict: string;
}
"#;
    let file = tsc_rs_parser::parse("var_redeclaration_scopes.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    let diagnostics = TypeChecker::new()
        .check_with_options(&file, &symbols, &CompilerOptions::default())
        .diagnostics;
    let redeclarations: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 2403)
        .collect();

    assert_eq!(redeclarations.len(), 1, "diagnostics: {diagnostics:?}");
    assert!(
        redeclarations[0].message.contains("conflict"),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn ts2403_compares_vars_only_within_each_function_like_scope() {
    let source = r#"
class Example {
    static {
        var static_block: number;
        var static_block: string;
    }
    static {
        var static_block: boolean;
    }

    method() {
        var method_local: number;
        var method_local: string;
    }
    siblingMethod() {
        var method_local: boolean;
    }

    get property() {
        var accessor_local: number;
        var accessor_local: string;
        return 1;
    }
    set property(value: number) {
        var accessor_local: boolean;
    }

    constructor() {
        var constructor_local: number;
        var constructor_local: string;
    }
}

const object = {
    method() {
        var object_method_local: number;
        var object_method_local: string;
    },
    siblingMethod() {
        var object_method_local: boolean;
    },
    get property() {
        var object_accessor_local: number;
        var object_accessor_local: string;
        return 1;
    },
    set property(value: number) {
        var object_accessor_local: boolean;
    },
};
"#;
    let file = tsc_rs_parser::parse("var_redeclaration_function_scopes.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    let diagnostics = TypeChecker::new()
        .check_with_options(&file, &symbols, &CompilerOptions::default())
        .diagnostics;
    let redeclarations: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 2403)
        .collect();

    assert_eq!(redeclarations.len(), 6, "diagnostics: {diagnostics:?}");
    for name in [
        "static_block",
        "method_local",
        "accessor_local",
        "constructor_local",
        "object_method_local",
        "object_accessor_local",
    ] {
        let message_prefix = format!("Variable '{name}'");
        assert_eq!(
            redeclarations
                .iter()
                .filter(|diagnostic| diagnostic.message.contains(&message_prefix))
                .count(),
            1,
            "diagnostics for {name}: {diagnostics:?}"
        );
    }
}

#[test]
fn ts2403_isolates_contextually_typed_object_members() {
    let source = r#"
interface Context {
    first(): void;
    second(): void;
    property: number;
}

const object: Context = {
    first() {
        var method_local: number;
        var method_local: string;
    },
    second() {
        var method_local: boolean;
    },
    get property() {
        var accessor_local: number;
        var accessor_local: string;
        return 1;
    },
    set property(value: number) {
        var accessor_local: boolean;
    },
};
"#;
    let file = tsc_rs_parser::parse("var_redeclaration_contextual_members.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    let diagnostics = TypeChecker::new()
        .check_with_options(&file, &symbols, &CompilerOptions::default())
        .diagnostics;
    let redeclarations: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 2403)
        .collect();

    assert_eq!(redeclarations.len(), 2, "diagnostics: {diagnostics:?}");
    for name in ["method_local", "accessor_local"] {
        let message_prefix = format!("Variable '{name}'");
        assert_eq!(
            redeclarations
                .iter()
                .filter(|diagnostic| diagnostic.message.contains(&message_prefix))
                .count(),
            1,
            "diagnostics for {name}: {diagnostics:?}"
        );
    }
}

#[test]
fn ts2403_keeps_blocks_and_loops_in_the_enclosing_var_scope() {
    let source = r#"
function outer() {
    var block_local: number;
    {
        var block_local: string;
    }

    for (var index = 0; index < 1; index++) {
        var loop_local: number;
    }
    var loop_local: string;

    const nested = () => {
        var block_local: boolean;
        var nested_local: number;
        var nested_local: string;
    };
    const sibling = function () {
        var nested_local: boolean;
    };
}
"#;
    let file = tsc_rs_parser::parse("var_redeclaration_block_scopes.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    let diagnostics = TypeChecker::new()
        .check_with_options(&file, &symbols, &CompilerOptions::default())
        .diagnostics;
    let redeclarations: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 2403)
        .collect();

    assert_eq!(redeclarations.len(), 3, "diagnostics: {diagnostics:?}");
    for name in ["block_local", "loop_local", "nested_local"] {
        let message_prefix = format!("Variable '{name}'");
        assert_eq!(
            redeclarations
                .iter()
                .filter(|diagnostic| diagnostic.message.contains(&message_prefix))
                .count(),
            1,
            "diagnostics for {name}: {diagnostics:?}"
        );
    }
}

#[test]
fn ts2403_external_var_state_reaches_only_the_cloned_file_scope() {
    let mut donor = TypeChecker::new();
    donor.register_external_var_types(&[(
        "shared".to_string(),
        "number".to_string(),
        Span::new(4, 10),
        "first.ts".to_string(),
        true,
        false,
    )]);

    for (file_name, top_level_type) in [("second.ts", "string"), ("third.ts", "boolean")] {
        let source = format!(
            r#"
class Example {{
    method() {{
        var shared: {top_level_type};
    }}
}}
var shared: {top_level_type};
"#
        );
        let file = tsc_rs_parser::parse(file_name, &source);
        let symbols = tsc_rs_symbols::bind(&file);
        let diagnostics = donor
            .clone()
            .check_with_options(&file, &symbols, &CompilerOptions::default())
            .diagnostics;
        let redeclarations: Vec<_> = diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 2403)
            .collect();

        assert_eq!(redeclarations.len(), 1, "diagnostics: {diagnostics:?}");
        assert!(
            redeclarations[0].message.contains("Variable 'shared'"),
            "diagnostics: {diagnostics:?}"
        );
        assert_eq!(
            redeclarations[0]
                .related
                .as_ref()
                .and_then(|related| related.first())
                .and_then(|related| related.file_name.as_deref()),
            Some("first.ts"),
            "diagnostics: {diagnostics:?}"
        );
    }
}

#[test]
fn ts2403_isolates_parameter_defaults_fields_and_ambient_signatures() {
    let source = r#"
var outer_local: number;

declare function ambient(callback?: () => void): void;
function overloaded(callback?: () => void): void;
function overloaded(callback = () => {
    var default_local: number;
    var default_local: string;
}) {
    var implementation_local: number;
    var implementation_local: string;
}

interface FieldContext {
    run(): void;
}
class Fields {
    field: FieldContext = {
        run() {
            var field_local: number;
            var field_local: string;
            var outer_local: boolean;
        },
    };
}

var outer_local: string;
"#;
    let file = tsc_rs_parser::parse("var_redeclaration_initializers.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    let diagnostics = TypeChecker::new()
        .check_with_options(&file, &symbols, &CompilerOptions::default())
        .diagnostics;
    let redeclarations: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 2403)
        .collect();

    assert_eq!(redeclarations.len(), 4, "diagnostics: {diagnostics:?}");
    for name in [
        "outer_local",
        "default_local",
        "implementation_local",
        "field_local",
    ] {
        let message_prefix = format!("Variable '{name}'");
        assert_eq!(
            redeclarations
                .iter()
                .filter(|diagnostic| diagnostic.message.contains(&message_prefix))
                .count(),
            1,
            "diagnostics for {name}: {diagnostics:?}"
        );
    }
}

#[test]
fn ts2403_inferred_var_types_stay_within_javascript_function_scopes() {
    let source = r#"
function first() {
    var local = 1;
    var local = "";
}
function second() {
    var local = true;
}
"#;
    let file = tsc_rs_parser::parse("var_redeclaration_scopes.js", source);
    let symbols = tsc_rs_symbols::bind(&file);
    let diagnostics = TypeChecker::new()
        .check_with_options(
            &file,
            &symbols,
            &CompilerOptions {
                allow_js: Some(true),
                check_js: Some(true),
                ..CompilerOptions::default()
            },
        )
        .diagnostics;
    let redeclarations: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 2403)
        .collect();

    assert_eq!(redeclarations.len(), 1, "diagnostics: {diagnostics:?}");
    assert!(
        redeclarations[0].message.contains("Variable 'local'"),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn ts2403_scope_state_survives_parse_recovery_between_functions() {
    let source = r#"
function first() {
    var recovered: number;
    var recovered: string;
}
)
function second() {
    var recovered: boolean;
}
"#;
    let file = tsc_rs_parser::parse("var_redeclaration_recovery.ts", source);
    assert!(
        !file.diagnostics.is_empty(),
        "fixture must exercise parser recovery"
    );
    let symbols = tsc_rs_symbols::bind(&file);
    let diagnostics = TypeChecker::new()
        .check_with_options(&file, &symbols, &CompilerOptions::default())
        .diagnostics;
    let redeclarations: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 2403)
        .collect();

    assert_eq!(redeclarations.len(), 1, "diagnostics: {diagnostics:?}");
    assert!(
        redeclarations[0].message.contains("Variable 'recovered'"),
        "diagnostics: {diagnostics:?}"
    );
}
