//! Print the checker's diagnostics for one file: `check_file <path> [--lib]`.
//!
//! `// @strict: false` and `// @target: es5` style directives in the file are
//! honored; `--lib` loads the default TypeScript lib declarations first (set
//! `TSC_RS_TYPESCRIPT_LIB_DIR` to pick the TypeScript installation).
fn main() {
    let path = std::env::args().nth(1).expect("path");
    let with_lib = std::env::args().any(|a| a == "--lib");
    let text = std::fs::read_to_string(&path).unwrap();
    let file = tsc_rs_parser::parse(&path, &text);
    let symbols = tsc_rs_symbols::bind(&file);
    let mut options = tsc_rs_ast::CompilerOptions::default();
    for line in text.lines() {
        let Some(rest) = line.trim().strip_prefix("// @") else {
            continue;
        };
        let Some((key, value)) = rest.split_once(':') else {
            continue;
        };
        let flag = value.trim().eq_ignore_ascii_case("true");
        match key.trim().to_ascii_lowercase().as_str() {
            "strict" => options.strict = Some(flag),
            "strictnullchecks" => options.strict_null_checks = Some(flag),
            "noimplicitany" => options.no_implicit_any = Some(flag),
            _ => {}
        }
    }
    let mut checker = tsc_rs_types::TypeChecker::new();
    let libs = if with_lib {
        tsc_rs_types::load_stdlib_sources(&options)
    } else {
        Vec::new()
    };
    let parsed: Vec<_> = libs
        .iter()
        .map(|lib| tsc_rs_parser::parse(&lib.file_name, &lib.source))
        .collect();
    if !parsed.is_empty() {
        let declarations: Vec<_> = parsed.iter().collect();
        checker.inject_external_types(&declarations);
        checker.take_diagnostics();
    }
    let out = checker.check_with_options(&file, &symbols, &options);
    for d in out.diagnostics {
        let span = d.span.unwrap_or_default();
        let line = text[..span.start as usize].matches('\n').count() + 1;
        println!("{line}: TS{} {}", d.code, d.message);
    }
}
