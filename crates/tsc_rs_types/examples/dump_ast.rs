fn main() {
    let path = std::env::args().nth(1).expect("path");
    let text = std::fs::read_to_string(&path).unwrap();
    let file = tsc_rs_parser::parse(&path, &text);
    println!("{:#?}", file.statements);
}
