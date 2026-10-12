use tsc_rs_ast::{CompilerOptions, Diagnostic, ModuleKind, ScriptTarget};
use tsc_rs_types::TypeChecker;

fn check(source: &str, providers: &[(&str, &str)], strict_null_checks: bool) -> Vec<Diagnostic> {
    let file = tsc_rs_parser::parse("subject.ts", source);
    let providers: Vec<_> = providers
        .iter()
        .map(|(name, text)| tsc_rs_parser::parse(name, text))
        .collect();
    let mut files: Vec<_> = providers.iter().collect();
    files.push(&file);
    for file in &files {
        assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    }
    let mut checker = TypeChecker::new();
    checker.register_available_files(
        &files
            .iter()
            .map(|file| file.file_name.clone())
            .collect::<Vec<_>>(),
    );
    checker.register_virtual_module_paths(
        "subject.ts",
        &providers
            .iter()
            .map(|file| {
                (
                    format!("./{}", file.file_name.trim_end_matches(".ts")),
                    file.file_name.clone(),
                )
            })
            .collect::<Vec<_>>(),
    );
    if !providers.is_empty() {
        checker.inject_external_types(&files);
    }
    checker.set_current_file_name("subject.ts");
    let mut errors = checker
        .check_with_options(
            &file,
            &tsc_rs_symbols::bind(&file),
            &CompilerOptions {
                target: Some(ScriptTarget::ES2015),
                module: Some(ModuleKind::CommonJS),
                strict: Some(false),
                strict_null_checks: Some(strict_null_checks),
                ..CompilerOptions::default()
            },
        )
        .diagnostics;
    errors.sort_by(|a, b| {
        (a.span.map(|span| span.start), a.code, &a.message).cmp(&(
            b.span.map(|span| span.start),
            b.code,
            &b.message,
        ))
    });
    errors
}

fn assert_conditions(source: &str, providers: &[(&str, &str)], expected: &[(&str, bool)]) {
    let errors = check(source, providers, true);
    let actual: Vec<_> = errors
        .iter()
        .map(|error| {
            assert_eq!(error.code, 2845, "{errors:?}");
            let span = error.span.unwrap();
            (
                &source[span.start as usize..span.end as usize],
                error.message.as_str(),
            )
        })
        .collect();
    let expected: Vec<_> = expected
        .iter()
        .map(|(span, truth)| {
            (
                *span,
                format!("This condition will always return '{truth}'."),
            )
        })
        .collect();
    let expected: Vec<_> = expected
        .iter()
        .map(|(span, message)| (*span, message.as_str()))
        .collect();
    assert_eq!(actual, expected, "{errors:?}");
}

#[test]
fn direct_enum_members_report_numeric_and_string_truthiness() {
    assert_conditions("enum E { Zero, One, Neg = -1, Empty = '', Text = 'ok', Computed = Math.random() } if (E.Zero) {} if (E.One) {} if (E.Neg) {} if (E.Empty) {} if (E.Text) {} if (E.Computed) {} E.Zero ? 1 : 2; E.One && 1;", &[], &[("E.Zero",false),("E.One",true),("E.Neg",true),("E.Empty",false),("E.Text",true),("E.Zero",false),("E.One",true)]);
}

#[test]
fn conditions_respect_syntax_and_strict_null_checks() {
    let source = "enum E { Zero, One } while(E.Zero) {} do {} while(E.One); for(;E.One;) {} if(!E.One) {} if(!!E.One) {} if(E['One']) {} const alias=E; if(alias.One) {} import Alias=E; if(Alias.One) {} const v=E.One; if(v) {} if((E).One) {} if(E!.One) {} if((E as typeof E).One) {} E.One || 1; E.One ?? 1;";
    assert_conditions(source, &[], &[]);
    let errors = check(
        "enum E { A, B } if(E.A){} E.B ? 1 : 2; E.A && 1;",
        &[],
        false,
    );
    assert!(errors.is_empty(), "{errors:?}");
    assert_conditions("enum E { A } if((E.A)) {}", &[], &[("E.A", false)]);
}

#[test]
fn ambient_members_without_initializers_are_not_constants() {
    assert_conditions("declare enum E { A, B=1, C } declare const enum F { A, B, C=-1 } if(E.A){} if(E.B){} if(E.C){} if(F.A){} if(F.B){} if(F.C){}",&[],&[("E.B",true),("F.A",false),("F.B",true),("F.C",true)]);
}

#[test]
fn namespace_aliases_preserve_injected_enum_constants() {
    assert_conditions("namespace Other { import Alias = Local.Inner; if(Alias.E.Zero){} if(Alias.E.One){} if(Alias.E.Folded){} }", &[("provider.ts","namespace Local.Inner { export const enum E { Zero, One, Folded = (One << 2) - 4 } }")], &[("Alias.E.Zero",false),("Alias.E.One",true),("Alias.E.Folded",false)]);
}

#[test]
fn namespace_enums_with_the_same_name_keep_distinct_values() {
    assert_conditions("namespace A { export enum E { Zero, One } } namespace B { export enum E { Zero = 1, One = 0 } } if(A.E.Zero){} if(A.E.One){} if(B.E.Zero){} if(B.E.One){}", &[], &[("A.E.Zero",false),("A.E.One",true),("B.E.Zero",true),("B.E.One",false)]);
}

#[test]
fn lexical_and_structural_shadows_do_not_become_enum_symbols() {
    assert_conditions("enum E { A=1 } namespace N {export enum E {A=1}} function f(E: typeof N.E, N: {E: {A: 1}}) { if(E.A){} if(N.E.A){} } const obj = {E}; if(obj.E.A){}",&[],&[]);
}

#[test]
fn logical_conditions_match_reference_operand_selection() {
    assert_conditions("enum E {Zero,One} if(E.Zero || E.One){} if(E.Zero && E.One){} E.Zero && E.One; const x=E.Zero || 3; const y=(E.Zero || E.One) && 4; const z=(E.Zero && E.One) || 4;",&[],&[("E.Zero",false),("E.Zero",true),("E.One",true),("E.Zero",false),("E.One",true),("E.Zero",false),("E.Zero",true),("E.One",true),("E.Zero",false)]);
}

#[test]
fn nan_is_falsy_and_infinity_is_truthy() {
    assert_conditions(
        "enum E { A=0/0, B=1/0, C=-1/0 } if(E.A){} if(E.B){} if(E.C){}",
        &[],
        &[("E.A", false), ("E.B", true), ("E.C", true)],
    );
}

#[test]
fn imported_enum_aliases_and_module_members_keep_symbol_meanings() {
    assert_conditions("import {E, NS} from './provider'; import * as p from './provider'; if(E.One){} if(NS.E.One){} if(p.E.One){}", &[("provider.ts", "export enum E {Zero,One} export namespace NS {export enum E {Zero,One}}")], &[("NS.E.One",true),("p.E.One",true)]);
}
