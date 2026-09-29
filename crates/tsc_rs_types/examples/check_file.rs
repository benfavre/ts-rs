//! Print the checker's diagnostics for one file: `check_file <path> [target]`.
fn main() {
    let path = std::env::args().nth(1).expect("path");
    let text = std::fs::read_to_string(&path).unwrap();
    let file = tsc_rs_parser::parse(&path, &text);
    let symbols = tsc_rs_symbols::bind(&file);
    let options = tsc_rs_ast::CompilerOptions::default();
    let out = tsc_rs_types::TypeChecker::new().check_with_options(&file, &symbols, &options);
    for d in out.diagnostics {
        let span = d.span.unwrap_or_default();
        let line = text[..span.start as usize].matches('\n').count() + 1;
        println!("{line}: TS{} {}", d.code, d.message);
    }
}
