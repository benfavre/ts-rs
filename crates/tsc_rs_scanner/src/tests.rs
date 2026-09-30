use crate::{Scanner, TokenKind, TsScanner};

/// Helper: scan all non-EOF tokens and return their kinds.
fn scan_kinds(source: &str) -> Vec<TokenKind> {
    let tokens = Scanner::new(source).scan_all();
    tokens
        .into_iter()
        .map(|t| t.kind)
        .filter(|k| *k != TokenKind::EndOfFile)
        .collect()
}

/// Helper: scan all tokens and return (kind, text) pairs.
fn scan_token_texts(source: &str) -> Vec<(TokenKind, String)> {
    let tokens = Scanner::new(source).scan_all();
    tokens
        .into_iter()
        .filter(|t| t.kind != TokenKind::EndOfFile)
        .map(|t| {
            let text = source[t.span.start as usize..t.span.end as usize].to_string();
            (t.kind, text)
        })
        .collect()
}

// =========================================================================
// Single character tokens
// =========================================================================

#[test]
fn single_char_tokens() {
    let kinds = scan_kinds("{ } ( ) [ ] ; , ~ @ #");
    assert_eq!(
        kinds,
        vec![
            TokenKind::OpenBrace,
            TokenKind::CloseBrace,
            TokenKind::OpenParen,
            TokenKind::CloseParen,
            TokenKind::OpenBracket,
            TokenKind::CloseBracket,
            TokenKind::Semicolon,
            TokenKind::Comma,
            TokenKind::Tilde,
            TokenKind::At,
            TokenKind::Hash,
        ]
    );
}

#[test]
fn dot_and_spread() {
    let kinds = scan_kinds(". ...");
    assert_eq!(kinds, vec![TokenKind::Dot, TokenKind::DotDotDot]);
}

#[test]
fn colon() {
    assert_eq!(scan_kinds(":"), vec![TokenKind::Colon]);
}

// =========================================================================
// Multi-character operators
// =========================================================================

#[test]
fn comparison_operators() {
    let kinds = scan_kinds("< > <= >= == != === !==");
    assert_eq!(
        kinds,
        vec![
            TokenKind::LessThan,
            TokenKind::GreaterThan,
            TokenKind::LessEqual,
            TokenKind::GreaterEqual,
            TokenKind::EqualsEquals,
            TokenKind::ExclEquals,
            TokenKind::EqualsEqualsEquals,
            TokenKind::ExclEqualsEquals,
        ]
    );
}

#[test]
fn shift_operators() {
    let kinds = scan_kinds("<< >> >>> <<= >>= >>>=");
    assert_eq!(
        kinds,
        vec![
            TokenKind::LessLess,
            TokenKind::GreaterGreater,
            TokenKind::GreaterGreaterGreater,
            TokenKind::LessLessEquals,
            TokenKind::GreaterGreaterEquals,
            TokenKind::GreaterGreaterGreaterEquals,
        ]
    );
}

#[test]
fn arithmetic_operators() {
    let kinds = scan_kinds("+ - * ** / % ++ --");
    assert_eq!(
        kinds,
        vec![
            TokenKind::Plus,
            TokenKind::Minus,
            TokenKind::Asterisk,
            TokenKind::AsteriskAsterisk,
            TokenKind::Slash,
            TokenKind::Percent,
            TokenKind::PlusPlus,
            TokenKind::MinusMinus,
        ]
    );
}

#[test]
fn assignment_operators() {
    let kinds = scan_kinds("= += -= *= **= /= %= &= |= ^= &&= ||= ??=");
    assert_eq!(
        kinds,
        vec![
            TokenKind::Equals,
            TokenKind::PlusEquals,
            TokenKind::MinusEquals,
            TokenKind::AsteriskEquals,
            TokenKind::AsteriskAsteriskEquals,
            TokenKind::SlashEquals,
            TokenKind::PercentEquals,
            TokenKind::AmpersandEquals,
            TokenKind::BarEquals,
            TokenKind::CaretEquals,
            TokenKind::AmpersandAmpersandEquals,
            TokenKind::BarBarEquals,
            TokenKind::QuestionQuestionEquals,
        ]
    );
}

#[test]
fn invalid_unicode_identifier_escape_splits_backslash_and_tail_identifier() {
    assert_eq!(
        scan_token_texts("var arg\\u003"),
        vec![
            (TokenKind::Var, "var".to_string()),
            (TokenKind::Identifier, "arg".to_string()),
            (TokenKind::Unknown, "\\".to_string()),
            (TokenKind::Identifier, "u003".to_string()),
        ]
    );
    assert_eq!(
        scan_token_texts("var arg\\uxxxx"),
        vec![
            (TokenKind::Var, "var".to_string()),
            (TokenKind::Identifier, "arg".to_string()),
            (TokenKind::Unknown, "\\".to_string()),
            (TokenKind::Identifier, "uxxxx".to_string()),
        ]
    );
    assert_eq!(
        scan_token_texts("var \\u0031a"),
        vec![
            (TokenKind::Var, "var".to_string()),
            (TokenKind::Unknown, "\\".to_string()),
            (TokenKind::Identifier, "u0031a".to_string()),
        ]
    );
}

#[test]
fn unicode_subscript_digit_does_not_extend_identifier() {
    assert_eq!(
        scan_token_texts("var a₁ = 1;"),
        vec![
            (TokenKind::Var, "var".to_string()),
            (TokenKind::Identifier, "a".to_string()),
            (TokenKind::Unknown, "₁".to_string()),
            (TokenKind::Equals, "=".to_string()),
            (TokenKind::NumericLiteral, "1".to_string()),
            (TokenKind::Semicolon, ";".to_string()),
        ]
    );
}

#[test]
fn logical_operators() {
    let kinds = scan_kinds("&& || ?? !");
    assert_eq!(
        kinds,
        vec![
            TokenKind::AmpersandAmpersand,
            TokenKind::BarBar,
            TokenKind::QuestionQuestion,
            TokenKind::Excl,
        ]
    );
}

#[test]
fn bitwise_operators() {
    let kinds = scan_kinds("& | ^ ~");
    assert_eq!(
        kinds,
        vec![
            TokenKind::Ampersand,
            TokenKind::Bar,
            TokenKind::Caret,
            TokenKind::Tilde,
        ]
    );
}

#[test]
fn question_variants() {
    let kinds = scan_kinds("? ?. ??");
    assert_eq!(
        kinds,
        vec![
            TokenKind::Question,
            TokenKind::QuestionDot,
            TokenKind::QuestionQuestion,
        ]
    );
}

#[test]
fn question_dot_not_number() {
    // `?.1` should be `?` `.1` (number), not `?.` `1`
    let kinds = scan_kinds("?.1");
    assert_eq!(kinds, vec![TokenKind::Question, TokenKind::NumericLiteral]);
}

#[test]
fn fat_arrow() {
    assert_eq!(scan_kinds("=>"), vec![TokenKind::FatArrow]);
}

// =========================================================================
// Keywords
// =========================================================================

#[test]
fn reserved_keywords() {
    let kinds = scan_kinds(
        "break case catch class const continue debugger default \
         delete do else enum export extends false finally for function \
         if import in instanceof let new null return super switch \
         this throw true try typeof var void while with yield",
    );
    assert_eq!(kinds.len(), 38);
    assert!(kinds.iter().all(|k| k.is_keyword()));
}

#[test]
fn contextual_keywords() {
    let kinds = scan_kinds(
        "as async await declare get set from of implements interface \
         module namespace type abstract override readonly keyof unique \
         infer is asserts require never unknown any number bigint \
         string boolean symbol undefined object satisfies using accessor out",
    );
    assert!(kinds.iter().all(|k| k.is_contextual_keyword()));
}

#[test]
fn identifier_not_keyword() {
    let kinds = scan_kinds("foo bar _private $dollar camelCase");
    assert!(kinds.iter().all(|k| *k == TokenKind::Identifier));
}

#[test]
fn keyword_initial_identifier_with_unicode_tail_stays_identifier() {
    let tokens = scan_token_texts("class classé café awaitλ");
    assert_eq!(
        tokens,
        vec![
            (TokenKind::Class, "class".to_string()),
            (TokenKind::Identifier, "classé".to_string()),
            (TokenKind::Identifier, "café".to_string()),
            (TokenKind::Identifier, "awaitλ".to_string()),
        ]
    );
}

// =========================================================================
// Numeric literals
// =========================================================================

#[test]
fn decimal_numbers() {
    let tokens = scan_token_texts("0 42 3.14 1e10 2.5e-3");
    assert_eq!(tokens.len(), 5);
    assert!(tokens.iter().all(|(k, _)| *k == TokenKind::NumericLiteral));
    assert_eq!(tokens[0].1, "0");
    assert_eq!(tokens[1].1, "42");
    assert_eq!(tokens[2].1, "3.14");
    assert_eq!(tokens[3].1, "1e10");
    assert_eq!(tokens[4].1, "2.5e-3");
}

#[test]
fn hex_numbers() {
    let tokens = scan_token_texts("0xFF 0X1A");
    assert!(tokens.iter().all(|(k, _)| *k == TokenKind::NumericLiteral));
}

#[test]
fn octal_numbers() {
    let tokens = scan_token_texts("0o77 0O10");
    assert!(tokens.iter().all(|(k, _)| *k == TokenKind::NumericLiteral));
}

#[test]
fn binary_numbers() {
    let tokens = scan_token_texts("0b1010 0B11");
    assert!(tokens.iter().all(|(k, _)| *k == TokenKind::NumericLiteral));
}

#[test]
fn bigint_literals() {
    let tokens = scan_token_texts("42n 0xFFn 0o77n 0b1010n");
    assert!(tokens.iter().all(|(k, _)| *k == TokenKind::BigIntLiteral));
}

#[test]
fn legacy_octal_bigint_suffix_is_separate() {
    assert_eq!(
        scan_token_texts("0123n 01_2"),
        vec![
            (TokenKind::NumericLiteral, "0123".to_string()),
            (TokenKind::Identifier, "n".to_string()),
            (TokenKind::NumericLiteral, "01".to_string()),
            (TokenKind::Identifier, "_2".to_string()),
        ]
    );
}

#[test]
fn numeric_separators() {
    let tokens = scan_token_texts("1_000_000 0xFF_FF 0b1010_0101");
    assert_eq!(tokens.len(), 3);
    assert_eq!(tokens[0].1, "1_000_000");
}

#[test]
fn number_starting_with_dot() {
    let tokens = scan_token_texts(".5 .123e4");
    assert!(tokens.iter().all(|(k, _)| *k == TokenKind::NumericLiteral));
    assert_eq!(tokens[0].1, ".5");
}

#[test]
fn bigint_starting_with_dot() {
    let tokens = scan_token_texts(".2n");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].0, TokenKind::BigIntLiteral);
    assert_eq!(tokens[0].1, ".2n");
}

// =========================================================================
// String literals
// =========================================================================

#[test]
fn single_quoted_string() {
    let kinds = scan_kinds("'hello'");
    assert_eq!(kinds, vec![TokenKind::StringLiteral]);
}

#[test]
fn double_quoted_string() {
    let kinds = scan_kinds("\"world\"");
    assert_eq!(kinds, vec![TokenKind::StringLiteral]);
}

#[test]
fn string_escape_sequences() {
    let mut s = TsScanner::new(r#"'hello\nworld'"#);
    s.scan();
    assert_eq!(s.token(), TokenKind::StringLiteral);
    assert_eq!(s.token_value(), "hello\nworld");
}

#[test]
fn string_hex_escape() {
    let mut s = TsScanner::new(r"'\x41'");
    s.scan();
    assert_eq!(s.token_value(), "A");
}

#[test]
fn string_unicode_escape() {
    let mut s = TsScanner::new(r"'\u0041'");
    s.scan();
    assert_eq!(s.token_value(), "A");
}

#[test]
fn string_unicode_braced_escape() {
    let mut s = TsScanner::new(r"'\u{1F600}'");
    s.scan();
    // U+1F600 is the grinning face emoji
    assert_eq!(s.token_value(), "\u{1F600}");
}

#[test]
fn cooked_value_does_not_leak_into_following_tokens() {
    let mut s = TsScanner::new(r#"'\x41' plain "raw""#);
    assert_eq!(s.scan(), TokenKind::StringLiteral);
    assert_eq!(s.token_value(), "A");
    assert_eq!(s.scan(), TokenKind::Identifier);
    assert_eq!(s.token_value(), "plain");
    assert_eq!(s.scan(), TokenKind::StringLiteral);
    assert_eq!(s.token_value(), "raw");
}

#[test]
fn unterminated_string_at_newline() {
    let kinds = scan_kinds("'hello\nworld'");
    // Should break at the newline
    assert_eq!(kinds[0], TokenKind::StringLiteral);
}

// =========================================================================
// Template literals
// =========================================================================

#[test]
fn no_substitution_template() {
    let kinds = scan_kinds("`hello world`");
    assert_eq!(kinds, vec![TokenKind::NoSubstitutionTemplate]);
}

#[test]
fn template_with_substitution() {
    let kinds = scan_kinds("`hello ${name}`");
    // The batch scanner pre-rescans `}` into TemplateTail/TemplateMiddle.
    assert_eq!(
        kinds,
        vec![
            TokenKind::TemplateHead,
            TokenKind::Identifier,
            TokenKind::TemplateTail
        ]
    );
}

#[test]
fn template_value() {
    let mut s = TsScanner::new("`hello`");
    s.scan();
    assert_eq!(s.token(), TokenKind::NoSubstitutionTemplate);
    assert_eq!(s.token_value(), "hello");
}

// =========================================================================
// Comments (skipped as trivia)
// =========================================================================

#[test]
fn single_line_comment_skipped() {
    let kinds = scan_kinds("a // comment\nb");
    assert_eq!(kinds, vec![TokenKind::Identifier, TokenKind::Identifier]);
}

#[test]
fn multi_line_comment_skipped() {
    let kinds = scan_kinds("a /* comment */ b");
    assert_eq!(kinds, vec![TokenKind::Identifier, TokenKind::Identifier]);
}

#[test]
fn multi_line_comment_with_newline_sets_line_break() {
    let mut s = TsScanner::new("a /* \n */ b");
    s.scan(); // a
    assert!(!s.has_preceding_line_break());
    s.scan(); // b
    assert!(s.has_preceding_line_break());
}

#[test]
fn conflict_markers_are_skipped_as_trivia() {
    let kinds = scan_kinds(
        "<<<<<<< HEAD\n\
         =======\n\
         >>>>>>> branch\n\
         foo",
    );
    assert_eq!(kinds, vec![TokenKind::Identifier]);
}

#[test]
fn diff3_conflict_marker_is_skipped_as_trivia() {
    let kinds = scan_kinds(
        "||||||| base\n\
         original\n\
         =======\n\
         incoming\n\
         >>>>>>> branch\n\
         foo",
    );
    assert_eq!(kinds, vec![TokenKind::Identifier]);
}

// =========================================================================
// Line break tracking
// =========================================================================

#[test]
fn line_break_after_newline() {
    let mut s = TsScanner::new("a\nb");
    s.scan(); // a
    assert!(!s.has_preceding_line_break());
    s.scan(); // b
    assert!(s.has_preceding_line_break());
}

#[test]
fn line_break_after_crlf() {
    let mut s = TsScanner::new("a\r\nb");
    s.scan();
    assert!(!s.has_preceding_line_break());
    s.scan();
    assert!(s.has_preceding_line_break());
}

#[test]
fn line_break_after_unicode_separator() {
    let mut s = TsScanner::new("a\u{2028}b");
    s.scan();
    assert!(!s.has_preceding_line_break());
    s.scan();
    assert!(s.has_preceding_line_break());
}

#[test]
fn no_line_break_on_same_line() {
    let mut s = TsScanner::new("a b");
    s.scan();
    s.scan();
    assert!(!s.has_preceding_line_break());
}

#[test]
fn unicode_whitespace_is_skipped() {
    let kinds = scan_kinds("a\u{00A0}b");
    assert_eq!(kinds, vec![TokenKind::Identifier, TokenKind::Identifier]);
}

// =========================================================================
// Re-scan methods
// =========================================================================

#[test]
fn rescan_slash_as_regex() {
    let mut s = TsScanner::new("/abc/gi");
    s.scan();
    assert_eq!(s.token(), TokenKind::Slash);
    let kind = s.re_scan_slash_token();
    assert_eq!(kind, TokenKind::RegExpLiteral);
    assert_eq!(s.token_value(), "/abc/gi");
}

#[test]
fn regex_flags_stop_before_unicode_trivia() {
    for whitespace in ['\u{00a0}', '\u{2028}', '\u{2029}', '\u{feff}'] {
        let source = format!("/a/g{whitespace}next");
        let mut scanner = TsScanner::new(&source);
        scanner.scan();
        assert_eq!(scanner.re_scan_slash_token(), TokenKind::RegExpLiteral);
        assert_eq!(scanner.token_value(), "/a/g");
        assert_eq!(scanner.scan(), TokenKind::Identifier);
        assert_eq!(scanner.token_value(), "next");
    }
}

#[test]
fn regex_flags_include_all_identifier_parts() {
    let mut scanner = TsScanner::new("/a/g1_$𝘨;");
    scanner.scan();
    assert_eq!(scanner.re_scan_slash_token(), TokenKind::RegExpLiteral);
    assert_eq!(scanner.token_value(), "/a/g1_$𝘨");
    assert_eq!(scanner.scan(), TokenKind::Semicolon);
}

#[test]
fn rescan_slash_equals_as_regex() {
    let mut s = TsScanner::new("/=abc/g");
    s.scan();
    assert_eq!(s.token(), TokenKind::SlashEquals);
    let kind = s.re_scan_slash_token();
    assert_eq!(kind, TokenKind::RegExpLiteral);
}

/// Regression: `.replace(/=/g, '')` was mis-tokenized because the
/// scanner bonds `/=` into a SlashEquals token and previously only
/// re-scanned bare `Slash` to a regex literal. When the SlashEquals
/// followed a regex-allowing token (OpenParen here) the original
/// version emitted a stray `/=` and the rest of the expression got
/// swallowed into a bogus RegExpLiteral — jose's base64url encode()
/// came out as `s.replace(/=/g); //g, '_');\n+/g, '-')...`.
#[test]
fn chained_replace_with_slash_equals_regex() {
    let kinds = scan_kinds(r#"s.replace(/=/g, '').replace(/\+/g, '-').replace(/\//g, '_');"#);
    assert_eq!(
        kinds,
        vec![
            TokenKind::Identifier, // s
            TokenKind::Dot,
            TokenKind::Identifier, // replace
            TokenKind::OpenParen,
            TokenKind::RegExpLiteral, // /=/g — previously mis-scanned as SlashEquals+junk
            TokenKind::Comma,
            TokenKind::StringLiteral, // ''
            TokenKind::CloseParen,
            TokenKind::Dot,
            TokenKind::Identifier, // replace
            TokenKind::OpenParen,
            TokenKind::RegExpLiteral, // /\+/g
            TokenKind::Comma,
            TokenKind::StringLiteral, // '-'
            TokenKind::CloseParen,
            TokenKind::Dot,
            TokenKind::Identifier, // replace
            TokenKind::OpenParen,
            TokenKind::RegExpLiteral, // /\//g
            TokenKind::Comma,
            TokenKind::StringLiteral, // '_'
            TokenKind::CloseParen,
            TokenKind::Semicolon,
        ]
    );
}

/// Bare `/=` in assignment-operator position keeps its SlashEquals
/// identity — the regex re-scan only kicks in after a token that can
/// actually precede a regex literal.
#[test]
fn slash_equals_in_assignment_sequence_is_not_regex() {
    let kinds = scan_kinds("= += /= %=");
    assert_eq!(
        kinds,
        vec![
            TokenKind::Equals,
            TokenKind::PlusEquals,
            TokenKind::SlashEquals,
            TokenKind::PercentEquals,
        ]
    );
}

#[test]
fn rescan_greater_token() {
    let mut s = TsScanner::new(">>=");
    s.scan();
    assert_eq!(s.token(), TokenKind::GreaterGreaterEquals);
    // If we re-scan from just `>`, we should get different results
    // The re-scan is for when parser needs to split the token
}

// =========================================================================
// Integration tests
// =========================================================================

#[test]
fn scan_variable_declaration() {
    let kinds = scan_kinds("const x: number = 42;");
    assert_eq!(
        kinds,
        vec![
            TokenKind::Const,
            TokenKind::Identifier,
            TokenKind::Colon,
            TokenKind::Number,
            TokenKind::Equals,
            TokenKind::NumericLiteral,
            TokenKind::Semicolon,
        ]
    );
}

#[test]
fn scan_function_declaration() {
    let kinds = scan_kinds("function add(a: number, b: number): number { return a + b; }");
    assert_eq!(kinds[0], TokenKind::Function);
    assert_eq!(kinds[1], TokenKind::Identifier); // add
    assert_eq!(kinds[2], TokenKind::OpenParen);
    assert!(kinds.contains(&TokenKind::Return));
    assert_eq!(*kinds.last().unwrap(), TokenKind::CloseBrace);
}

#[test]
fn scan_arrow_function() {
    let kinds = scan_kinds("(x) => x + 1");
    assert!(kinds.contains(&TokenKind::FatArrow));
}

#[test]
fn scan_class() {
    let kinds = scan_kinds("class Foo extends Bar { constructor() {} }");
    assert_eq!(kinds[0], TokenKind::Class);
    assert_eq!(kinds[1], TokenKind::Identifier);
    assert_eq!(kinds[2], TokenKind::Extends);
}

#[test]
fn scan_interface() {
    let kinds = scan_kinds("interface Shape { area(): number; }");
    assert_eq!(kinds[0], TokenKind::Interface);
}

#[test]
fn scan_generic_types() {
    let kinds = scan_kinds("Array<string>");
    assert_eq!(
        kinds,
        vec![
            TokenKind::Identifier,
            TokenKind::LessThan,
            TokenKind::String,
            TokenKind::GreaterThan,
        ]
    );
}

#[test]
fn scan_optional_chaining_and_nullish() {
    let kinds = scan_kinds("a?.b ?? c");
    assert_eq!(
        kinds,
        vec![
            TokenKind::Identifier,
            TokenKind::QuestionDot,
            TokenKind::Identifier,
            TokenKind::QuestionQuestion,
            TokenKind::Identifier,
        ]
    );
}

#[test]
fn scan_destructuring() {
    let kinds = scan_kinds("const { a, b: c, ...rest } = obj;");
    assert!(kinds.contains(&TokenKind::DotDotDot));
    assert!(kinds.contains(&TokenKind::OpenBrace));
    assert!(kinds.contains(&TokenKind::CloseBrace));
}

#[test]
fn scan_empty_string() {
    let kinds = scan_kinds("");
    assert!(kinds.is_empty());
}

#[test]
fn scan_only_whitespace() {
    let kinds = scan_kinds("   \t\n\r\n  ");
    assert!(kinds.is_empty());
}

#[test]
fn scan_only_comments() {
    let kinds = scan_kinds("// line comment\n/* block */");
    assert!(kinds.is_empty());
}

#[test]
fn token_positions_are_correct() {
    let tokens = Scanner::new("let x = 5;").scan_all();
    // "let" at 0..3
    assert_eq!(tokens[0].span.start, 0);
    assert_eq!(tokens[0].span.end, 3);
    assert_eq!(tokens[0].kind, TokenKind::Let);
    // "x" at 4..5
    assert_eq!(tokens[1].span.start, 4);
    assert_eq!(tokens[1].span.end, 5);
    // "=" at 6..7
    assert_eq!(tokens[2].span.start, 6);
    assert_eq!(tokens[2].span.end, 7);
    // "5" at 8..9
    assert_eq!(tokens[3].span.start, 8);
    assert_eq!(tokens[3].span.end, 9);
    // ";" at 9..10
    assert_eq!(tokens[4].span.start, 9);
    assert_eq!(tokens[4].span.end, 10);
}

#[test]
fn shebang_is_skipped() {
    let kinds = scan_kinds("#!/usr/bin/env node\nconsole.log('hi')");
    assert_eq!(kinds[0], TokenKind::Identifier); // console
}

#[test]
fn shebang_after_first_line_recovers_as_bang_expression() {
    let tokens = scan_token_texts("let x = 1;\n#!/usr/bin/env node");
    assert!(
        tokens.iter().all(|(kind, _)| *kind != TokenKind::Hash),
        "non-leading shebang recovery should drop `#`: {tokens:?}"
    );
    assert_eq!(tokens[5], (TokenKind::Excl, "!".to_string()));
}

// =========================================================================
// TokenKind helper methods
// =========================================================================

#[test]
fn is_identifier_name_covers_contiguous_block() {
    assert!(!TokenKind::RegExpLiteral.is_identifier_name());
    assert!(TokenKind::Identifier.is_identifier_name());
    assert!(TokenKind::Break.is_identifier_name());
    assert!(TokenKind::Out.is_identifier_name());
    assert!(!TokenKind::OpenBrace.is_identifier_name());
}

#[test]
fn is_assignment_covers_all() {
    let assignments = vec![
        TokenKind::Equals,
        TokenKind::PlusEquals,
        TokenKind::MinusEquals,
        TokenKind::AsteriskEquals,
        TokenKind::AsteriskAsteriskEquals,
        TokenKind::SlashEquals,
        TokenKind::PercentEquals,
        TokenKind::LessLessEquals,
        TokenKind::GreaterGreaterEquals,
        TokenKind::GreaterGreaterGreaterEquals,
        TokenKind::AmpersandEquals,
        TokenKind::BarEquals,
        TokenKind::CaretEquals,
        TokenKind::AmpersandAmpersandEquals,
        TokenKind::BarBarEquals,
        TokenKind::QuestionQuestionEquals,
    ];
    for kind in &assignments {
        assert!(kind.is_assignment(), "{:?} should be assignment", kind);
    }
    assert!(!TokenKind::Plus.is_assignment());
    assert!(!TokenKind::Identifier.is_assignment());
}

#[test]
fn is_literal_covers_all() {
    assert!(TokenKind::NumericLiteral.is_literal());
    assert!(TokenKind::BigIntLiteral.is_literal());
    assert!(TokenKind::StringLiteral.is_literal());
    assert!(TokenKind::RegExpLiteral.is_literal());
    assert!(TokenKind::NoSubstitutionTemplate.is_literal());
    assert!(!TokenKind::Identifier.is_literal());
}

#[test]
fn is_trivia_covers_all() {
    assert!(TokenKind::WhitespaceTrivia.is_trivia());
    assert!(TokenKind::NewLineTrivia.is_trivia());
    assert!(TokenKind::SingleLineCommentTrivia.is_trivia());
    assert!(TokenKind::MultiLineCommentTrivia.is_trivia());
    assert!(TokenKind::ShebangTrivia.is_trivia());
    assert!(TokenKind::ConflictMarkerTrivia.is_trivia());
    assert!(!TokenKind::Identifier.is_trivia());
}

// --- SWAR identifier classifier: differential test vs the scalar predicate ---
#[test]
fn swar_ident_classifier_matches_scalar_for_every_byte_in_every_lane() {
    use crate::char_utils::{first_non_identifier_continue, is_identifier_continue};
    for lane in 0..8usize {
        for v in 0u16..256 {
            let v = v as u8;
            // All-identifier chunk with one byte under test at `lane`.
            let mut chunk = [b'a'; 8];
            chunk[lane] = v;

            let got = first_non_identifier_continue(chunk);

            // Scalar ground truth: a non-ASCII byte anywhere forces Some(0)
            // (caller must fall back to UTF-8 decoding).
            let expected = if chunk.iter().any(|&b| b >= 0x80) {
                Some(0)
            } else {
                chunk
                    .iter()
                    .position(|&b| !is_identifier_continue(b))
                    .map(|i| i as u32)
            };
            assert_eq!(got, expected, "lane={lane} byte=0x{v:02x} chunk={chunk:?}");
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[test]
fn sse_ident_classifier_matches_scalar_for_every_byte_in_every_lane() {
    use crate::char_utils::{first_non_identifier_continue_16, is_identifier_continue};
    for lane in 0..16usize {
        for v in 0u16..256 {
            let v = v as u8;
            let mut chunk = [b'a'; 16];
            chunk[lane] = v;

            // SAFETY: `chunk` contains 16 readable bytes.
            let got = unsafe { first_non_identifier_continue_16(chunk.as_ptr()) };
            let expected = chunk
                .iter()
                .position(|&b| b >= 0x80 || !is_identifier_continue(b))
                .map(|i| i as u32);
            assert_eq!(got, expected, "lane={lane} byte=0x{v:02x} chunk={chunk:?}");
        }
    }
}

#[test]
fn swar_ident_classifier_matches_scalar_for_adjacent_ascii_pairs() {
    use crate::char_utils::{first_non_identifier_continue, is_identifier_continue};
    // A single varied byte cannot expose carries or borrows between lanes.
    // In particular, '$%' and '_^' must stop at their operator byte.
    for fill in [b'a', b'$', b'_'] {
        for lane in 0..7 {
            for first in 0u8..128 {
                for second in 0u8..128 {
                    let mut chunk = [fill; 8];
                    chunk[lane] = first;
                    chunk[lane + 1] = second;
                    let expected = chunk.iter().position(|&b| !is_identifier_continue(b));
                    assert_eq!(
                        first_non_identifier_continue(chunk),
                        expected.map(|position| position as u32),
                        "chunk={chunk:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn identifier_dollar_and_underscore_do_not_consume_following_operators() {
    for prefix in ["a", "longIdentifierPrefix"] {
        for (suffix, operator, kind) in
            [("$", "%", TokenKind::Percent), ("_", "^", TokenKind::Caret)]
        {
            // Padding also exercises the wide path on each host architecture.
            let name = format!("{prefix}{suffix}");
            let source = format!("{name}{operator}rhs; trailingIdentifier");
            let tokens = scan_token_texts(&source);
            assert_eq!(tokens[0], (TokenKind::Identifier, name));
            assert_eq!(tokens[1], (kind, operator.to_string()));
            assert_eq!(tokens[2], (TokenKind::Identifier, "rhs".into()));
        }
    }
}

// --- keyword_from_str: differential test against the pre-u64-packing table ---
#[test]
fn keyword_lookup_matches_reference_table() {
    use crate::token_kind::TokenKind;
    // Every (keyword -> TokenKind) pair extracted from the original `match s` table.
    const REFERENCE: &[(&str, TokenKind)] = &[
        ("break", TokenKind::Break),
        ("case", TokenKind::Case),
        ("catch", TokenKind::Catch),
        ("class", TokenKind::Class),
        ("const", TokenKind::Const),
        ("continue", TokenKind::Continue),
        ("debugger", TokenKind::Debugger),
        ("default", TokenKind::Default),
        ("delete", TokenKind::Delete),
        ("do", TokenKind::Do),
        ("else", TokenKind::Else),
        ("enum", TokenKind::Enum),
        ("export", TokenKind::Export),
        ("extends", TokenKind::Extends),
        ("false", TokenKind::False),
        ("finally", TokenKind::Finally),
        ("for", TokenKind::For),
        ("function", TokenKind::Function),
        ("if", TokenKind::If),
        ("import", TokenKind::Import),
        ("in", TokenKind::In),
        ("instanceof", TokenKind::InstanceOf),
        ("let", TokenKind::Let),
        ("new", TokenKind::New),
        ("null", TokenKind::Null),
        ("return", TokenKind::Return),
        ("super", TokenKind::Super),
        ("switch", TokenKind::Switch),
        ("this", TokenKind::This),
        ("throw", TokenKind::Throw),
        ("true", TokenKind::True),
        ("try", TokenKind::Try),
        ("typeof", TokenKind::TypeOf),
        ("var", TokenKind::Var),
        ("void", TokenKind::Void),
        ("while", TokenKind::While),
        ("with", TokenKind::With),
        ("yield", TokenKind::Yield),
        ("as", TokenKind::As),
        ("async", TokenKind::Async),
        ("await", TokenKind::Await),
        ("constructor", TokenKind::Constructor),
        ("declare", TokenKind::Declare),
        ("get", TokenKind::Get),
        ("set", TokenKind::Set),
        ("from", TokenKind::From),
        ("of", TokenKind::Of),
        ("implements", TokenKind::Implements),
        ("interface", TokenKind::Interface),
        ("module", TokenKind::Module),
        ("namespace", TokenKind::Namespace),
        ("package", TokenKind::Package),
        ("private", TokenKind::Private),
        ("protected", TokenKind::Protected),
        ("public", TokenKind::Public),
        ("static", TokenKind::Static),
        ("type", TokenKind::Type),
        ("global", TokenKind::Global),
        ("abstract", TokenKind::Abstract),
        ("override", TokenKind::Override),
        ("readonly", TokenKind::Readonly),
        ("keyof", TokenKind::Keyof),
        ("unique", TokenKind::Unique),
        ("infer", TokenKind::Infer),
        ("is", TokenKind::Is),
        ("asserts", TokenKind::Asserts),
        ("assert", TokenKind::Assert),
        ("require", TokenKind::Require),
        ("never", TokenKind::Never),
        ("unknown", TokenKind::UnknownKeyword),
        ("any", TokenKind::Any),
        ("number", TokenKind::Number),
        ("bigint", TokenKind::BigInt),
        ("string", TokenKind::String),
        ("boolean", TokenKind::Boolean),
        ("symbol", TokenKind::Symbol),
        ("undefined", TokenKind::Undefined),
        ("object", TokenKind::Object),
        ("intrinsic", TokenKind::Intrinsic),
        ("satisfies", TokenKind::Satisfies),
        ("using", TokenKind::Using),
        ("accessor", TokenKind::Accessor),
        ("out", TokenKind::Out),
    ];
    for (kw, expect) in REFERENCE {
        assert_eq!(
            expect.fixed_text(),
            Some(*kw),
            "keyword spelling {expect:?}"
        );
        assert_eq!(
            TokenKind::keyword_from_str(kw),
            *expect,
            "keyword {kw:?} must still map to {expect:?}"
        );
    }
    // Non-keywords must fall through to Identifier, including near-misses:
    // wrong case, too long, too short, keyword-prefixed, and non-ASCII.
    for s in [
        "Break",
        "BREAK",
        "breaks",
        "brea",
        "b",
        "x",
        "constructor_",
        "constructorx",
        "_const",
        "const_",
        "$const",
        "cONST",
        "instanceofx",
        "implement",
        "föö",
        "a",
        "",
        "verylongidentifiername",
        "type_",
        "typeo",
        "typeoff",
    ] {
        assert_eq!(
            TokenKind::keyword_from_str(s),
            TokenKind::Identifier,
            "{s:?} must not be treated as a keyword"
        );
    }
}

#[test]
fn fixed_punctuation_spellings_and_nonfixed_tokens() {
    for spelling in "{ } ( ) [ ] . ... ; , < > <= >= == != === !== => + - * ** / % ++ -- << >> >>> & | ^ ! ~ && || ? ?. ?? : = += -= *= **= /= %= <<= >>= >>>= &= |= ^= &&= ||= ??= @ #".split_whitespace() {
        let mut scanner = TsScanner::new(spelling);
        let kind = scanner.scan();
        assert_eq!(kind.fixed_text(), Some(spelling), "{kind:?}");
    }
    assert_eq!(TokenKind::Backtick.fixed_text(), Some("`"));
    for kind in [
        TokenKind::Identifier,
        TokenKind::StringLiteral,
        TokenKind::NumericLiteral,
        TokenKind::EndOfFile,
        TokenKind::WhitespaceTrivia,
    ] {
        assert_eq!(kind.fixed_text(), None);
    }
}

#[test]
fn jsx_identifier_rescan_keeps_contiguous_hyphens_digits_and_unicode() {
    for (source, raw, cooked, next) in [
        ("a--1/>", "a--1", "a--1", TokenKind::Slash),
        ("A-B=", "A-B", "A-B", TokenKind::Equals),
        ("a-1.2", "a-1", "a-1", TokenKind::NumericLiteral),
        ("ns-:tag", "ns-", "ns-", TokenKind::Colon),
        ("a-\u{0301}/>", "a-\u{0301}", "a-\u{0301}", TokenKind::Slash),
        (
            r"\u0061-\u0032/>",
            r"\u0061-\u0032",
            "a-2",
            TokenKind::Slash,
        ),
        (r"a-\u{0062}-/>", r"a-\u{0062}-", "a-b-", TokenKind::Slash),
    ] {
        let mut scanner = TsScanner::new(source);
        scanner.scan();
        assert_eq!(
            scanner.scan_jsx_identifier(),
            TokenKind::Identifier,
            "{source}"
        );
        assert_eq!(scanner.token_text(), raw, "{source}");
        assert_eq!(scanner.token_value(), cooked, "{source}");
        assert_eq!(scanner.text_pos(), raw.len());
        assert_eq!(scanner.scan(), next, "{source}");
    }
}

#[test]
fn jsx_identifier_rescan_stops_at_trivia_and_invalid_escapes() {
    for (source, name) in [
        ("a -b", "a"),
        ("a/*comment*/-b", "a"),
        ("a- b", "a-"),
        (r"a-\u002d", "a-"),
        (r"a-\uXYZ", "a-"),
    ] {
        let mut scanner = TsScanner::new(source);
        scanner.scan();
        scanner.scan_jsx_identifier();
        assert_eq!(scanner.token_text(), name, "{source}");
        assert_eq!(scanner.token_value(), name, "{source}");
    }
    let mut scanner = TsScanner::new("32-name");
    assert_eq!(scanner.scan(), TokenKind::NumericLiteral);
    assert_eq!(scanner.scan_jsx_identifier(), TokenKind::NumericLiteral);
    assert_eq!(scanner.token_text(), "32");
}

#[test]
fn greater_than_before_an_arrow_leaves_the_arrow_whole() {
    // `(): Promise<void>=> x`: the type's `>` must not swallow the arrow's `=`.
    assert_eq!(
        scan_kinds(">=>"),
        vec![TokenKind::GreaterThan, TokenKind::FatArrow]
    );
    assert_eq!(
        scan_kinds(">>=>"),
        vec![TokenKind::GreaterGreater, TokenKind::FatArrow]
    );
    assert_eq!(
        scan_kinds(">>>=>"),
        vec![TokenKind::GreaterGreaterGreater, TokenKind::FatArrow]
    );
    // Real operators are unchanged.
    assert_eq!(scan_kinds(">="), vec![TokenKind::GreaterEqual]);
    assert_eq!(scan_kinds(">>="), vec![TokenKind::GreaterGreaterEquals]);
    assert_eq!(
        scan_kinds(">>>="),
        vec![TokenKind::GreaterGreaterGreaterEquals]
    );
    assert_eq!(
        scan_kinds(">= >"),
        vec![TokenKind::GreaterEqual, TokenKind::GreaterThan]
    );
}
