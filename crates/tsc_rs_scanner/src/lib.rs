//! Complete TypeScript scanner/tokenizer.
//!
//! Provides both a streaming API (`TsScanner`) and a batch API (`Scanner`)
//! for tokenizing TypeScript/JavaScript source text.

pub mod char_utils;
pub mod scanner;
pub mod token_kind;

#[cfg(test)]
mod tests;

pub use scanner::{RawComment, TsScanner};
pub use token_kind::TokenKind;

use tsc_rs_ast::Span;

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
    /// Whether trivia containing a line terminator (including a multi-line
    /// comment that spans lines) precedes this token. The scanner already knows
    /// this while skipping trivia; recording it here lets the parser answer ASI
    /// questions with a field read instead of re-scanning the source text.
    /// Fits in `Token`'s existing padding — `Token` stays 12 bytes.
    pub preceded_by_line_break: bool,
}

const _: () = assert!(std::mem::size_of::<Token>() == 12);

/// Batch scanner that collects all tokens at once.
/// Used by the parser via `Scanner::new(source).scan_all()`.
pub struct Scanner<'a> {
    inner: TsScanner<'a>,
}

impl<'a> Scanner<'a> {
    pub fn new(source: &'a str) -> Self {
        Self {
            inner: TsScanner::new(source),
        }
    }

    pub fn scan_all(mut self) -> Vec<Token> {
        self.scan_all_inner()
    }

    /// Scan all tokens and also return every comment found in the source.
    pub fn scan_all_with_comments(mut self) -> (Vec<Token>, Vec<RawComment>) {
        let tokens = self.scan_all_inner();
        let comments = std::mem::take(&mut self.inner.comments);
        (tokens, comments)
    }

    /// [`Self::scan_all_with_comments`] plus the start offsets of the merge
    /// conflict markers skipped as trivia (TS1185).
    pub fn scan_all_with_comments_and_conflict_markers(
        mut self,
    ) -> (Vec<Token>, Vec<RawComment>, Vec<u32>) {
        let tokens = self.scan_all_inner();
        let comments = std::mem::take(&mut self.inner.comments);
        let markers = std::mem::take(&mut self.inner.conflict_markers);
        (tokens, comments, markers)
    }

    /// Shared scanning logic used by both `scan_all` and `scan_all_with_comments`.
    fn scan_all_inner(&mut self) -> Vec<Token> {
        // Pre-allocate: typical TS/JS has ~1 token per 5 bytes of source.
        let mut tokens = Vec::with_capacity(self.inner.source_len() / 5);
        let mut template_brace_depth: Vec<u32> = Vec::new();
        // Track previous non-trivia token for regex vs division disambiguation.
        let mut prev_non_trivia_kind = TokenKind::EndOfFile;
        let mut scan_pos = 0;
        let scan_source = self.inner.source_ptr();
        let scan_end = self.inner.source_len();

        loop {
            // SAFETY: both frozen values came from this scanner and its source
            // allocation is borrowed for the scanner's full lifetime.
            let scanned = unsafe { self.inner.scan_batch_from(scan_pos, scan_source, scan_end) };
            scan_pos = scanned.pos;
            let kind = scanned.kind;
            // Trivia before this token — captured right after `scan()`, before any
            // rescan (rescans don't re-skip trivia, so the flag stays valid).
            let nl = self.inner.has_preceding_line_break();

            let final_kind = match kind {
                TokenKind::TemplateHead => {
                    template_brace_depth.push(0);
                    kind
                }
                TokenKind::OpenBrace if !template_brace_depth.is_empty() => {
                    if let Some(depth) = template_brace_depth.last_mut() {
                        *depth += 1;
                    }
                    kind
                }
                TokenKind::CloseBrace if !template_brace_depth.is_empty() => {
                    let depth = template_brace_depth.last_mut().unwrap();
                    if *depth > 0 {
                        *depth -= 1;
                        kind
                    } else {
                        self.inner.publish_batch_token(scan_pos, kind);
                        let rescanned = self.inner.re_scan_template_token();
                        scan_pos = self.inner.text_pos();
                        if rescanned == TokenKind::TemplateTail {
                            template_brace_depth.pop();
                        }
                        rescanned
                    }
                }
                _ => {
                    // Regex vs division disambiguation: when we get Slash or
                    // SlashEquals and the previous non-trivia token is one that
                    // can precede a regex literal, rescan as regex.  This
                    // prevents the scanner from later mis-scanning `/*` inside
                    // a regex body as a block comment.
                    // Also rescan `SlashEquals` — the scanner bonds `/=` into
                    // the compound-assignment token, but in regex position
                    // (e.g. `.replace(/=/g, "")`) that `/=` is the opening `/`
                    // followed by `=` as the first char of the pattern. The
                    // `re_scan_slash_token` helper already accepts both kinds.
                    if (kind == TokenKind::Slash
                        || kind == TokenKind::SlashEquals)
                            && token_precedes_regex(prev_non_trivia_kind)
                            // Don't rescan as regex if next char is `)` or `>`
                            // — these can't start valid regex content.
                            // Fixes `(/)` in JSX text being misscanned as regex.
                            && {
                                let next_pos = self.inner.text_pos();
                                next_pos >= self.inner.source_len()
                                    || !matches!(
                                        self.inner.source_byte_at(next_pos),
                                        b')' | b'>' | b']'
                                    )
                            }
                    {
                        self.inner.publish_batch_token(scan_pos, kind);
                        let rescanned = self.inner.re_scan_slash_token();
                        scan_pos = self.inner.text_pos();
                        // Drop any comments the scanner recorded inside the regex
                        // body (a `/*` in a pattern is not a comment). Comments are
                        // appended in source order and nothing past `regex_end` has
                        // been scanned yet, so the ones to drop are always a suffix
                        // — pop them instead of `retain`-ing over the whole list,
                        // which was O(comments) per regex (quadratic on big files).
                        if rescanned == TokenKind::RegExpLiteral {
                            let regex_start = self.inner.token_pos() as u32;
                            let regex_end = self.inner.text_pos() as u32;
                            while let Some(last) = self.inner.comments.last() {
                                if last.end > regex_start && last.pos < regex_end {
                                    self.inner.comments.pop();
                                } else {
                                    break;
                                }
                            }
                        }
                        rescanned
                    } else {
                        kind
                    }
                }
            };
            debug_assert!(!final_kind.is_trivia());
            prev_non_trivia_kind = final_kind;
            tokens.push(Token {
                kind: final_kind,
                span: Span::new(self.inner.token_pos() as u32, scan_pos as u32),
                preceded_by_line_break: nl,
            });
            if final_kind == TokenKind::EndOfFile {
                break;
            }
        }
        tokens
    }

    /// Re-scan the last `}` token as a template middle/tail.
    /// Requires that the last token pushed was `CloseBrace`.
    pub fn rescan_template_token(&mut self) -> Token {
        let kind = self.inner.re_scan_template_token();
        Token {
            kind,
            span: Span::new(self.inner.token_pos() as u32, self.inner.text_pos() as u32),
            preceded_by_line_break: self.inner.has_preceding_line_break(),
        }
    }
}

/// Returns `true` if `/` following a token of this kind should be treated as
/// the start of a regular expression literal rather than division.
fn token_precedes_regex(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Equals
            | TokenKind::PlusEquals
            | TokenKind::MinusEquals
            | TokenKind::AsteriskEquals
            | TokenKind::AsteriskAsteriskEquals
            | TokenKind::SlashEquals
            | TokenKind::PercentEquals
            | TokenKind::LessLessEquals
            | TokenKind::GreaterGreaterEquals
            | TokenKind::GreaterGreaterGreaterEquals
            | TokenKind::AmpersandEquals
            | TokenKind::BarEquals
            | TokenKind::CaretEquals
            | TokenKind::AmpersandAmpersandEquals
            | TokenKind::BarBarEquals
            | TokenKind::QuestionQuestionEquals
            | TokenKind::OpenParen
            | TokenKind::OpenBracket
            | TokenKind::OpenBrace
            | TokenKind::Excl
            | TokenKind::Tilde
            | TokenKind::Ampersand
            | TokenKind::Bar
            | TokenKind::Caret
            | TokenKind::AmpersandAmpersand
            | TokenKind::BarBar
            | TokenKind::QuestionQuestion
            | TokenKind::Plus
            | TokenKind::Minus
            | TokenKind::Asterisk
            | TokenKind::AsteriskAsterisk
            | TokenKind::Percent
            // NOTE: LessThan and GreaterThan are intentionally omitted.
            // `<` appears in JSX closing tags (`</div>`, `</>`) where `/`
            // is NOT a regex start.  The parser handles the rare case of
            // `a < /regex/` via its own re-scan logic.
            | TokenKind::LessEqual
            | TokenKind::GreaterEqual
            | TokenKind::EqualsEquals
            | TokenKind::ExclEquals
            | TokenKind::EqualsEqualsEquals
            | TokenKind::ExclEqualsEquals
            | TokenKind::LessLess
            // NOTE: GreaterGreater and GreaterGreaterGreater are
            // intentionally omitted because they can appear in JSX/TSX
            // contexts where the following `/` is part of a self-closing
            // tag (e.g. `<div<T>>/>` produces `GreaterGreater` then `/`).
            | TokenKind::FatArrow
            | TokenKind::Comma
            | TokenKind::Semicolon
            | TokenKind::Colon
            | TokenKind::Question
            | TokenKind::Return
            | TokenKind::Case
            | TokenKind::TypeOf
            | TokenKind::Void
            | TokenKind::Delete
            | TokenKind::Throw
            | TokenKind::New
            | TokenKind::In
            | TokenKind::InstanceOf
            | TokenKind::Await
            | TokenKind::Yield
            | TokenKind::Extends
            | TokenKind::Do
            | TokenKind::Else
            | TokenKind::If
            | TokenKind::While
            | TokenKind::For
            | TokenKind::With
            | TokenKind::Switch
            | TokenKind::Export
            | TokenKind::Default
            | TokenKind::EndOfFile
            | TokenKind::DotDotDot
    )
}
