use tsc_rs_scanner::Scanner;

fn main() {
    let src = r#"s.replace(/=/g, '').replace(/\+/g, '-').replace(/\//g, '_');"#;
    println!("SRC: {}", src);
    let tokens = Scanner::new(src).scan_all();
    for t in &tokens {
        let text = &src[t.span.start as usize..t.span.end as usize];
        println!(
            "  {:?} @{}..{} = {:?}",
            t.kind, t.span.start, t.span.end, text
        );
    }
}
