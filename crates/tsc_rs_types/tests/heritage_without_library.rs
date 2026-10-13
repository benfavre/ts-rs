use std::process::Command;

#[test]
fn heritage_uses_builtin_instances_without_library_files() {
    const CHILD: &str = "TSC_RS_HERITAGE_WITHOUT_LIBRARY_CHILD";
    if std::env::var_os(CHILD).is_none() {
        // Library discovery is cached process-wide. A child process isolates
        // this environment from tests which load the real declaration files.
        let directory = std::env::temp_dir().join(format!(
            "ts-rs-heritage-without-library-{}",
            std::process::id()
        ));
        std::fs::create_dir(&directory).unwrap();
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "heritage_uses_builtin_instances_without_library_files",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env("TSC_RS_TYPESCRIPT_LIB_DIR", &directory)
            .output()
            .unwrap();
        std::fs::remove_dir(&directory).unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let cases: &[(&str, &str, &[u32])] = &[
        (
            "case.ts",
            "class C1 extends Object {} class C2 extends Function {} class C3 extends String {} class C4 extends Boolean {} class C5 extends Number {} class C6 extends Date {} class C7 extends RegExp {} class C8 extends Error {} class C9 extends Array {} class C10 extends Array<number> {}",
            &[],
        ),
        (
            "case.d.ts",
            "export class C extends Object { static readonly p: unique symbol; [C.p](): void; }",
            &[],
        ),
        (
            "case.ts",
            "interface A {} function factory(a: any): {new(): Object} { return null; } class C extends factory(A) {}",
            &[2322, 2693],
        ),
        (
            "case.ts",
            "type Object = number; declare const Base: new () => Object; class C extends Base {}",
            &[2509],
        ),
    ];
    for &(name, source, expected) in cases {
        let file = tsc_rs_parser::parse(name, source);
        assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
        let symbols = tsc_rs_symbols::bind(&file);
        let options = tsc_rs_ast::CompilerOptions {
            target: Some(tsc_rs_ast::ScriptTarget::ES2015),
            strict: Some(true),
            ..Default::default()
        };
        for spans in [false, true] {
            let mut checker = tsc_rs_types::TypeChecker::new();
            if spans {
                checker.enable_expression_type_spans();
            }
            let result = checker.check_with_options(&file, &symbols, &options);
            let actual: Vec<_> = result.diagnostics.iter().map(|d| d.code).collect();
            assert_eq!(actual, expected, "{source}: {:?}", result.diagnostics);
        }
    }
}
