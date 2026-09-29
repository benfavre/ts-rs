// Expected diagnostics and spans checked against TypeScript 6.0.3.
use tsc_rs_ast::CompilerOptions;
use tsc_rs_types::TypeChecker;

fn check(sources: &[(&str, &str)], expected: &[(&str, u32, &str, &str)]) {
    let files: Vec<_> = sources
        .iter()
        .map(|(name, text)| tsc_rs_parser::parse(name, text))
        .collect();
    let refs: Vec<_> = files.iter().collect();
    let mut donor = TypeChecker::new();
    donor.register_available_files(
        &sources
            .iter()
            .map(|(name, _)| name.to_string())
            .collect::<Vec<_>>(),
    );
    donor.register_external_export_names(&refs);
    let mut actual = Vec::new();
    for file in &files {
        let symbols = tsc_rs_symbols::bind(file);
        let result = donor.clone().check_with_options(
            file,
            &symbols,
            &CompilerOptions {
                strict: Some(false),
                allow_synthetic_default_imports: Some(true),
                ..CompilerOptions::default()
            },
        );
        for diagnostic in result
            .diagnostics
            .into_iter()
            .filter(|diagnostic| diagnostic.code == 2303)
        {
            let span = diagnostic.span.unwrap();
            actual.push((
                file.file_name.clone(),
                span.start,
                diagnostic.message,
                file.text[span.start as usize..span.end as usize].to_string(),
            ));
        }
    }
    actual.sort();
    let mut expected: Vec<_> = expected
        .iter()
        .map(|(file, start, message, span)| {
            (
                file.to_string(),
                *start,
                message.to_string(),
                span.to_string(),
            )
        })
        .collect();
    expected.sort();
    assert_eq!(actual, expected, "{sources:?}");
}

#[test]
fn self_import() {
    check(
        &[(
            r###"/a.ts"###,
            r###"import self = require("./a"); export = self;"###,
        )],
        &[(
            r###"/a.ts"###,
            0,
            r###"Circular definition of import alias 'self'."###,
            r###"import self = require("./a");"###,
        )],
    );
}

#[test]
fn two_files() {
    check(
        &[
            (
                r###"/a.ts"###,
                r###"import self = require("./b"); export = self;"###,
            ),
            (
                r###"/b.ts"###,
                r###"import self = require("./a"); export = self;"###,
            ),
        ],
        &[(
            r###"/b.ts"###,
            0,
            r###"Circular definition of import alias 'self'."###,
            r###"import self = require("./a");"###,
        )],
    );
}

#[test]
fn three_files() {
    check(
        &[
            (
                r###"/a.ts"###,
                r###"import one = require("./b"); export = one;"###,
            ),
            (
                r###"/b.ts"###,
                r###"import two = require("./c"); export = two;"###,
            ),
            (
                r###"/c.ts"###,
                r###"import three = require("./a"); export = three;"###,
            ),
        ],
        &[(
            r###"/c.ts"###,
            0,
            r###"Circular definition of import alias 'three'."###,
            r###"import three = require("./a");"###,
        )],
    );
}

#[test]
fn ambient_self() {
    check(
        &[(
            r###"/a.d.ts"###,
            r###"declare module "a" { import self = require("a"); export = self; }"###,
        )],
        &[(
            r###"/a.d.ts"###,
            21,
            r###"Circular definition of import alias 'self'."###,
            r###"import self = require("a");"###,
        )],
    );
}

#[test]
fn ambient_cycle() {
    check(
        &[(
            r###"/a.d.ts"###,
            r###"declare module "a" { import b = require("b"); export = b; } declare module "b" { import a = require("a"); export = a; }"###,
        )],
        &[(
            r###"/a.d.ts"###,
            21,
            r###"Circular definition of import alias 'b'."###,
            r###"import b = require("b");"###,
        )],
    );
}

#[test]
fn unused_self_import() {
    check(
        &[(
            r###"/a.ts"###,
            r###"import self = require("./a"); export const x = 1;"###,
        )],
        &[],
    );
}

#[test]
fn grounded_exports() {
    check(
        &[
            (
                r###"/a.ts"###,
                r###"import B = require("./b"); class A { b?: B; } export = A;"###,
            ),
            (
                r###"/b.ts"###,
                r###"import A = require("./a"); class B { a?: A; } export = B;"###,
            ),
        ],
        &[],
    );
}

#[test]
fn local_chain() {
    check(
        &[(
            r###"/a.ts"###,
            r###"import first = second; import second = require("./a"); export = first;"###,
        )],
        &[(
            r###"/a.ts"###,
            0,
            r###"Circular definition of import alias 'first'."###,
            r###"import first = second;"###,
        )],
    );
}

#[test]
fn exported_import() {
    check(
        &[
            (
                r###"/a.ts"###,
                r###"export import first = require("./b"); export = first;"###,
            ),
            (
                r###"/b.ts"###,
                r###"import second = require("./a"); export = second;"###,
            ),
        ],
        &[(
            r###"/b.ts"###,
            0,
            r###"Circular definition of import alias 'second'."###,
            r###"import second = require("./a");"###,
        )],
    );
}

#[test]
fn qualified_export() {
    check(
        &[(
            r###"/a.ts"###,
            r###"namespace N { export import x = require("./a"); } export = N.x;"###,
        )],
        &[],
    );
}

#[test]
fn qualified_alias() {
    check(
        &[(
            r###"/a.ts"###,
            r###"namespace N { export import x = require("./a"); } import y = N.x; export = y;"###,
        )],
        &[],
    );
}

#[test]
fn named_export_cycle() {
    check(
        &[
            (r###"/a.ts"###, r###"export { x } from "./b";"###),
            (r###"/b.ts"###, r###"export { x } from "./a";"###),
        ],
        &[(
            r###"/b.ts"###,
            9,
            r###"Circular definition of import alias 'x'."###,
            r###"x"###,
        )],
    );
}

#[test]
fn named_import_cycle() {
    check(
        &[
            (
                r###"/a.ts"###,
                r###"import { x } from "./b"; export { x };"###,
            ),
            (
                r###"/b.ts"###,
                r###"import { x } from "./a"; export { x };"###,
            ),
        ],
        &[(
            r###"/b.ts"###,
            9,
            r###"Circular definition of import alias 'x'."###,
            r###"x"###,
        )],
    );
}

#[test]
fn namespace_self() {
    check(
        &[(
            r###"/a.ts"###,
            r###"import * as self from "./a"; export = self;"###,
        )],
        &[(
            r###"/a.ts"###,
            12,
            r###"Circular definition of import alias 'self'."###,
            r###"self"###,
        )],
    );
}

#[test]
fn default_self() {
    check(
        &[(
            r###"/a.ts"###,
            r###"import self from "./a"; export default self;"###,
        )],
        &[(
            r###"/a.ts"###,
            7,
            r###"Circular definition of import alias 'self'."###,
            r###"self"###,
        )],
    );
}

#[test]
fn default_equals() {
    check(
        &[(
            r###"/a.ts"###,
            r###"import self from "./a"; export = self;"###,
        )],
        &[(
            r###"/a.ts"###,
            7,
            r###"Circular definition of import alias 'self'."###,
            r###"self"###,
        )],
    );
}

#[test]
fn ambient_split() {
    check(
        &[
            (
                r###"/a.d.ts"###,
                r###"declare module "a" { import b = require("b"); export = b; }"###,
            ),
            (
                r###"/b.d.ts"###,
                r###"declare module "b" { import a = require("a"); export = a; }"###,
            ),
        ],
        &[(
            r###"/a.d.ts"###,
            21,
            r###"Circular definition of import alias 'b'."###,
            r###"import b = require("b");"###,
        )],
    );
}

#[test]
fn independent_cycles() {
    check(
        &[
            (
                r###"/a.ts"###,
                r###"import a = require("./a"); export = a;"###,
            ),
            (
                r###"/b.ts"###,
                r###"import b = require("./b"); export = b;"###,
            ),
        ],
        &[
            (
                r###"/a.ts"###,
                0,
                r###"Circular definition of import alias 'a'."###,
                r###"import a = require("./a");"###,
            ),
            (
                r###"/b.ts"###,
                0,
                r###"Circular definition of import alias 'b'."###,
                r###"import b = require("./b");"###,
            ),
        ],
    );
}

#[test]
fn downstream_import() {
    check(
        &[
            (r###"/use.ts"###, r###"import use = require("./a");"###),
            (
                r###"/a.ts"###,
                r###"import a = require("./b"); export = a;"###,
            ),
            (
                r###"/b.ts"###,
                r###"import b = require("./a"); export = b;"###,
            ),
        ],
        &[(
            r###"/b.ts"###,
            0,
            r###"Circular definition of import alias 'b'."###,
            r###"import b = require("./a");"###,
        )],
    );
}

#[test]
fn valid_forwarding() {
    check(
        &[
            (
                r###"/a.ts"###,
                r###"import b = require("./b"); export = b;"###,
            ),
            (r###"/b.ts"###, r###"class B {} export = B;"###),
        ],
        &[],
    );
}

#[test]
fn escaped_import() {
    check(
        &[(
            r###"/a.ts"###,
            r###"import \u0061 = require("./a"); export = a;"###,
        )],
        &[(
            r###"/a.ts"###,
            0,
            r###"Circular definition of import alias '\u0061'."###,
            r###"import \u0061 = require("./a");"###,
        )],
    );
}

#[test]
fn escaped_export() {
    check(
        &[(
            r###"/a.ts"###,
            r###"import a = require("./a"); export = \u0061;"###,
        )],
        &[(
            r###"/a.ts"###,
            0,
            r###"Circular definition of import alias 'a'."###,
            r###"import a = require("./a");"###,
        )],
    );
}

#[test]
fn type_only_default() {
    check(
        &[(
            r###"/a.ts"###,
            r###"import type type from "./a"; export default type;"###,
        )],
        &[(
            r###"/a.ts"###,
            7,
            r###"Circular definition of import alias 'type'."###,
            r###"type type"###,
        )],
    );
}

#[test]
fn umd_cycle() {
    check(
        &[(
            r###"/a.d.ts"###,
            r###"declare global { namespace N {} } export = N; export as namespace N;"###,
        )],
        &[(
            r###"/a.d.ts"###,
            46,
            r###"Circular definition of import alias 'N'."###,
            r###"export as namespace N;"###,
        )],
    );
}

#[test]
fn valid_umd() {
    check(
        &[(
            r###"/a.d.ts"###,
            r###"declare class C {} export = C; export as namespace N;"###,
        )],
        &[],
    );
}

#[test]
fn star_cycle() {
    check(
        &[
            (r###"/a.ts"###, r###"export * from "./b";"###),
            (r###"/b.ts"###, r###"export * from "./a";"###),
            (r###"/use.ts"###, r###"import {x} from "./a";"###),
        ],
        &[],
    );
}

#[test]
fn grounded_star() {
    check(
        &[
            (r###"/a.ts"###, r###"export * from "./b";"###),
            (
                r###"/b.ts"###,
                r###"export * from "./a"; export const x = 1;"###,
            ),
            (r###"/use.ts"###, r###"import {x} from "./a";"###),
        ],
        &[],
    );
}

#[test]
fn mixed_star() {
    check(
        &[
            (r###"/a.ts"###, r###"import {x} from "./b"; export {x};"###),
            (
                r###"/b.ts"###,
                r###"export * from "./a"; export * from "./c";"###,
            ),
            (r###"/c.ts"###, r###"export const x = 1;"###),
        ],
        &[(
            r###"/a.ts"###,
            31,
            r###"Circular definition of import alias 'x'."###,
            r###"x"###,
        )],
    );
}

#[test]
fn renamed_cycle() {
    check(
        &[
            (r###"/a.ts"###, r###"export {x as y} from "./b";"###),
            (r###"/b.ts"###, r###"export {y as x} from "./a";"###),
        ],
        &[(
            r###"/b.ts"###,
            8,
            r###"Circular definition of import alias 'x'."###,
            r###"y as x"###,
        )],
    );
}

#[test]
fn local_cycle() {
    check(
        &[(r###"/a.ts"###, r###"import a = b; import b = a;"###)],
        &[(
            r###"/a.ts"###,
            0,
            r###"Circular definition of import alias 'a'."###,
            r###"import a = b;"###,
        )],
    );
}

#[test]
fn namespace_local_cycle() {
    check(
        &[(
            r###"/a.ts"###,
            r###"namespace N { export import a = b; export import b = a; }"###,
        )],
        &[(
            r###"/a.ts"###,
            14,
            r###"Circular definition of import alias 'a'."###,
            r###"export import a = b;"###,
        )],
    );
}

#[test]
fn keyword_namespace() {
    check(
        &[(
            r###"/a.ts"###,
            r###"import * as as from "./a"; export = as;"###,
        )],
        &[(
            r###"/a.ts"###,
            12,
            r###"Circular definition of import alias 'as'."###,
            r###"as"###,
        )],
    );
}

#[test]
fn keyword_default() {
    check(
        &[(
            r###"/a.ts"###,
            r###"import from from "./a"; export default from;"###,
        )],
        &[(
            r###"/a.ts"###,
            7,
            r###"Circular definition of import alias 'from'."###,
            r###"from"###,
        )],
    );
}

#[test]
fn runtime_extension_cycle() {
    check(
        &[
            (
                r###"/a.mts"###,
                r###"import a = require("./b.cjs"); export = a;"###,
            ),
            (
                r###"/b.cts"###,
                r###"import b = require("./a.mjs"); export = b;"###,
            ),
        ],
        &[(
            r###"/b.cts"###,
            0,
            r###"Circular definition of import alias 'b'."###,
            r###"import b = require("./a.mjs");"###,
        )],
    );
}

#[test]
fn namespace_export_assignment_is_grounded() {
    check(
        &[(
            "/a.d.ts",
            "declare namespace N { export interface A {} } export = N; export as namespace N;",
        )],
        &[],
    );
}

#[test]
fn single_file_checker_indexes_aliases_without_a_donor() {
    let source = "import self = require(\"./a\"); export = self;";
    let file = tsc_rs_parser::parse("/a.ts", source);
    let symbols = tsc_rs_symbols::bind(&file);
    let result =
        TypeChecker::new().check_with_options(&file, &symbols, &CompilerOptions::default());
    let cycles: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 2303)
        .collect();
    assert_eq!(cycles.len(), 1);
    assert_eq!(cycles[0].span, Some(tsc_rs_ast::Span::new(0, 29)));
    assert_eq!(
        cycles[0].message,
        "Circular definition of import alias 'self'."
    );
}
