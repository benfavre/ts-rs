use tsc_rs_ast::{CompilerOptions, Diagnostic, Span};
use tsc_rs_types::TypeChecker;

fn check(
    file_name: &str,
    source: &str,
    experimental_decorators: Option<bool>,
    check_js: Option<bool>,
) -> (Vec<Diagnostic>, Vec<Diagnostic>) {
    let file = tsc_rs_parser::parse(file_name, source);
    let parse_diagnostics = file.diagnostics.clone();
    let symbols = tsc_rs_symbols::bind(&file);
    let options = CompilerOptions {
        experimental_decorators,
        allow_js: file_name.ends_with(".js").then_some(true),
        check_js,
        ..CompilerOptions::default()
    };
    let semantic_diagnostics = TypeChecker::new()
        .check_with_options(&file, &symbols, &options)
        .diagnostics;
    (parse_diagnostics, semantic_diagnostics)
}

fn ts1206(diagnostics: &[Diagnostic]) -> Vec<Span> {
    diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 1206)
        .map(|diagnostic| {
            assert_eq!(diagnostic.message, "Decorators are not valid here.");
            diagnostic.span.expect("TS1206 should carry a span")
        })
        .collect()
}

fn starts(source: &str, needles: &[&str]) -> Vec<Span> {
    needles
        .iter()
        .map(|needle| {
            let start = source
                .find(needle)
                .unwrap_or_else(|| panic!("missing {needle}")) as u32;
            Span::new(start, start + 1)
        })
        .collect()
}

#[test]
fn legacy_decorators_require_class_declarations_and_reject_private_names() {
    let source = r#"
class Declaration {
    @fieldDec ["field"]: unknown;
    @methodDec ["method"]() {}
    named(@methodParamDec parameter: unknown) {}
    @getterDec get ["value"]() { return 1; }
    @setterDec set value(@setterParamDec value: number) {}
    @privateDec #private: unknown;
    @privateMethodDec #method() {}
    @privateGetterDec get #value() { return 1; }
    constructor(@constructorParamDec value: unknown) {}
}
const Expression = @classExpressionDec class {
    @expressionFieldDec ["field"]: unknown;
    @expressionMethodDec ["method"]() {}
    @expressionGetterDec get ["value"]() { return 1; }
    named(@expressionParamDec value: unknown) {}
};
"#;
    let (parse_diagnostics, semantic_diagnostics) = check("legacy.ts", source, Some(true), None);
    assert!(parse_diagnostics.is_empty(), "{parse_diagnostics:#?}");
    assert_eq!(
        ts1206(&semantic_diagnostics),
        starts(
            source,
            &[
                "@privateDec",
                "@privateMethodDec",
                "@privateGetterDec",
                "@classExpressionDec",
                "@expressionFieldDec",
                "@expressionMethodDec",
                "@expressionGetterDec",
                "@expressionParamDec",
            ],
        ),
        "{semantic_diagnostics:#?}"
    );
}

#[test]
fn standard_decorators_reject_parameters_and_non_runtime_properties() {
    let source = r#"
@classDec
abstract class Declaration {
    @abstractFieldDec abstract field: unknown;
    @abstractGetterDec abstract get value(): number;
    @declareFieldDec declare other: unknown;
    @privateFieldDec #private: unknown;
    @privateMethodDec #method() {}
    @privateGetterDec get #value() { return 1; }
    @staticFieldDec static ["static"]: unknown;
    @methodDec ["method"]() {}
    named(@methodParamDec parameter: unknown) {}
    @setterDec set value(@setterParamDec value: number) {}
    @constructorDec constructor(@constructorParamDec value: unknown) {}
}
const Expression = @classExpressionDec class {
    @expressionFieldDec ["field"]: unknown;
    @expressionMethodDec ["method"]() {}
    named(@expressionParamDec value: unknown) {}
};
function plain(@functionParamDec value: unknown) {}
"#;
    let expected = starts(
        source,
        &[
            "@abstractFieldDec",
            "@abstractGetterDec",
            "@declareFieldDec",
            "@methodParamDec",
            "@setterParamDec",
            "@constructorDec",
            "@constructorParamDec",
            "@expressionParamDec",
            "@functionParamDec",
        ],
    );
    for experimental_decorators in [None, Some(false)] {
        let (parse_diagnostics, semantic_diagnostics) =
            check("standard.ts", source, experimental_decorators, None);
        assert!(parse_diagnostics.is_empty(), "{parse_diagnostics:#?}");
        assert_eq!(
            ts1206(&semantic_diagnostics),
            expected,
            "{experimental_decorators:?}: {semantic_diagnostics:#?}"
        );
    }
}

#[test]
fn ambient_members_follow_implementation_and_parameter_rules() {
    let source = r#"
declare class Ambient {
    @propertyDec property: unknown;
    @methodDec method(): void;
    @getterDec get value(): number;
    constructor(@constructorParameterDec value: unknown);
}
"#;
    for experimental_decorators in [None, Some(false), Some(true)] {
        let (parse_diagnostics, semantic_diagnostics) =
            check("ambient.ts", source, experimental_decorators, None);
        assert!(parse_diagnostics.is_empty(), "{parse_diagnostics:#?}");
        assert_eq!(
            ts1206(&semantic_diagnostics),
            starts(source, &["@getterDec", "@constructorParameterDec"],),
            "{experimental_decorators:?}: {semantic_diagnostics:#?}"
        );
    }
}

#[test]
fn invalid_decorator_lists_report_once_and_do_not_check_the_expression() {
    let source = r#"
@functionMissing
function invalidTarget() {}
function decorated(@firstMissing @secondMissing parameter: unknown) {}
const Expression = @classMissingA @classMissingB class {};
"#;
    let (parse_diagnostics, semantic_diagnostics) = check("lists.ts", source, Some(true), None);
    assert!(parse_diagnostics.is_empty(), "{parse_diagnostics:#?}");
    assert_eq!(
        ts1206(&semantic_diagnostics),
        starts(
            source,
            &["@functionMissing", "@firstMissing", "@classMissingA"],
        ),
        "{semantic_diagnostics:#?}"
    );
    assert!(
        semantic_diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != 2304),
        "{semantic_diagnostics:#?}"
    );
}

#[test]
fn valid_decorator_expression_diagnostics_exclude_the_at_sign() {
    let source = r#"
class C {
    @missing plain() {}
    @computed ["name"]() {}
    @called() other() {}
}
"#;
    for experimental_decorators in [None, Some(true)] {
        let (parse_diagnostics, semantic_diagnostics) =
            check("expressions.ts", source, experimental_decorators, None);
        assert!(parse_diagnostics.is_empty(), "{parse_diagnostics:#?}");
        let observed: Vec<_> = semantic_diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 2304)
            .map(|diagnostic| diagnostic.span.expect("TS2304 span"))
            .collect();
        let expected: Vec<_> = ["missing", "computed", "called"]
            .into_iter()
            .map(|needle| {
                let start = source.find(needle).unwrap() as u32;
                Span::new(start, start + needle.len() as u32)
            })
            .collect();
        assert_eq!(
            observed, expected,
            "{experimental_decorators:?}: {semantic_diagnostics:#?}"
        );
    }
}

#[test]
fn method_overloads_keep_their_distinct_decorator_diagnostic_family() {
    let source = r#"
class C {
    @decorator overloaded(value: unknown): void;
    overloaded(value: unknown): void {}
}
"#;
    for experimental_decorators in [None, Some(true)] {
        let (parse_diagnostics, semantic_diagnostics) =
            check("overload.ts", source, experimental_decorators, None);
        assert!(parse_diagnostics.is_empty(), "{parse_diagnostics:#?}");
        assert!(
            ts1206(&semantic_diagnostics).is_empty(),
            "{experimental_decorators:?}: {semantic_diagnostics:#?}"
        );
        let ts1249: Vec<_> = semantic_diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 1249)
            .collect();
        assert_eq!(ts1249.len(), 1, "{semantic_diagnostics:#?}");
        assert_eq!(
            ts1249[0].message,
            "A decorator can only decorate a method implementation, not an overload."
        );
        assert_eq!(ts1249[0].span, Some(starts(source, &["@decorator"])[0]));
    }
}

#[test]
fn decorated_this_parameters_keep_ts1433_precedence() {
    let source = r#"
class C {
    method(@methodDecorator this: C) {}
}
function plain(@functionDecorator this: unknown) {}
"#;
    for experimental_decorators in [None, Some(false), Some(true)] {
        let (parse_diagnostics, semantic_diagnostics) =
            check("this-parameters.ts", source, experimental_decorators, None);
        let ts1433: Vec<_> = parse_diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 1433)
            .collect();
        assert_eq!(ts1433.len(), 2, "{parse_diagnostics:#?}");
        assert!(ts1433.iter().all(|diagnostic| {
            diagnostic.message
                == "Neither decorators nor modifiers may be applied to 'this' parameters."
        }));
        assert!(
            ts1206(&semantic_diagnostics).is_empty(),
            "{experimental_decorators:?}: {semantic_diagnostics:#?}"
        );
    }
}

#[test]
fn javascript_this_parameters_keep_the_syntactic_ts1206_companion() {
    let source = "function plain(@decorator this) {}";
    let start = source.find("@decorator").unwrap() as u32;
    for check_js in [Some(false), Some(true)] {
        let (parse_diagnostics, semantic_diagnostics) =
            check("this-parameter.js", source, None, check_js);
        assert_eq!(
            parse_diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == 1433)
                .count(),
            1,
            "{parse_diagnostics:#?}"
        );
        assert_eq!(
            ts1206(&semantic_diagnostics),
            [Span::new(start, start + "@decorator".len() as u32)],
            "{semantic_diagnostics:#?}"
        );
    }

    let (_, legacy) = check("this-parameter.js", source, Some(true), Some(true));
    assert!(ts1206(&legacy).is_empty(), "{legacy:#?}");
}

#[test]
fn arrow_and_missing_parameters_do_not_invent_semantic_ts1206() {
    let arrow = "const fn = (@decorator value) => value;";
    for experimental_decorators in [None, Some(false), Some(true)] {
        let (_, typescript) = check("arrow.ts", arrow, experimental_decorators, None);
        assert!(ts1206(&typescript).is_empty(), "{typescript:#?}");

        for check_js in [Some(false), Some(true)] {
            let (_, javascript) = check("arrow.js", arrow, experimental_decorators, check_js);
            let start = arrow.find("@decorator").unwrap() as u32;
            assert_eq!(
                ts1206(&javascript),
                [Span::new(start, start + "@decorator".len() as u32)],
                "{javascript:#?}"
            );
        }
    }

    let missing = "function fn(@\nvalue) {}";
    let (_, typescript) = check("missing.ts", missing, None, None);
    assert!(ts1206(&typescript).is_empty(), "{typescript:#?}");
    let (_, javascript) = check("missing.js", missing, None, Some(true));
    let start = missing.find('@').unwrap() as u32;
    assert_eq!(
        ts1206(&javascript),
        [Span::new(start, start + "@\nvalue".len() as u32)],
        "{javascript:#?}"
    );
}

#[test]
fn javascript_matches_syntactic_and_checkjs_semantic_spans() {
    let source = "class C { method(@decorator parameter) {} }";
    let decorator_start = source.find("@decorator").unwrap() as u32;

    let (parse_diagnostics, unchecked) = check("decorators.js", source, None, Some(false));
    assert!(parse_diagnostics.is_empty(), "{parse_diagnostics:#?}");
    assert_eq!(
        ts1206(&unchecked),
        [Span::new(
            decorator_start,
            decorator_start + "@decorator".len() as u32,
        )]
    );

    let (_, checked) = check("decorators.js", source, None, Some(true));
    assert_eq!(
        ts1206(&checked),
        [
            Span::new(decorator_start, decorator_start + 1),
            Span::new(decorator_start, decorator_start + "@decorator".len() as u32,),
        ]
    );

    let (_, legacy) = check("decorators.js", source, Some(true), Some(true));
    assert!(ts1206(&legacy).is_empty(), "{legacy:#?}");
}

#[test]
fn javascript_discarded_decorators_follow_checkjs_phase_rules() {
    let source = r#"
@variableDec const value = 1;
@interfaceDec interface I {}
@usingDec using resource = null;
@missingUsingInitializer using missing;
@objectUsingBinding using {} = resource;
@arrayUsingBinding using [] = resource;
@awaitUsingDec await using awaited = null;
@missingAwaitUsingInitializer await using missingAwaited;
@objectAwaitUsingBinding await using {} = resource;
@invalidUsingBinding using 1;
@invalidAwaitUsingBinding await using [];
@bareVar var;
@invalidVar var 1;
@bareLet let;
@invalidLet let 1;
@bareConst const;
@invalidConst const 1;
@bareUsing using;
class C {
    @staticBlockDec static {}
}
export @betweenExportAndDefault default class Invalid {}
"#;
    let expected_syntactic = [
        "@variableDec",
        "@usingDec",
        "@missingUsingInitializer",
        "@objectUsingBinding",
        "@arrayUsingBinding",
        "@awaitUsingDec",
        "@missingAwaitUsingInitializer",
        "@objectAwaitUsingBinding",
        "@invalidUsingBinding",
        "@invalidAwaitUsingBinding",
        "@bareVar",
        "@invalidVar",
        "@bareLet",
        "@invalidLet",
        "@bareConst",
        "@invalidConst",
        "@bareUsing",
        "@staticBlockDec",
        "@betweenExportAndDefault",
    ];
    let (unchecked_parse, unchecked_semantic) = check("contexts.js", source, None, Some(false));
    assert_eq!(
        ts1206(&unchecked_parse),
        expected_syntactic
            .iter()
            .map(|needle| {
                let start = source.find(needle).unwrap() as u32;
                Span::new(start, start + needle.len() as u32)
            })
            .collect::<Vec<_>>(),
        "{unchecked_parse:#?}"
    );
    assert!(
        ts1206(&unchecked_semantic).is_empty(),
        "{unchecked_semantic:#?}"
    );

    let (checked_parse, checked_semantic) = check("contexts.js", source, None, Some(true));
    assert_eq!(ts1206(&checked_parse), ts1206(&unchecked_parse));
    let variable = source.find("@variableDec").unwrap() as u32;
    let interface = source.find("@interfaceDec").unwrap() as u32;
    let using = source.find("@usingDec").unwrap() as u32;
    let missing_using = source.find("@missingUsingInitializer").unwrap() as u32;
    let object_using = source.find("@objectUsingBinding").unwrap() as u32;
    let array_using = source.find("@arrayUsingBinding").unwrap() as u32;
    let await_using = source.find("@awaitUsingDec").unwrap() as u32;
    let missing_await_using = source.find("@missingAwaitUsingInitializer").unwrap() as u32;
    let object_await_using = source.find("@objectAwaitUsingBinding").unwrap() as u32;
    let bare_var = source.find("@bareVar").unwrap() as u32;
    let bare_let = source.find("@bareLet").unwrap() as u32;
    let bare_const = source.find("@bareConst").unwrap() as u32;
    let bare_using = source.find("@bareUsing").unwrap() as u32;
    let static_block = source.find("@staticBlockDec").unwrap() as u32;
    let export = source.find("@betweenExportAndDefault").unwrap() as u32;
    assert_eq!(
        ts1206(&checked_semantic),
        [
            Span::new(variable, variable + 1),
            Span::new(interface, interface + 1),
            Span::new(using, using + 1),
            Span::new(missing_using, missing_using + 1),
            Span::new(object_using, object_using + 1),
            Span::new(array_using, array_using + 1),
            Span::new(await_using, await_using + 1),
            Span::new(missing_await_using, missing_await_using + 1),
            Span::new(object_await_using, object_await_using + 1),
            Span::new(bare_var, bare_var + 1),
            Span::new(bare_let, bare_let + 1),
            Span::new(bare_const, bare_const + 1),
            Span::new(bare_using, bare_using + 1),
            Span::new(static_block, static_block + 1),
            Span::new(export, export + "@betweenExportAndDefault".len() as u32,),
        ],
        "{checked_semantic:#?}"
    );
}

#[test]
fn javascript_method_overload_diagnostic_requires_checkjs() {
    let source = "class C { @decorator method(); method() {} }";
    let (_, unchecked) = check("overload.js", source, None, Some(false));
    assert!(
        unchecked.iter().all(|diagnostic| diagnostic.code != 1249),
        "{unchecked:#?}"
    );

    let (_, checked) = check("overload.js", source, None, Some(true));
    let ts1249: Vec<_> = checked
        .iter()
        .filter(|diagnostic| diagnostic.code == 1249)
        .collect();
    assert_eq!(ts1249.len(), 1, "{checked:#?}");
    let start = source.find("@decorator").unwrap() as u32;
    assert_eq!(ts1249[0].span, Some(Span::new(start, start + 1)));
}

#[test]
fn javascript_export_recovery_adds_only_the_checkjs_first_token() {
    let cases = [
        ("@decorator export * from \"module\";", true),
        ("@decorator export = value;", false),
    ];
    for (source, has_syntactic) in cases {
        let start = source.find("@decorator").unwrap() as u32;
        let (unchecked_parse, unchecked_semantic) = check("exports.js", source, None, Some(false));
        assert_eq!(
            ts1206(&unchecked_parse),
            if has_syntactic {
                vec![Span::new(start, start + "@decorator".len() as u32)]
            } else {
                Vec::new()
            },
            "{unchecked_parse:#?}"
        );
        assert!(ts1206(&unchecked_semantic).is_empty());

        let (checked_parse, checked_semantic) = check("exports.js", source, None, Some(true));
        assert_eq!(ts1206(&checked_parse), ts1206(&unchecked_parse));
        assert_eq!(
            ts1206(&checked_semantic),
            [Span::new(start, start + 1)],
            "{checked_semantic:#?}"
        );
    }
}
