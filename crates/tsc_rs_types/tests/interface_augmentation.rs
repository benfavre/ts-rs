use tsc_rs_ast::{CompilerOptions, Diagnostic, ScriptTarget, SourceFile};
use tsc_rs_types::{load_stdlib_sources, TypeChecker};

fn check_with_declarations(declarations: &[&SourceFile], source: &str) -> Vec<Diagnostic> {
    let use_file = tsc_rs_parser::parse("use.ts", source);
    let symbols = tsc_rs_symbols::bind(&use_file);
    let mut donor = TypeChecker::new();
    donor.inject_external_types(declarations);
    donor.take_diagnostics();
    donor
        .check_with_options(&use_file, &symbols, &CompilerOptions::default())
        .diagnostics
}

fn diagnostic_lhs<'a>(source: &'a str, diagnostics: &[Diagnostic]) -> Vec<&'a str> {
    diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == 2322)
        .map(|diagnostic| {
            let span = diagnostic.span.expect("TS2322 should carry a span");
            &source[span.start as usize..span.end as usize]
        })
        .collect()
}

fn diagnostic_lines(source: &str, diagnostics: &[Diagnostic], code: u32) -> Vec<usize> {
    diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == code)
        .map(|diagnostic| {
            let start = diagnostic
                .span
                .expect("diagnostic should carry a span")
                .start as usize;
            source.as_bytes()[..start]
                .iter()
                .filter(|byte| **byte == b'\n')
                .count()
                + 1
        })
        .collect()
}

fn check_file_from_donor(donor: &TypeChecker, file: &SourceFile) -> Vec<Diagnostic> {
    let symbols = tsc_rs_symbols::bind(file);
    let mut checker = donor.clone();
    checker.set_current_file_name(&file.file_name);
    checker
        .check_with_options(file, &symbols, &CompilerOptions::default())
        .diagnostics
}

#[test]
fn global_interface_fragments_merge_in_both_file_orders() {
    let first = tsc_rs_parser::parse("first.d.ts", "interface Merged { first: string }");
    let second = tsc_rs_parser::parse("second.d.ts", "interface Merged { second: number }");
    let source = r#"
declare let merged: Merged;
const first: string = merged.first;
const second: number = merged.second;
"#;

    for declarations in [[&first, &second], [&second, &first]] {
        let diagnostics = check_with_declarations(&declarations, source);
        assert!(
            diagnostics.is_empty(),
            "global fragments should merge independent of file order: {diagnostics:?}"
        );
    }
}

#[test]
fn well_known_symbol_augmentations_are_order_independent_and_distinct() {
    let base = tsc_rs_parser::parse(
        "base.d.ts",
        r#"
interface SymbolConstructor {
    readonly iterator: unique symbol;
    readonly toStringTag: unique symbol;
}
declare var Symbol: SymbolConstructor;
interface A { common: number }
interface B { common: number }
"#,
    );
    let augmentation = tsc_rs_parser::parse(
        "augmentation.d.ts",
        r#"
interface A {
    readonly [Symbol.iterator]: () => "A iterator";
    readonly [Symbol.toStringTag]: "A";
}
interface B {
    readonly [Symbol.iterator]: () => "B iterator";
    readonly [Symbol.toStringTag]: "B";
}
"#,
    );
    let source = r#"
declare let a: A;
declare let b: B;
a = b;
b = a;
a = a;
b = b;
"#;

    for declarations in [[&base, &augmentation], [&augmentation, &base]] {
        let diagnostics = check_with_declarations(&declarations, source);
        assert_eq!(
            diagnostic_lhs(source, &diagnostics),
            ["a", "b"],
            "well-known symbol members should remain distinct in either order: {diagnostics:?}"
        );
        assert_eq!(
            diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == 2322)
                .count(),
            2,
            "only the two crossed assignments should fail: {diagnostics:?}"
        );
    }
}

#[test]
fn array_augmentation_preserves_canonical_array_assignability() {
    let array = tsc_rs_parser::parse(
        "array.d.ts",
        r#"
interface Array<T> {
    fill(value: T): this;
}
"#,
    );
    let source = r#"
interface Entity { id: string }
declare const length: number;
const inferred: (Entity | null)[] = new Array(length).fill(null);
const explicit: string[] = new Array<string>(length).fill("");
new Array<string>(length).fill(123);
"#;
    let diagnostics = check_with_declarations(&[&array], source);
    assert_eq!(
        diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code)
            .collect::<Vec<_>>(),
        [2345],
        "explicit element types must be preserved without breaking equivalent arrays: {diagnostics:?}"
    );
}

#[test]
fn local_array_constructor_is_not_treated_as_the_global_builtin() {
    let source = r#"
declare const Array: {
    new(): { local: true };
};
const value: { local: true } = new Array();
"#;
    let diagnostics = check_with_declarations(&[], source);
    assert!(
        diagnostics.is_empty(),
        "a lexical constructor named Array must retain its declared result: {diagnostics:?}"
    );
}

#[test]
fn es2015_stdlib_distinguishes_typed_array_interfaces() {
    let mut options = CompilerOptions::default();
    options.target = Some(ScriptTarget::ES2015);
    let libraries = load_stdlib_sources(&options);
    if libraries.is_empty() {
        eprintln!("TypeScript standard-library declarations are unavailable; skipping");
        return;
    }
    let parsed: Vec<_> = libraries
        .iter()
        .map(|library| tsc_rs_parser::parse(&library.file_name, &library.source))
        .collect();
    let declarations: Vec<_> = parsed.iter().collect();
    let source = r#"
let int8 = new Int8Array(1);
let uint8 = new Uint8Array(1);
int8 = uint8;
uint8 = int8;
int8 = int8;
uint8 = uint8;
"#;
    let diagnostics = check_with_declarations(&declarations, source);
    assert_eq!(
        diagnostic_lhs(source, &diagnostics),
        ["int8", "uint8"],
        "typed-array literal tags should reject only crossed assignments: {diagnostics:?}"
    );
}

#[test]
fn typed_array_cross_assignability_matches_all_64_oracle_locations() {
    let source = include_str!("../../../tests/cases/compiler/typedArraysCrossAssignability01.ts");
    let diagnostics = check_with_declarations(&[], source);
    // Upstream's baseline strips the leading directive and blank line before
    // reporting its virtual source. Add those two physical lines back for the
    // checked-in compiler-case file used here.
    let expected_lines: Vec<_> = [
        13, 14, 15, 16, 17, 18, 19, 20, 22, 24, 25, 26, 27, 28, 29, 30, 32, 33, 35, 36, 37, 38, 39,
        40, 42, 43, 44, 46, 47, 48, 49, 50, 52, 53, 54, 55, 57, 58, 59, 60, 62, 63, 64, 65, 66, 67,
        69, 70, 72, 73, 74, 75, 76, 77, 78, 80, 82, 83, 84, 85, 86, 87, 88, 89,
    ]
    .into_iter()
    .map(|line| line + 2)
    .collect();
    assert_eq!(
        diagnostic_lines(source, &diagnostics, 2322),
        expected_lines,
        "typed-array cross assignments should match the TypeScript oracle exactly"
    );
    assert!(
        diagnostics.iter().all(|diagnostic| diagnostic.code == 2322),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn same_module_fragments_merge_but_different_modules_stay_isolated() {
    let directory =
        std::env::temp_dir().join(format!("tsc-rs-interface-owners-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let package_path = directory.join("package.d.ts");
    let peer_path = directory.join("peer.d.ts");
    let package_use_path = directory.join("use-package.ts");
    let peer_use_path = directory.join("use-peer.ts");
    std::fs::write(
        &package_path,
        "export interface Shared { first: string }\n\
         export interface Shared { second: number }\n",
    )
    .unwrap();
    std::fs::write(&peer_path, "export interface Shared { peer: boolean }\n").unwrap();
    std::fs::write(
        &package_use_path,
        "import type { Shared } from './package';\n\
         declare let value: Shared;\n\
         const first: string = value.first;\n\
         const second: number = value.second;\n\
         value.peer;\n",
    )
    .unwrap();
    std::fs::write(
        &peer_use_path,
        "import type { Shared } from './peer';\n\
         declare let value: Shared;\n\
         const peer: boolean = value.peer;\n\
         value.first;\n",
    )
    .unwrap();

    let package = tsc_rs_parser::parse(
        package_path.to_string_lossy().as_ref(),
        &std::fs::read_to_string(&package_path).unwrap(),
    );
    let peer = tsc_rs_parser::parse(
        peer_path.to_string_lossy().as_ref(),
        &std::fs::read_to_string(&peer_path).unwrap(),
    );
    let package_use = tsc_rs_parser::parse(
        package_use_path.to_string_lossy().as_ref(),
        &std::fs::read_to_string(&package_use_path).unwrap(),
    );
    let peer_use = tsc_rs_parser::parse(
        peer_use_path.to_string_lossy().as_ref(),
        &std::fs::read_to_string(&peer_use_path).unwrap(),
    );

    for declarations in [[&package, &peer], [&peer, &package]] {
        let mut donor = TypeChecker::new();
        donor.inject_external_types(&declarations);
        donor.take_diagnostics();
        let package_diagnostics = check_file_from_donor(&donor, &package_use);
        let peer_diagnostics = check_file_from_donor(&donor, &peer_use);
        assert_eq!(
            package_diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == 2339)
                .count(),
            1,
            "package should see both own fragments but not peer: {package_diagnostics:?}"
        );
        assert_eq!(
            peer_diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == 2339)
                .count(),
            1,
            "peer should not inherit package fragments: {peer_diagnostics:?}"
        );
    }
}
