//! Complete TypeScript recursive-descent parser.
//!
//! Produces a full AST from a token stream. Handles all TypeScript syntax
//! including type annotations, generics, decorators, JSX, and more.

mod class_modifiers;
mod index_signatures;
mod parameter_defaults;
mod parse_jsx;
mod parse_types;

use tsc_rs_ast::*;
use tsc_rs_scanner::{Scanner, Token, TokenKind, TsScanner};

#[cfg(test)]
mod tests;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn parse(file_name: &str, source: &str) -> SourceFile {
    let is_tsx = file_name.ends_with(".tsx") || file_name.ends_with(".jsx");
    parse_with_jsx(file_name, source, is_tsx)
}

/// Whether parser diagnostics contain a syntactic recovery error. Grammar
/// errors reported alongside an otherwise valid AST do not block later checks.
/// Diagnostics this parser reports that tsc reports from its checker's
/// GRAMMAR checks (`grammarErrorOnNode` and friends), which tsc drops in a
/// file that has real parse errors.
pub fn is_grammar_diagnostic(code: u32) -> bool {
    matches!(
        code,
        1163 | 1009
            | 1014
            | 1015
            | 1016
            | 1017
            | 1018
            | 1019
            | 1020
            | 1022
            | 1024
            | 1025
            | 1028
            | 1031
            | 1039
            | 1040
            | 1042
            | 1046
            | 1047
            | 1048
            | 1089
            | 1092
            | 1093
            | 1096
            | 1098
            | 1099
            | 1206
            | 1242
            | 1246
            | 1247
            | 1248
            | 1275
    )
}

pub fn has_syntax_errors(diagnostics: &[Diagnostic]) -> bool {
    diagnostics.iter().any(|diagnostic| {
        matches!(
            diagnostic.code,
            1002 | 1003
                | 1005
                | 1010
                | 1109
                | 1110
                | 1127
                | 1128
                | 1131
                | 1134
                | 1135
                | 1136
                | 1146
                | 1160
                | 1161
                | 1434
                | 1472
        )
    })
}

/// Parse with explicit JSX mode control. When `jsx_enabled` is true,
/// the parser recognizes JSX syntax (angle-bracket elements, fragments, etc.)
/// regardless of file extension.
pub fn parse_with_jsx(file_name: &str, source: &str, jsx_enabled: bool) -> SourceFile {
    let scanner = Scanner::new(source);
    let (tokens, raw_comments, conflict_markers) =
        scanner.scan_all_with_comments_and_conflict_markers();
    let lower_file_name = file_name.to_ascii_lowercase();
    let is_declaration_file = lower_file_name.ends_with(".d.ts")
        || lower_file_name.ends_with(".d.tsx")
        || lower_file_name.ends_with(".d.mts")
        || lower_file_name.ends_with(".d.cts");
    let is_js_file = lower_file_name.ends_with(".js")
        || lower_file_name.ends_with(".jsx")
        || lower_file_name.ends_with(".mjs")
        || lower_file_name.ends_with(".cjs");
    let mut p = Parser::new(source, tokens, jsx_enabled, is_declaration_file, is_js_file);
    let statements = p.parse_source_elements();
    let suppress_typescript_ts1206 = p.suppress_typescript_ts1206;
    let mut diagnostics = p.diagnostics;
    // TS1185: the scanner skips merge conflict markers as trivia.
    if !conflict_markers.is_empty() {
        for start in conflict_markers {
            diagnostics.push(Diagnostic {
                code: 1185,
                message: "Merge conflict marker encountered.".to_string(),
                category: DiagnosticCategory::Error,
                file_name: Some(file_name.to_string()),
                span: Some(Span::new(start, start + 7)),
                related: None,
            });
        }
        diagnostics.sort_by_key(|diagnostic| diagnostic.span.map_or(0, |span| span.start));
    }
    // TS1160: the scanner ends an unterminated template at EOF without
    // complaint; tsc reports it at the end of the file.
    if let Some(last) = p
        .tokens
        .iter()
        .rev()
        .find(|token| token.kind != TokenKind::EndOfFile)
    {
        let text = &source[last.span.start as usize..last.span.end as usize];
        let terminated = || {
            let bytes = text.as_bytes();
            bytes.len() >= 2
                && bytes[bytes.len() - 1] == b'`'
                && bytes[..bytes.len() - 1]
                    .iter()
                    .rev()
                    .take_while(|&&b| b == b'\\')
                    .count()
                    % 2
                    == 0
        };
        if matches!(
            last.kind,
            TokenKind::NoSubstitutionTemplate | TokenKind::TemplateTail
        ) && last.span.end as usize == source.len()
            && !terminated()
        {
            let end = source.len() as u32;
            diagnostics.push(Diagnostic {
                code: 1160,
                message: "Unterminated template literal.".to_string(),
                category: DiagnosticCategory::Error,
                file_name: Some(file_name.to_string()),
                span: Some(Span::new(end, end)),
                related: None,
            });
        }
    }
    // These grammar checks run only after a syntactically valid parse.
    // Keep parser recovery errors and semantic diagnostics independent.
    if has_syntax_errors(&diagnostics) {
        diagnostics.retain(|diagnostic| {
            !diagnostic
                .span
                .is_some_and(|span| p.grammar_errors.contains(&(diagnostic.code, span)))
        });
    }
    // Eager scanner diagnostics are collected from the token stream before
    // recursive-descent parsing starts. Merge them back into source order with
    // parser diagnostics, matching TypeScript's streaming scanner.
    if diagnostics
        .iter()
        .any(|diagnostic| matches!(diagnostic.code, 1121 | 1125))
    {
        // TypeScript keeps the grammar diagnostic over TS1121 when both report
        // at the same source position. This matters for invalid negative
        // property names: TS1136 starts on `-`, which is also where the signed
        // TS1121 span would start. A missing hexadecimal digit is different:
        // the streaming scanner owns that failure position and prevents a
        // parser-recovery diagnostic such as TS1005 from replacing TS1125.
        let grammar_error_starts: Vec<_> = diagnostics
            .iter()
            .filter(|diagnostic| !matches!(diagnostic.code, 1121 | 1125))
            .filter_map(|diagnostic| diagnostic.span.map(|span| span.start))
            .collect();
        let hexadecimal_failure_starts: Vec<_> = diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == 1125)
            .filter_map(|diagnostic| diagnostic.span.map(|span| span.start))
            .collect();
        diagnostics.retain(|diagnostic| {
            if diagnostic.code == 1121 {
                return !diagnostic
                    .span
                    .is_some_and(|span| grammar_error_starts.contains(&span.start));
            }
            diagnostic.code == 1125
                || !diagnostic
                    .span
                    .is_some_and(|span| hexadecimal_failure_starts.contains(&span.start))
        });
        diagnostics.sort_by_key(|diagnostic| {
            diagnostic
                .span
                .map_or(u32::MAX, |diagnostic_span| diagnostic_span.start)
        });
    }
    if !is_js_file
        && (suppress_typescript_ts1206
            || diagnostics.iter().any(|diagnostic| diagnostic.code != 1206))
    {
        let bare_variable_decorator_starts: Vec<_> = statements
            .iter()
            .filter_map(|statement| {
                let StmtKind::Var(variable) = &statement.kind else {
                    return None;
                };
                let is_bare_recovery = !variable.declarations.is_empty()
                    && variable.declarations.iter().all(|declaration| {
                        matches!(&declaration.name.kind, PatKind::Ident(name) if name == "<error>")
                            && declaration.init.is_none()
                            && source
                                .as_bytes()
                                .get(declaration.name.span.start as usize)
                                .is_none_or(|byte| *byte == b';')
                    });
                (is_bare_recovery
                    && source.as_bytes().get(statement.span.start as usize) == Some(&b'@'))
                .then_some(statement.span.start)
            })
            .collect();
        diagnostics.retain(|diagnostic| {
            diagnostic.code != 1206
                || diagnostic
                    .span
                    .is_some_and(|span| bare_variable_decorator_starts.contains(&span.start))
        });
    }
    for diagnostic in &mut diagnostics {
        for related in diagnostic.related.iter_mut().flatten() {
            if related.file_name.is_none() {
                related.file_name = Some(file_name.to_owned());
            }
        }
    }
    let end = source.len() as u32;
    SourceFile {
        file_name: file_name.to_string(),
        text: source.to_string(),
        statements,
        diagnostics,
        span: Span::new(0, end),
        comments: raw_comments,
    }
}

// ---------------------------------------------------------------------------
// Parser state
// ---------------------------------------------------------------------------

struct Parser<'a> {
    source: &'a str,
    tokens: Vec<Token>,
    /// Dense control-plane view of `tokens`. Recursive-descent parsing queries
    /// kinds far more often than spans; keeping one byte per token contiguous
    /// avoids pulling the 12-byte token records into cache for every `cur()`.
    token_kinds: Vec<TokenKind>,
    pos: usize,
    diagnostics: Vec<Diagnostic>,
    grammar_errors: Vec<(u32, Span)>,
    /// True when parsing a .tsx file (JSX enabled, angle-bracket type assertions disabled)
    is_tsx: bool,
    /// True when parsing the init expression of a for-loop header, so that
    /// the `in` keyword is not treated as a binary operator.
    disallow_in: bool,
    /// True when parsing the consequent of a ternary expression.
    /// Used to disambiguate `(x): T =>` (arrow return type) from `: alt =>` (ternary colon).
    in_ternary_consequent: bool,
    /// >0 while speculatively parsing (rolled back on failure). Error-recovery
    /// resyncs must not run then — consuming junk would make a doomed
    /// speculation "succeed" (e.g. `a ? (b ? c : d) : e` mis-committing as
    /// arrow params).
    speculation_depth: u32,
    /// Set when member recovery decides the class body should be abandoned
    /// (tsc's `modifier {` recovery re-parses the remainder as statements).
    abandon_class_body: bool,
    /// Depth of class bodies being parsed.
    class_depth: u32,
    /// True only when parsing the DIRECT body of a class method/constructor.
    /// Nested blocks (if, try, for, etc.) set this to false.
    in_class_method_top_body: bool,
    /// True when type predicates (`x is T`) are allowed. Only set when parsing
    /// function return types, NOT var declaration type annotations.
    allow_type_predicate: bool,
    /// When a block body is closed early due to class member recovery, this
    /// stores the source position of the recovered token so that enclosing
    /// spans can be extended to cover the intervening whitespace/newlines.
    block_recovery_end: Option<u32>,
    /// True when parsing the constraint type in `infer T extends <type>`.
    /// Prevents the type parser from consuming `extends ... ? ... : ...`
    /// as a conditional type, so that the `?` can be matched by an outer
    /// conditional type.
    disallow_conditional_types: bool,
    /// Scratch stacks for building child lists at their exact final length.
    ///
    /// A `Vec` jumps straight to capacity 4 on first push, so a one-declarator
    /// `var` statement allocated 448 B to hold 112, and a one-param function
    /// 544 B to hold 136. Lists are accumulated here and then copied into a
    /// buffer sized to the real element count (see `take_exact`).
    ///
    /// Nesting is LIFO — a default value may contain an arrow function with its
    /// own parameter list — so each list records a `mark` and drains only the
    /// region it pushed. Both list parsers have a single exit and always drain,
    /// so a rolled-back speculative parse cannot leave items stranded here.
    scratch_params: Vec<Param>,
    scratch_decls: Vec<VarDeclarator>,
    scratch_binding_name_full_starts: Vec<BindingNameFullStart>,
    type_arg_split_rollback: Vec<(usize, Token)>,
    type_arg_split_rollback_depth: usize,
    /// `(pos, end)` of the last `>` that `eat_greater_than` split off a
    /// compound token (`>>`, `>=`, `>>=`, ...). The split leaves `pos` on the
    /// remainder, so `tokens[pos - 1]` is the token BEFORE the `>`; `span_from`
    /// uses this to end the node after the `>` (`Record<K,V>={}` otherwise
    /// ends at `V`, and fast-emit's type erasure left `x>={}` behind).
    split_gt_end: Option<(usize, u32)>,
    /// Declaration files and ambient module bodies establish ambient grammar
    /// context. Only their direct statement lists participate in TS1036.
    ambient_depth: u32,
    /// Declaration-file ambient context is implicit at the source-file level.
    /// Unlike nested ambient bodies, it still permits an explicit `declare`.
    is_declaration_file: bool,
    /// JavaScript source files accept the common grammar for recovery, but
    /// TypeScript-only syntax still receives the TS800x syntactic diagnostics.
    is_js_file: bool,
    /// Nested `with` statements only receive the strict-mode diagnostic;
    /// TS2410 is attached to the outermost unsupported with-region.
    with_depth: u32,
    /// Upstream records a parse failure for recovery that this parser can
    /// represent structurally without surfacing the same diagnostic yet.
    suppress_typescript_ts1206: bool,
}

/// Move `scratch[mark..]` into a `Vec` by splitting the reusable tail off the
/// scratch buffer.
fn take_exact<T>(scratch: &mut Vec<T>, mark: usize) -> Vec<T> {
    let n = scratch.len() - mark;
    if n == 0 {
        // An empty `Vec` never allocates; keep it that way.
        return Vec::new();
    }
    scratch.split_off(mark)
}

impl<'a> Parser<'a> {
    fn grammar_error_at_span(&mut self, code: u32, message: String, span: Span) {
        self.grammar_errors.push((code, span));
        self.error_at_span(code, message, span);
    }

    fn new(
        source: &'a str,
        tokens: Vec<Token>,
        is_tsx: bool,
        is_declaration_file: bool,
        is_js_file: bool,
    ) -> Self {
        let diagnostics = Self::eager_scanner_diagnostics(source, &tokens);
        let token_kinds = tokens.iter().map(|token| token.kind).collect();
        debug_assert_eq!(
            tokens.last().map(|token| token.kind),
            Some(TokenKind::EndOfFile)
        );
        Self {
            source,
            tokens,
            token_kinds,
            pos: 0,
            diagnostics,
            grammar_errors: Vec::new(),
            disallow_in: false,
            in_ternary_consequent: false,
            speculation_depth: 0,
            abandon_class_body: false,
            is_tsx,
            class_depth: 0,
            in_class_method_top_body: false,
            allow_type_predicate: false,
            block_recovery_end: None,
            disallow_conditional_types: false,
            scratch_params: Vec::with_capacity(24),
            scratch_decls: Vec::with_capacity(16),
            scratch_binding_name_full_starts: Vec::with_capacity(16),
            type_arg_split_rollback: Vec::with_capacity(16),
            type_arg_split_rollback_depth: 0,
            split_gt_end: None,
            ambient_depth: u32::from(is_declaration_file),
            is_declaration_file,
            is_js_file,
            with_depth: 0,
            suppress_typescript_ts1206: false,
        }
    }

    /// Collect scanner-level numeric diagnostics before parsing mutates or
    /// speculatively rewinds the token stream.
    ///
    /// TypeScript diagnoses these tokens in every syntactic position. Its
    /// scanner also remembers whether the preceding token was `-`: in that
    /// case the suggested spelling is negative and the diagnostic begins one
    /// source character before the numeric token. This intentionally includes
    /// the final trivia character when trivia separates `-` and the literal,
    /// matching the upstream scanner's span behavior.
    fn eager_scanner_diagnostics(source: &str, tokens: &[Token]) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        for (index, token) in tokens.iter().enumerate() {
            if !matches!(
                token.kind,
                TokenKind::NumericLiteral | TokenKind::BigIntLiteral
            ) {
                continue;
            }

            let raw = &source[token.span.start as usize..token.span.end as usize];
            if raw
                .get(..2)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("0x"))
                && !raw[2..].bytes().any(|byte| byte.is_ascii_hexdigit())
            {
                // The numeric scanner consumes separators and a possible
                // bigint suffix while looking for the first digit. TypeScript
                // anchors the zero-width failure after those leading
                // separators, before the suffix or next token.
                let separator_count = raw[2..].bytes().take_while(|byte| *byte == b'_').count();
                let failure = token.span.start + 2 + separator_count as u32;
                diagnostics.push(Diagnostic {
                    code: 1125,
                    message: "Hexadecimal digit expected.".to_string(),
                    category: DiagnosticCategory::Error,
                    file_name: None,
                    span: Some(Span::new(failure, failure)),
                    related: None,
                });
            }

            if token.kind != TokenKind::NumericLiteral {
                continue;
            }
            if !Self::is_legacy_octal_literal(raw) {
                continue;
            }

            let is_negative = index
                .checked_sub(1)
                .and_then(|previous| tokens.get(previous))
                .is_some_and(|previous| previous.kind == TokenKind::Minus);
            let digits = raw.trim_start_matches('0');
            let digits = if digits.is_empty() { "0" } else { digits };
            let literal = if is_negative {
                format!("-0o{digits}")
            } else {
                format!("0o{digits}")
            };
            let start = if is_negative {
                source[..token.span.start as usize]
                    .char_indices()
                    .next_back()
                    .map_or(token.span.start, |(start, _)| start as u32)
            } else {
                token.span.start
            };

            diagnostics.push(Diagnostic {
                code: 1121,
                message: format!("Octal literals are not allowed. Use the syntax '{literal}'."),
                category: DiagnosticCategory::Error,
                file_name: None,
                span: Some(Span::new(start, token.span.end)),
                related: None,
            });
        }

        diagnostics
    }

    /// The repository scanner is eager, whereas TypeScript's scanner is
    /// parser-driven. In ambiguous contexts such as `value < /\00/`, the
    /// eager token stream may contain a numeric `00` in a range that the parser
    /// later reinterprets as regular-expression or JSX text. Discard scanner
    /// diagnostics covered by that successful reinterpretation.
    pub(crate) fn suppress_eager_scanner_diagnostics_in_span(&mut self, covered_span: Span) {
        self.diagnostics.retain(|diagnostic| {
            !matches!(diagnostic.code, 1121 | 1125)
                || !diagnostic.span.is_some_and(|span| {
                    span.start >= covered_span.start && span.end <= covered_span.end
                })
        });
    }

    // -----------------------------------------------------------------------
    // Token helpers
    // -----------------------------------------------------------------------

    fn cur(&self) -> TokenKind {
        debug_assert!(self.pos < self.token_kinds.len());
        // SAFETY: `bump` stops at the EOF sentinel and every direct cursor
        // increment is guarded by a non-EOF token or the token-vector length.
        unsafe { *self.token_kinds.get_unchecked(self.pos) }
    }

    #[inline(always)]
    fn debug_assert_token_cursor_invariants(&self) {
        debug_assert_eq!(self.tokens.len(), self.token_kinds.len());
        debug_assert_eq!(self.token_kinds.last(), Some(&TokenKind::EndOfFile));
        debug_assert!(self.pos < self.token_kinds.len());
    }

    #[inline]
    fn replace_token(&mut self, index: usize, token: Token) {
        self.token_kinds[index] = token.kind;
        self.tokens[index] = token;
        self.debug_assert_token_cursor_invariants();
    }

    #[inline]
    fn set_token_kind(&mut self, index: usize, kind: TokenKind) {
        self.token_kinds[index] = kind;
        self.tokens[index].kind = kind;
        self.debug_assert_token_cursor_invariants();
    }

    fn insert_token(&mut self, index: usize, token: Token) {
        self.token_kinds.insert(index, token.kind);
        self.tokens.insert(index, token);
        self.debug_assert_token_cursor_invariants();
    }

    fn remove_token(&mut self, index: usize) -> Token {
        self.token_kinds.remove(index);
        let token = self.tokens.remove(index);
        self.debug_assert_token_cursor_invariants();
        token
    }

    fn cur_span(&self) -> Span {
        debug_assert!(self.pos < self.tokens.len());
        // SAFETY: same cursor invariant as `cur`.
        unsafe { self.tokens.get_unchecked(self.pos).span }
    }

    fn text(&self, span: Span) -> &'a str {
        &self.source[span.start as usize..span.end as usize]
    }

    fn _cur_text(&self) -> &'a str {
        self.text(self.cur_span())
    }

    fn bump(&mut self) -> Span {
        let span = self.cur_span();
        // Keep the cursor parked on the final EOF sentinel. The bool-to-usize
        // conversion gives the hot non-EOF path no branch.
        self.pos += usize::from(self.pos + 1 < self.tokens.len());
        span
    }

    /// Restore the token cursor after an abandoned speculative parse branch.
    fn rewind_to(&mut self, position: usize) {
        self.pos = position;
    }

    /// Consume an ordinary JavaScript/TypeScript string literal and surface
    /// escape diagnostics. JSX attribute strings deliberately do not use this
    /// helper: backslashes in quoted JSX text are literal characters.
    #[inline]
    fn bump_string_literal(&mut self) -> Span {
        debug_assert_eq!(self.cur(), TokenKind::StringLiteral);
        let span = self.cur_span();
        if self.text(span).as_bytes().contains(&b'\\') {
            self.report_literal_escape_diagnostics(span);
        }
        self.bump()
    }

    /// Consume one untagged template chunk. Invalid escapes are permitted in
    /// tagged templates (their cooked value is `undefined`), so tagged paths
    /// continue to use `bump` directly.
    #[inline]
    fn bump_template_chunk(&mut self, is_tagged: bool) -> Span {
        debug_assert!(matches!(
            self.cur(),
            TokenKind::NoSubstitutionTemplate
                | TokenKind::TemplateHead
                | TokenKind::TemplateMiddle
                | TokenKind::TemplateTail
        ));
        let span = self.cur_span();
        if !is_tagged && self.text(span).as_bytes().contains(&b'\\') {
            self.report_literal_escape_diagnostics(span);
        }
        self.bump()
    }

    /// Report scanner escape diagnostics in a raw literal fragment.
    ///
    #[cold]
    fn report_literal_escape_diagnostics(&mut self, span: Span) {
        let bytes = self.text(span).as_bytes();
        let mut offset = 0usize;
        let diagnostic_start = self.diagnostics.len();
        // TypeScript's parser keeps only the first scanner diagnostic at a
        // source position. If a malformed \x / \u escape stops on another
        // backslash, its zero-width hex/unicode error suppresses a TS1487
        // attempted at that same backslash, but later escapes still report.
        let mut hexadecimal_failure_starts = Vec::new();
        let mut suppressed_octal_starts = Vec::new();
        let mut found = Vec::new();

        while offset + 1 < bytes.len() {
            if bytes[offset] != b'\\' {
                offset += 1;
                continue;
            }

            let first = bytes[offset + 1];
            if first == b'x' {
                let mut failure = offset + 2;
                for _ in 0..2 {
                    if failure >= bytes.len() || !bytes[failure].is_ascii_hexdigit() {
                        break;
                    }
                    failure += 1;
                }
                if failure < offset + 4 {
                    hexadecimal_failure_starts.push(failure);
                    if bytes.get(failure) == Some(&b'\\') {
                        suppressed_octal_starts.push(failure);
                    }
                }
                offset += 2;
                continue;
            }
            if first == b'u' {
                let mut failure = offset + 2;
                if bytes.get(failure) == Some(&b'{') {
                    failure += 1;
                    while bytes
                        .get(failure)
                        .is_some_and(|byte| byte.is_ascii_hexdigit())
                    {
                        failure += 1;
                    }
                    // Once at least one digit is present, a malformed or
                    // missing closing brace is TS1199 rather than TS1125.
                    if failure == offset + 3 {
                        hexadecimal_failure_starts.push(failure);
                    }
                    if bytes.get(failure) == Some(&b'\\') {
                        suppressed_octal_starts.push(failure);
                    }
                } else {
                    for _ in 0..4 {
                        if failure >= bytes.len() || !bytes[failure].is_ascii_hexdigit() {
                            break;
                        }
                        failure += 1;
                    }
                    if failure < offset + 6 {
                        hexadecimal_failure_starts.push(failure);
                        if bytes.get(failure) == Some(&b'\\') {
                            suppressed_octal_starts.push(failure);
                        }
                    }
                }
                offset += 2;
                continue;
            }
            if !(b'0'..=b'7').contains(&first) {
                // Consume the escaped byte as well, which is what excludes the
                // second slash in `\\01` from becoming an octal escape.
                offset += 2;
                continue;
            }

            let followed_by_decimal = bytes
                .get(offset + 2)
                .is_some_and(|byte| byte.is_ascii_digit());
            if (first == b'0' && !followed_by_decimal) || suppressed_octal_starts.contains(&offset)
            {
                offset += 2;
                continue;
            }

            let max_digits = if first <= b'3' { 3 } else { 2 };
            let mut end = offset + 2;
            while end < bytes.len()
                && end < offset + 1 + max_digits
                && (b'0'..=b'7').contains(&bytes[end])
            {
                end += 1;
            }

            let mut value = 0u8;
            for digit in &bytes[offset + 1..end] {
                value = value * 8 + (*digit - b'0');
            }
            found.push((
                Span::new(span.start + offset as u32, span.start + end as u32),
                value,
            ));
            offset = end;
        }

        for failure in hexadecimal_failure_starts {
            let failure = span.start + failure as u32;
            let failure_span = Span::new(failure, failure);
            if self
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == 1125 && diagnostic.span == Some(failure_span))
            {
                continue;
            }
            self.error_at_span(
                1125,
                "Hexadecimal digit expected.".to_string(),
                failure_span,
            );
        }

        for (escape_span, value) in found {
            if self.diagnostics.iter().any(|diagnostic| {
                (diagnostic.code == 1487 && diagnostic.span == Some(escape_span))
                    || (diagnostic.code == 1125
                        && diagnostic
                            .span
                            .is_some_and(|span| span.start == escape_span.start))
            }) {
                continue;
            }
            self.error_at_span(
                1487,
                format!("Octal escape sequences are not allowed. Use the syntax '\\x{value:02x}'."),
                escape_span,
            );
        }

        self.diagnostics[diagnostic_start..].sort_by_key(|diagnostic| {
            diagnostic
                .span
                .map_or(u32::MAX, |diagnostic_span| diagnostic_span.start)
        });
    }

    fn previous_token_can_tag_template(&self) -> bool {
        let Some(previous) = self
            .pos
            .checked_sub(1)
            .and_then(|index| self.token_kinds.get(index))
        else {
            return false;
        };
        previous.is_identifier_name()
            || matches!(
                previous,
                TokenKind::NumericLiteral
                    | TokenKind::BigIntLiteral
                    | TokenKind::StringLiteral
                    | TokenKind::RegExpLiteral
                    | TokenKind::NoSubstitutionTemplate
                    | TokenKind::TemplateTail
                    | TokenKind::CloseParen
                    | TokenKind::CloseBracket
                    | TokenKind::CloseBrace
                    | TokenKind::PlusPlus
                    | TokenKind::MinusMinus
                    | TokenKind::Excl
            )
    }

    /// Balanced import-attribute recovery skips expressions instead of fully
    /// parsing them. Preserve literal diagnostics while tracking whether each
    /// template head is postfix-tagged.
    fn bump_skipped_import_token(&mut self, template_tags: &mut Vec<bool>) {
        match self.cur() {
            TokenKind::StringLiteral => {
                self.bump_string_literal();
            }
            TokenKind::NoSubstitutionTemplate => {
                let tagged = self.previous_token_can_tag_template();
                self.bump_template_chunk(tagged);
            }
            TokenKind::TemplateHead => {
                let tagged = self.previous_token_can_tag_template();
                template_tags.push(tagged);
                self.bump_template_chunk(tagged);
            }
            TokenKind::TemplateMiddle => {
                let tagged = template_tags.last().copied().unwrap_or(false);
                self.bump_template_chunk(tagged);
            }
            TokenKind::TemplateTail => {
                let tagged = template_tags.pop().unwrap_or(false);
                self.bump_template_chunk(tagged);
            }
            _ => {
                self.bump();
            }
        }
    }

    fn at(&self, kind: TokenKind) -> bool {
        self.cur() == kind
    }

    fn eat(&mut self, kind: TokenKind) -> Option<Span> {
        if self.at(kind) {
            Some(self.bump())
        } else {
            None
        }
    }

    fn expect(&mut self, kind: TokenKind) -> Span {
        if let Some(s) = self.eat(kind) {
            s
        } else {
            self.expect_error(kind)
        }
    }

    #[cold]
    #[inline(never)]
    fn expect_error(&mut self, kind: TokenKind) -> Span {
        let span = self.cur_span();
        let code = if kind == TokenKind::Identifier {
            1003
        } else {
            1005
        };
        let message = if kind == TokenKind::Identifier {
            "Identifier expected.".to_owned()
        } else if let Some(text) = kind.fixed_text() {
            format!("'{text}' expected.")
        } else {
            // `expect` callers request identifiers, keywords, or punctuation.
            // Keep a useful diagnostic if a future caller requests a literal.
            format!("{kind:?} expected.")
        };
        self.error_code(code, message);
        span
    }

    #[inline]
    fn expect_opening_delimiter(&mut self, kind: TokenKind) -> Option<Span> {
        let opening = self.eat(kind);
        if opening.is_none() {
            self.expect_error(kind);
        }
        opening
    }

    #[inline]
    fn expect_matching_delimiter(
        &mut self,
        opening: Option<Span>,
        open_kind: TokenKind,
        close_kind: TokenKind,
    ) -> Span {
        if let Some(span) = self.eat(close_kind) {
            return span;
        }
        self.matching_delimiter_error(opening, open_kind, close_kind)
    }

    #[cold]
    #[inline(never)]
    fn matching_delimiter_error(
        &mut self,
        opening: Option<Span>,
        open_kind: TokenKind,
        close_kind: TokenKind,
    ) -> Span {
        let current = self.cur_span();
        // Nested missing delimiters share an error position. Retain the first
        // diagnostic and its innermost opening token, as TypeScript does.
        if self.diagnostics.last().is_some_and(|diagnostic| {
            diagnostic
                .span
                .is_some_and(|span| span.start == current.start)
        }) {
            return current;
        }
        let span = self.expect_error(close_kind);
        if let (Some(opening), Some(open), Some(close)) =
            (opening, open_kind.fixed_text(), close_kind.fixed_text())
        {
            if let Some(diagnostic) = self.diagnostics.last_mut() {
                diagnostic.related = Some(vec![RelatedDiagnostic {
                    code: 1007,
                    message: format!(
                        "The parser expected to find a '{close}' to match the '{open}' token here."
                    ),
                    file_name: None,
                    span: Some(opening),
                }]);
            }
        }
        span
    }

    /// Try to consume a single `>` token. If the current token is a compound
    /// greater-than token (`>>`, `>>>`, `>>=`, `>>>=`), split it: consume the
    /// first `>` and replace the current token with the remainder.
    ///
    /// This is needed in type parameter/argument contexts where `>>` should be
    /// treated as two closing `>` tokens (e.g. `Map<string, Array<number>>`).
    fn eat_greater_than(&mut self) -> Option<Span> {
        if let Some(tok) = self.tokens.get(self.pos) {
            let span = tok.span;
            match tok.kind {
                TokenKind::GreaterThan => {
                    self.pos += 1;
                    return Some(span);
                }
                TokenKind::GreaterGreater => {
                    // >> → consume first >, leave >
                    let mid = span.start + 1;
                    if self.type_arg_split_rollback_depth > 0 {
                        self.type_arg_split_rollback.push((self.pos, tok.clone()));
                    }
                    self.replace_token(
                        self.pos,
                        Token {
                            kind: TokenKind::GreaterThan,
                            span: Span::new(mid, span.end),
                            preceded_by_line_break: false,
                        },
                    );
                    self.split_gt_end = Some((self.pos, mid));
                    return Some(Span::new(span.start, mid));
                }
                TokenKind::GreaterGreaterGreater => {
                    // >>> → consume first >, leave >>
                    let mid = span.start + 1;
                    if self.type_arg_split_rollback_depth > 0 {
                        self.type_arg_split_rollback.push((self.pos, tok.clone()));
                    }
                    self.replace_token(
                        self.pos,
                        Token {
                            kind: TokenKind::GreaterGreater,
                            span: Span::new(mid, span.end),
                            preceded_by_line_break: false,
                        },
                    );
                    self.split_gt_end = Some((self.pos, mid));
                    return Some(Span::new(span.start, mid));
                }
                TokenKind::GreaterGreaterEquals => {
                    // >>= → consume first >, leave >=
                    let mid = span.start + 1;
                    if self.type_arg_split_rollback_depth > 0 {
                        self.type_arg_split_rollback.push((self.pos, tok.clone()));
                    }
                    self.replace_token(
                        self.pos,
                        Token {
                            kind: TokenKind::GreaterEqual,
                            span: Span::new(mid, span.end),
                            preceded_by_line_break: false,
                        },
                    );
                    self.split_gt_end = Some((self.pos, mid));
                    return Some(Span::new(span.start, mid));
                }
                TokenKind::GreaterGreaterGreaterEquals => {
                    // >>>= → consume first >, leave >>=
                    let mid = span.start + 1;
                    if self.type_arg_split_rollback_depth > 0 {
                        self.type_arg_split_rollback.push((self.pos, tok.clone()));
                    }
                    self.replace_token(
                        self.pos,
                        Token {
                            kind: TokenKind::GreaterGreaterEquals,
                            span: Span::new(mid, span.end),
                            preceded_by_line_break: false,
                        },
                    );
                    self.split_gt_end = Some((self.pos, mid));
                    return Some(Span::new(span.start, mid));
                }
                TokenKind::GreaterEqual => {
                    // >= → consume first >, leave =
                    let mid = span.start + 1;
                    if self.type_arg_split_rollback_depth > 0 {
                        self.type_arg_split_rollback.push((self.pos, tok.clone()));
                    }
                    self.replace_token(
                        self.pos,
                        Token {
                            kind: TokenKind::Equals,
                            span: Span::new(mid, span.end),
                            preceded_by_line_break: false,
                        },
                    );
                    self.split_gt_end = Some((self.pos, mid));
                    return Some(Span::new(span.start, mid));
                }
                _ => {}
            }
        }
        None
    }

    /// Try to consume a single `<` token. If the current token is a compound
    /// less-than token (`<<` or `<<=`), split it so type parsing can consume
    /// just the first `<` and leave the remainder for subsequent parsing.
    fn eat_less_than(&mut self) -> Option<Span> {
        if let Some(tok) = self.tokens.get(self.pos) {
            let span = tok.span;
            match tok.kind {
                TokenKind::LessThan => {
                    self.pos += 1;
                    return Some(span);
                }
                TokenKind::LessLess => {
                    let mid = span.start + 1;
                    if self.type_arg_split_rollback_depth > 0 {
                        self.type_arg_split_rollback.push((self.pos, tok.clone()));
                    }
                    self.replace_token(
                        self.pos,
                        Token {
                            kind: TokenKind::LessThan,
                            span: Span::new(mid, span.end),
                            preceded_by_line_break: false,
                        },
                    );
                    return Some(Span::new(span.start, mid));
                }
                TokenKind::LessLessEquals => {
                    let mid = span.start + 1;
                    if self.type_arg_split_rollback_depth > 0 {
                        self.type_arg_split_rollback.push((self.pos, tok.clone()));
                    }
                    self.replace_token(
                        self.pos,
                        Token {
                            kind: TokenKind::LessEqual,
                            span: Span::new(mid, span.end),
                            preceded_by_line_break: false,
                        },
                    );
                    return Some(Span::new(span.start, mid));
                }
                _ => {}
            }
        }
        None
    }

    fn error(&mut self, message: String) {
        self.error_code(1002, message);
    }

    /// Parser diagnostic with tsc's real error code — recall/precision
    /// against tsc baselines matches on (file, line, code).
    fn error_code(&mut self, code: u32, message: String) {
        let span = if code == 1003 && self.is_eof() {
            // Missing identifiers are inserted before EOF's leading trivia.
            let position = self
                .pos
                .checked_sub(1)
                .and_then(|index| self.tokens.get(index))
                .map_or(0, |token| token.span.end);
            Span::new(position, position)
        } else {
            self.cur_span()
        };
        self.error_at_span(code, message, span);
    }

    fn error_at_span(&mut self, code: u32, message: String, span: Span) {
        self.diagnostics.push(Diagnostic {
            code,
            message,
            category: DiagnosticCategory::Error,
            file_name: None,
            span: Some(span),
            related: None,
        });
    }

    fn is_eof(&self) -> bool {
        self.cur() == TokenKind::EndOfFile
    }

    /// Skip a `{ ... }` block for error recovery. Consumes matched braces.
    fn skip_block(&mut self) {
        if self.eat(TokenKind::OpenBrace).is_none() {
            return;
        }
        let mut depth: u32 = 1;
        while depth > 0 && !self.is_eof() {
            match self.cur() {
                TokenKind::OpenBrace => depth += 1,
                TokenKind::CloseBrace => depth -= 1,
                _ => {}
            }
            self.bump();
        }
    }

    /// Check if current token is an identifier or a contextual keyword usable as identifier.
    fn is_identifier(&self) -> bool {
        matches!(
            self.cur(),
            TokenKind::Identifier
                | TokenKind::As
                | TokenKind::Async
                | TokenKind::Await
                | TokenKind::Constructor
                | TokenKind::Declare
                | TokenKind::Get
                | TokenKind::Set
                | TokenKind::From
                | TokenKind::Of
                | TokenKind::Implements
                | TokenKind::Interface
                | TokenKind::Module
                | TokenKind::Namespace
                | TokenKind::Package
                | TokenKind::Private
                | TokenKind::Protected
                | TokenKind::Public
                | TokenKind::Static
                | TokenKind::Type
                | TokenKind::Global
                | TokenKind::Abstract
                | TokenKind::Override
                | TokenKind::Readonly
                | TokenKind::Keyof
                | TokenKind::Unique
                | TokenKind::Infer
                | TokenKind::Is
                | TokenKind::Asserts
                | TokenKind::Assert
                | TokenKind::Require
                | TokenKind::Never
                | TokenKind::UnknownKeyword
                | TokenKind::Any
                | TokenKind::Number
                | TokenKind::BigInt
                | TokenKind::String
                | TokenKind::Boolean
                | TokenKind::Symbol
                | TokenKind::Undefined
                | TokenKind::Object
                | TokenKind::Intrinsic
                | TokenKind::Satisfies
                | TokenKind::Using
                | TokenKind::Accessor
                | TokenKind::Out
                | TokenKind::Let
                | TokenKind::Yield
        )
    }

    #[inline]
    fn parse_identifier(&mut self) -> (AstString, Span) {
        if self.is_identifier() {
            self.bump_identifier_unchecked()
        } else {
            self.parse_identifier_error()
        }
    }

    #[inline(always)]
    fn bump_identifier_unchecked(&mut self) -> (AstString, Span) {
        let span = self.bump();
        (self.text(span).into(), span)
    }

    #[cold]
    #[inline(never)]
    fn parse_identifier_error(&mut self) -> (AstString, Span) {
        let span = self.cur_span();
        self.error_code(1003, "Identifier expected.".into());
        if !self.is_eof() {
            self.bump();
        }
        ("<error>".into(), span)
    }

    fn parse_identifier_name(&mut self) -> Option<(AstString, Span)> {
        // Any keyword can be used as a property name
        if self.cur().is_identifier_name() {
            let span = self.bump();
            Some((self.text(span).into(), span))
        } else {
            None
        }
    }

    fn string_literal_content(raw: &str) -> &str {
        let bytes = raw.as_bytes();
        let Some(first) = bytes.first().copied() else {
            return "";
        };
        if first != b'"' && first != b'\'' {
            return raw;
        }
        if bytes.len() >= 2 && bytes.last().copied() == Some(first) {
            &raw[1..raw.len() - 1]
        } else {
            &raw[1..]
        }
    }

    fn parse_module_export_name(&mut self) -> Option<(String, bool)> {
        if self.at(TokenKind::StringLiteral) {
            Some((self.parse_string_literal(), true))
        } else {
            self.parse_identifier_name()
                .map(|(name, _)| (name.into(), false))
        }
    }

    fn recover_named_import_local_error_tail(&mut self) {
        while !self.at(TokenKind::Comma)
            && !self.at(TokenKind::CloseBrace)
            && !self.at(TokenKind::From)
            && !self.is_eof()
        {
            let kind = self.cur();
            if kind == TokenKind::Unknown
                || kind == TokenKind::Identifier
                || kind.is_contextual_keyword()
                || kind.is_keyword()
            {
                self.bump();
            } else {
                break;
            }
        }
    }

    // -----------------------------------------------------------------------
    // Source elements
    // -----------------------------------------------------------------------

    fn parse_source_elements(&mut self) -> Vec<Stmt> {
        let mut stmts = Vec::new();
        let mut ambient_statement_reported = false;
        while !self.is_eof() {
            let before = self.pos;
            let first_token_span = self.cur_span();
            if self.ambient_depth > 0 {
                self.report_statement_in_ambient_context(&mut ambient_statement_reported);
            }
            let stmt = self.parse_statement(self.current_token_starts_statement());
            if self.is_declaration_file {
                self.report_missing_declare_modifier(&stmt, first_token_span);
            }
            stmts.push(stmt);
            // Error recovery: if we didn't advance, skip the current token
            if self.pos == before && !self.is_eof() {
                self.error_code(1128, "Declaration or statement expected.".into());
                self.bump();
            }
        }
        stmts
    }

    // -----------------------------------------------------------------------
    // Statements
    // -----------------------------------------------------------------------

    fn parse_statement(&mut self, at_statement_boundary: bool) -> Stmt {
        let start = self.cur_span().start;
        let decorators = if self.at(TokenKind::At) {
            self.parse_decorators()
        } else {
            Vec::new()
        };
        let modifier_pos = self.pos;
        let modifiers = match self.cur() {
            TokenKind::Declare | TokenKind::Abstract | TokenKind::Async => self.parse_modifiers(),
            _ => MOD_NONE,
        };
        if !decorators.is_empty() && !self.decorated_statement_targets_class() {
            // Decorator expressions are grammar-checked only when the
            // decorator is attached to a class declaration. TypeScript keeps
            // parsing other declaration-shaped targets for TS1206 recovery,
            // but does not run regexp grammar checks inside their discarded
            // decorators.
            let decorator_is_missing = matches!(decorators[0].kind, ExprKind::Omitted)
                || matches!(&decorators[0].kind, ExprKind::Ident(name) if name == "<error>");
            let invalid_declaration_decorator = if self.is_js_file {
                matches!(
                    self.cur(),
                    TokenKind::Var | TokenKind::Let | TokenKind::Const | TokenKind::Using
                ) || self.at(TokenKind::Await) && self.peek_is(TokenKind::Using)
                    || self.at(TokenKind::Import) && !self.current_import_is_import_equals()
                    || self.at(TokenKind::Export)
                        && self.current_js_export_has_syntactic_decorator()
                        && !self.current_export_eventually_targets_class()
                    || !at_statement_boundary && self.at(TokenKind::Function)
            } else {
                matches!(
                    self.cur(),
                    TokenKind::Interface
                        | TokenKind::Type
                        | TokenKind::Enum
                        | TokenKind::Namespace
                        | TokenKind::Module
                        | TokenKind::Import
                        | TokenKind::Global
                ) || matches!(
                    self.cur(),
                    TokenKind::Var | TokenKind::Let | TokenKind::Const | TokenKind::Using
                ) && self.current_variable_starts_checkable_declaration()
                    || self.at(TokenKind::Await)
                        && self.peek_is(TokenKind::Using)
                        && self.current_await_using_starts_checkable_declaration()
                    || self.at(TokenKind::Export)
                        && !self.peek_is(TokenKind::As)
                        && !self.current_export_eventually_targets_class()
            };
            if (!decorator_is_missing || self.is_js_file)
                && invalid_declaration_decorator
                && (at_statement_boundary || self.is_js_file)
            {
                let span = self.decorator_diagnostic_span(
                    &decorators[0],
                    self.is_js_file && !decorator_is_missing,
                );
                self.error_at_span(1206, "Decorators are not valid here.".to_string(), span);
            } else if !self.current_can_recover_as_decorated_declaration() {
                if self.is_js_file && !decorator_is_missing {
                    let span = self.decorator_diagnostic_span(&decorators[0], true);
                    self.error_at_span(1206, "Decorators are not valid here.".to_string(), span);
                }
                let end = decorators
                    .last()
                    .map_or(start, |decorator| decorator.span.end);
                self.error_at_span(
                    1146,
                    "Declaration expected.".to_string(),
                    Span::new(end, end),
                );
            }
        }
        if !decorators.is_empty()
            && self.at(TokenKind::Using)
            && !self.current_variable_starts_checkable_declaration()
        {
            self.suppress_typescript_ts1206 = true;
        }
        if self.is_js_file {
            for index in modifier_pos..self.pos {
                let token = &self.tokens[index];
                let name = match token.kind {
                    TokenKind::Declare => Some("declare"),
                    TokenKind::Abstract => Some("abstract"),
                    _ => None,
                };
                if let Some(name) = name {
                    self.error_at_span(
                        8009,
                        format!("The '{name}' modifier can only be used in TypeScript files."),
                        token.span,
                    );
                }
            }
        }
        if modifiers & MOD_DECLARE != 0 && self.ambient_depth > u32::from(self.is_declaration_file)
        {
            self.diagnostics.push(Diagnostic {
                code: 1038,
                message: "A 'declare' modifier cannot be used in an already ambient context."
                    .to_string(),
                category: DiagnosticCategory::Error,
                file_name: None,
                span: Some(Span::new(start, start + "declare".len() as u32)),
                related: None,
            });
        }

        match self.cur() {
            TokenKind::OpenBrace => self.parse_block_stmt(start),
            TokenKind::Var => self.parse_var_stmt(modifiers, VarKind::Var, start),
            TokenKind::Let => {
                if self.is_let_declaration() {
                    self.parse_var_stmt(modifiers, VarKind::Let, start)
                } else {
                    self.parse_expression_statement(start, modifiers)
                }
            }
            TokenKind::Const => {
                if self.peek_is(TokenKind::Enum) {
                    self.bump(); // const
                    self.parse_enum_decl(modifiers | MOD_CONST, start)
                } else {
                    self.parse_var_stmt(modifiers, VarKind::Const, start)
                }
            }
            TokenKind::Using
                if self.is_using_declaration()
                    || !decorators.is_empty() && self.peek_is(TokenKind::OpenBracket) =>
            {
                self.parse_var_stmt(modifiers, VarKind::Using, start)
            }
            TokenKind::Function => self.parse_function_decl(modifiers, decorators, start),
            TokenKind::Class => self.parse_class_decl(modifiers, decorators, start),
            TokenKind::Interface
                if modifiers & MOD_DECLARE != 0 || self.is_interface_declaration() =>
            {
                self.parse_interface_decl(modifiers, start)
            }
            TokenKind::Type if modifiers & MOD_DECLARE != 0 || self.is_type_alias_declaration() => {
                self.parse_type_alias_decl(modifiers, start)
            }
            TokenKind::Enum => self.parse_enum_decl(modifiers, start),
            TokenKind::Namespace | TokenKind::Module
                if modifiers & MOD_DECLARE != 0 || self.is_module_declaration() =>
            {
                self.parse_module_decl(modifiers, start)
            }
            TokenKind::Abstract if modifiers & MOD_ABSTRACT != 0 && self.at(TokenKind::Class) => {
                self.parse_class_decl(modifiers, decorators, start)
            }
            TokenKind::Default
                if !decorators.is_empty()
                    && matches!(
                        self.tokens.get(self.pos + 1).map(|t| t.kind),
                        Some(TokenKind::Class) | Some(TokenKind::Abstract)
                    ) =>
            {
                let saved = self.pos;
                self.bump(); // default
                let inner_modifiers = self.parse_modifiers();
                if self.at(TokenKind::Class) {
                    self.parse_class_decl(inner_modifiers, decorators, start)
                } else {
                    self.rewind_to(saved);
                    self.parse_expression_or_labeled(start, modifiers)
                }
            }
            TokenKind::Import
                if !self.peek_is(TokenKind::Dot) && !self.peek_is(TokenKind::OpenParen) =>
            {
                self.parse_import_or_import_equals(modifiers, start)
            }
            TokenKind::Export => self.parse_export_decl(modifiers, decorators, start),
            TokenKind::Return => self.parse_return_stmt(start),
            TokenKind::If => self.parse_if_stmt(start),
            TokenKind::While => self.parse_while_stmt(start),
            TokenKind::Do => self.parse_do_while_stmt(start),
            TokenKind::For => self.parse_for_stmt(start),
            TokenKind::Switch => self.parse_switch_stmt(start),
            TokenKind::Try => self.parse_try_stmt(start),
            // Error recovery: standalone `finally` synthesizes `try {} finally {}`
            TokenKind::Finally => {
                self.bump(); // finally
                let finalizer = self.parse_block_body();
                Stmt {
                    kind: StmtKind::Try(Box::new(TryStmt {
                        block: Vec::new(),
                        handler: None,
                        finalizer: Some(finalizer),
                    })),
                    span: self.span_from(start),
                }
            }
            // Error recovery: standalone `catch (x) { }` synthesizes `try {} catch (x) {}`
            // Only when followed by `(` to avoid capturing `catch` used as identifier.
            TokenKind::Catch
                if self.peek_is(TokenKind::OpenParen) || self.peek_is(TokenKind::OpenBrace) =>
            {
                self.bump(); // catch
                let handler = self.parse_catch_clause();
                Stmt {
                    kind: StmtKind::Try(Box::new(TryStmt {
                        block: Vec::new(),
                        handler: Some(handler),
                        finalizer: None,
                    })),
                    span: self.span_from(start),
                }
            }
            TokenKind::Throw => self.parse_throw_stmt(start),
            TokenKind::Break => {
                self.bump();
                let label = self.parse_optional_label();
                self.eat_semicolon();
                Stmt {
                    kind: StmtKind::Break(label),
                    span: self.span_from(start),
                }
            }
            TokenKind::Continue => {
                self.bump();
                let label = self.parse_optional_label();
                self.eat_semicolon();
                Stmt {
                    kind: StmtKind::Continue(label),
                    span: self.span_from(start),
                }
            }
            TokenKind::Debugger => {
                self.bump();
                self.eat_semicolon();
                Stmt {
                    kind: StmtKind::Debugger,
                    span: self.span_from(start),
                }
            }
            TokenKind::With => self.parse_with_stmt(start),
            TokenKind::Semicolon => {
                self.bump();
                Stmt {
                    kind: StmtKind::Empty,
                    span: self.span_from(start),
                }
            }
            TokenKind::Await
                if self.peek_is(TokenKind::Using)
                    && self.tokens.get(self.pos + 2).is_some_and(|t| {
                        t.kind == TokenKind::Identifier || t.kind == TokenKind::OpenBrace
                    }) =>
            {
                self.bump(); // await
                self.parse_var_stmt(modifiers, VarKind::AwaitUsing, start)
            }
            TokenKind::Async
                if self.peek_is(TokenKind::Function) && !self.peek_is_on_new_line() =>
            {
                self.parse_function_decl(modifiers | MOD_ASYNC, decorators, start)
            }
            TokenKind::Global
                if modifiers & MOD_DECLARE != 0 || self.peek_is(TokenKind::OpenBrace) =>
            {
                // `declare global { ... }` — route to module decl
                // Also plain `global { ... }` (global augmentation without `declare`)
                self.parse_module_decl(modifiers, start)
            }
            // Unknown/invalid tokens (e.g., bare `\`): skip without consuming
            // following valid tokens like `;` that should be separate statements.
            // Exception: `Unknown =` is handled in parse_expression_statement as
            // a recovery for `₁ = "hello"` → `"hello";`.
            TokenKind::Unknown if !self.peek_is(TokenKind::Equals) => {
                let span = self.bump();
                Stmt {
                    kind: StmtKind::Expr(Box::new(Expr {
                        kind: ExprKind::Ident("<error>".into()),
                        span: Span::new(span.start, span.end),
                    })),
                    span: Span::new(span.start, span.end),
                }
            }
            _ => self.parse_expression_or_labeled(start, modifiers),
        }
    }

    fn decorated_statement_targets_class(&self) -> bool {
        let mut position = self.pos;
        while matches!(
            self.tokens.get(position).map(|token| token.kind),
            Some(TokenKind::Export | TokenKind::Default | TokenKind::Abstract)
        ) {
            position += 1;
        }
        self.tokens
            .get(position)
            .is_some_and(|token| token.kind == TokenKind::Class)
    }

    fn current_can_recover_as_decorated_declaration(&self) -> bool {
        matches!(
            self.cur(),
            TokenKind::Var
                | TokenKind::Let
                | TokenKind::Const
                | TokenKind::Using
                | TokenKind::Function
                | TokenKind::Class
                | TokenKind::Interface
                | TokenKind::Type
                | TokenKind::Enum
                | TokenKind::Namespace
                | TokenKind::Module
                | TokenKind::Import
                | TokenKind::Export
                | TokenKind::Global
        ) || (self.at(TokenKind::Await) && self.peek_is(TokenKind::Using))
    }

    fn current_variable_starts_checkable_declaration(&self) -> bool {
        self.tokens.get(self.pos + 1).is_some_and(|token| {
            matches!(
                token.kind,
                TokenKind::Identifier
                    | TokenKind::OpenBrace
                    | TokenKind::OpenBracket
                    | TokenKind::Semicolon
                    | TokenKind::EndOfFile
            ) || token.kind.is_contextual_keyword()
        })
    }

    fn current_await_using_starts_checkable_declaration(&self) -> bool {
        self.tokens.get(self.pos + 2).is_some_and(|token| {
            token.kind == TokenKind::Identifier
                || token.kind == TokenKind::OpenBrace
                || token.kind.is_contextual_keyword()
        })
    }

    fn current_import_is_import_equals(&self) -> bool {
        let mut position = self.pos + 1;
        if self
            .tokens
            .get(position)
            .is_some_and(|token| token.kind == TokenKind::Type)
        {
            position += 1;
        }
        position += 1;
        self.tokens
            .get(position)
            .is_some_and(|token| token.kind == TokenKind::Equals)
    }

    fn current_js_export_has_syntactic_decorator(&self) -> bool {
        !matches!(
            self.tokens.get(self.pos + 1).map(|token| token.kind),
            Some(
                TokenKind::Equals
                    | TokenKind::Interface
                    | TokenKind::Type
                    | TokenKind::Enum
                    | TokenKind::Namespace
                    | TokenKind::Module
            )
        )
    }

    fn current_export_eventually_targets_class(&self) -> bool {
        for token in &self.tokens[self.pos + 1..] {
            match token.kind {
                TokenKind::Class => return true,
                TokenKind::Semicolon
                | TokenKind::OpenBrace
                | TokenKind::CloseBrace
                | TokenKind::EndOfFile => return false,
                _ => {}
            }
        }
        false
    }

    fn decorator_diagnostic_span(&self, decorator: &Expr, whole_decorator: bool) -> Span {
        let mut start = decorator.span.start;
        if self.source.as_bytes().get(start as usize) != Some(&b'@')
            && start > 0
            && self.source.as_bytes().get(start as usize - 1) == Some(&b'@')
        {
            start -= 1;
        }
        Span::new(
            start,
            if whole_decorator {
                decorator.span.end.max(start.saturating_add(1))
            } else {
                start.saturating_add(1)
            },
        )
    }

    /// TS1036 is attached to the first token of the first executable
    /// statement in an ambient statement list. Declarations remain valid;
    /// after the first report TypeScript continues parsing without repeating
    /// TS1036 for every subsequent statement in that same list.
    fn report_statement_in_ambient_context(&mut self, reported: &mut bool) {
        if *reported || self.current_starts_ambient_declaration() {
            return;
        }

        self.diagnostics.push(Diagnostic {
            code: 1036,
            message: "Statements are not allowed in ambient contexts.".to_string(),
            category: DiagnosticCategory::Error,
            file_name: None,
            span: Some(self.cur_span()),
            related: None,
        });
        *reported = true;
    }

    fn current_starts_ambient_declaration(&self) -> bool {
        match self.cur() {
            TokenKind::Var | TokenKind::Const | TokenKind::Function | TokenKind::Class => true,
            TokenKind::Let => self.is_let_declaration(),
            TokenKind::Using => self.is_using_declaration(),
            TokenKind::Interface => self.is_interface_declaration(),
            TokenKind::Type => self.is_type_alias_declaration(),
            TokenKind::Enum => true,
            TokenKind::Namespace | TokenKind::Module => self.is_module_declaration(),
            TokenKind::Import => {
                !self.peek_is(TokenKind::Dot) && !self.peek_is(TokenKind::OpenParen)
            }
            TokenKind::Export => true,
            TokenKind::Global => self.peek_is(TokenKind::OpenBrace),
            TokenKind::Declare => self.peek_can_follow_declare(),
            TokenKind::Abstract => self.peek_is(TokenKind::Class),
            TokenKind::Async => self.peek_is(TokenKind::Function) && !self.peek_is_on_new_line(),
            TokenKind::Await => {
                self.peek_is(TokenKind::Using)
                    && self.tokens.get(self.pos + 2).is_some_and(|token| {
                        matches!(
                            token.kind,
                            TokenKind::Identifier | TokenKind::OpenBrace | TokenKind::OpenBracket
                        )
                    })
            }
            // Decorators only introduce declarations. Invalid decorated forms
            // receive their more specific parser diagnostics.
            TokenKind::At => true,
            _ => false,
        }
    }

    fn report_missing_declare_modifier(&mut self, stmt: &Stmt, first_token_span: Span) {
        let modifiers = match &stmt.kind {
            StmtKind::Var(var_stmt) => var_stmt.modifiers,
            StmtKind::FnDecl(function) => function.modifiers,
            StmtKind::ClassDecl(class) => class.modifiers,
            StmtKind::EnumDecl(enumeration) => enumeration.modifiers,
            StmtKind::ModuleDecl(module) => module.modifiers,
            // Imports and purely type-level declarations are inherently valid
            // in declaration files; exported value declarations are wrapped
            // in `StmtKind::Export` and are valid as well.
            _ => return,
        };
        if modifiers & MOD_DECLARE != 0 {
            return;
        }

        self.diagnostics.push(Diagnostic {
            code: 1046,
            message:
                "Top-level declarations in .d.ts files must start with either a 'declare' or 'export' modifier."
                    .to_string(),
            category: DiagnosticCategory::Error,
            file_name: None,
            span: Some(first_token_span),
            related: None,
        });
    }

    fn parse_expression_or_labeled(&mut self, start: u32, modifiers: ModifierFlags) -> Stmt {
        if self.is_identifier() && self.peek_is(TokenKind::Colon) {
            let (label, _) = self.parse_identifier();
            self.expect(TokenKind::Colon);
            let body = self.parse_statement(true);
            return Stmt {
                kind: StmtKind::Labeled(Box::new(LabeledStmt {
                    label: label.into(),
                    body: Box::new(body),
                })),
                span: self.span_from(start),
            };
        }
        self.parse_expression_statement(start, modifiers)
    }

    fn parse_expression_statement(&mut self, start: u32, _modifiers: ModifierFlags) -> Stmt {
        // Recovery: when a statement starts with an invalid token followed by
        // `=`, TypeScript drops the broken LHS and keeps only the RHS
        // expression statement (e.g. `₁ = "hello"` -> `"hello";`).
        let unmatched_delimiter = matches!(
            self.cur(),
            TokenKind::CloseBracket | TokenKind::CloseParen | TokenKind::CloseBrace
        );
        if (self.at(TokenKind::Unknown) || unmatched_delimiter) && self.peek_is(TokenKind::Equals) {
            if unmatched_delimiter {
                self.error_code(1128, "Declaration or statement expected.".into());
            }
            self.bump();
            if unmatched_delimiter {
                self.error_code(1128, "Declaration or statement expected.".into());
            }
            self.bump();
            let expr = self.parse_expression();
            self.eat_semicolon();
            let span = self.span_from(expr.span.start);
            return Stmt {
                kind: StmtKind::Expr(Box::new(expr)),
                span,
            };
        }

        // Recovery: if we started on `}`, parse_expression() will consume it as
        // an unexpected token. Keep any following `;` for a separate empty
        // statement to better mirror tsc's recovery output.
        let started_on_close_brace = self.at(TokenKind::CloseBrace);
        let diags_before = self.diagnostics.len();
        let expr = self.parse_expression();
        // A STATEMENT that begins on a stray `}` is tsc's TS1128
        // "Declaration or statement expected"; the same `}` reached mid-
        // expression (`(x) => }`) stays TS1109. Rewrite the diagnostic the
        // expression fallthrough just pushed.
        if started_on_close_brace {
            if let Some(d) = self.diagnostics[diags_before..]
                .iter_mut()
                .find(|d| d.code == 1109)
            {
                d.code = 1128;
                d.message = "Declaration or statement expected.".to_string();
            }
        }
        if matches!(
            &expr.kind,
            ExprKind::Arrow(arrow) if matches!(arrow.body, ArrowBody::Block(_))
        ) && self.at(TokenKind::Question)
        {
            self.rewrite_invalid_conditional_tail_as_statement_separators();
        }
        if !self.is_on_new_line()
            && !matches!(
                self.cur(),
                TokenKind::Semicolon | TokenKind::CloseBrace | TokenKind::EndOfFile
            )
        {
            if let ExprKind::Ident(name) = &expr.kind {
                let diagnostic = match name.as_str() {
                    "interface" => Some((2427, 1438, "Interface", TokenKind::OpenBrace)),
                    "type" => Some((2457, 1439, "Type alias", TokenKind::Equals)),
                    "namespace" | "module" => Some((2819, 1437, "Namespace", TokenKind::OpenBrace)),
                    _ => None,
                };
                if let Some((invalid_code, missing_code, declaration, missing_token)) = diagnostic {
                    if self.at(missing_token) {
                        self.error_code(
                            missing_code,
                            format!("{declaration} must be given a name."),
                        );
                    } else {
                        let mut scanner = TsScanner::new(self.text(self.cur_span()));
                        scanner.scan();
                        self.error_code(
                            invalid_code,
                            format!("{declaration} name cannot be '{}'.", scanner.token_value()),
                        );
                    }
                }
            }
        }
        // The scanner deliberately splits a legacy octal before `.` / `e`
        // (`01.5` => `01`, `.5`; `01e5` => `01`, `e5`). Those adjacent
        // same-line tokens cannot be separated by ASI, so TypeScript reports
        // TS1005 and then leaves the suffix available for ordinary recovery.
        if !self.is_on_new_line()
            && !matches!(
                self.cur(),
                TokenKind::Semicolon | TokenKind::CloseBrace | TokenKind::EndOfFile
            )
            && Self::is_legacy_octal_suffix(self.cur(), self._cur_text())
            && matches!(
                &expr.kind,
                ExprKind::NumLit(raw) if Self::is_legacy_octal_literal(raw)
            )
        {
            self.error_code(1005, "';' expected.".into());
        }
        // `this.foo: any;` — a stray `:` after a complete expression
        // statement is tsc's TS1005 "';' expected."; the remainder parses
        // as its own statement.
        if self.at(TokenKind::Colon)
            && self.speculation_depth == 0
            && matches!(expr.kind, ExprKind::Member(_) | ExprKind::ArrayLit(_))
        {
            self.error_code(1005, "';' expected.".into());
            self.bump();
            // Span excludes the stray colon — the emitter slices source by
            // statement spans and must print `this.foo;`, not `this.foo:;`.
            let span = Span {
                start,
                end: expr.span.end,
            };
            return Stmt {
                kind: StmtKind::Expr(Box::new(expr)),
                span,
            };
        }
        if !started_on_close_brace {
            self.eat_semicolon();
        }
        Stmt {
            kind: StmtKind::Expr(Box::new(expr)),
            span: self.span_from(start),
        }
    }

    fn parse_block_stmt(&mut self, start: u32) -> Stmt {
        let stmts = self.parse_block_body();
        Stmt {
            kind: StmtKind::Block(stmts),
            span: self.span_from(start),
        }
    }

    fn is_legacy_octal_literal(raw: &str) -> bool {
        let bytes = raw.as_bytes();
        bytes.len() > 1 && bytes[0] == b'0' && bytes.iter().all(|byte| matches!(byte, b'0'..=b'7'))
    }

    fn is_legacy_octal_suffix(kind: TokenKind, raw: &str) -> bool {
        match kind {
            TokenKind::NumericLiteral => raw.starts_with('.'),
            TokenKind::Identifier => true,
            _ => false,
        }
    }

    fn parse_block_body(&mut self) -> Vec<Stmt> {
        self.parse_block_body_with_ambient_diagnostics(false)
    }

    fn parse_ambient_block_body(&mut self) -> Vec<Stmt> {
        self.ambient_depth += 1;
        let body = self.parse_block_body_with_ambient_diagnostics(true);
        self.ambient_depth -= 1;
        body
    }

    fn parse_block_body_with_ambient_diagnostics(
        &mut self,
        report_ambient_statements: bool,
    ) -> Vec<Stmt> {
        let opening = self.expect_opening_delimiter(TokenKind::OpenBrace);
        let mut stmts = Vec::with_capacity(4);
        let mut recovered_class_modifier = false;
        let mut ambient_statement_reported = false;
        while !self.at(TokenKind::CloseBrace) && !self.is_eof() {
            // Error recovery: when inside the outermost class body
            // (class_depth == 1) and the block has very few statements
            // (0-2), if we see `public`/`private`/`protected` on a new line
            // followed by an identifier, the current block was likely left
            // unclosed.  Break out so the class body parser picks it up.
            //
            // Guards:
            // - class_depth == 1: avoid false positives in nested classes
            //   (which can appear due to earlier error recovery)
            // - stmts.len() <= 2: blocks with many statements are likely
            //   properly structured; recovery is for short/unclosed blocks
            // - is_on_new_line: the modifier must start on a fresh line
            // - next token is an identifier: matches `public name(` patterns
            if self.class_depth == 1
                && stmts.len() <= 2
                && self.is_on_new_line()
                && matches!(
                    self.cur(),
                    TokenKind::Public | TokenKind::Private | TokenKind::Protected
                )
                && self
                    .tokens
                    .get(self.pos + 1)
                    .is_some_and(|t| t.kind == TokenKind::Identifier)
            {
                // `public name(` looks like an unclosed body followed by a
                // class method — recover by ending the block. A property
                // form (`public p1 = 0;`) is tsc's TS1128 at the modifier,
                // parsed on as plain statements.
                if !self
                    .tokens
                    .get(self.pos + 2)
                    .is_some_and(|t| t.kind == TokenKind::OpenParen)
                {
                    // Property form: tsc reports TS1128 at the modifier and
                    // STILL treats the block as ended (the stray class `}`
                    // gets its own TS1128 downstream).
                    self.error_code(1128, "Declaration or statement expected.".to_string());
                }
                recovered_class_modifier = true;
                self.block_recovery_end = Some(self.cur_span().start);
                break;
            }
            let before = self.pos;
            if report_ambient_statements {
                self.report_statement_in_ambient_context(&mut ambient_statement_reported);
            }
            stmts.push(self.parse_statement(self.current_token_starts_statement()));
            // Error recovery: if we didn't advance, skip the current token
            if self.pos == before && !self.is_eof() {
                self.error_code(1128, "Declaration or statement expected.".into());
                self.bump();
            }
        }
        if !recovered_class_modifier {
            self.expect_matching_delimiter(opening, TokenKind::OpenBrace, TokenKind::CloseBrace);
        }
        stmts
    }

    fn rewrite_invalid_conditional_tail_as_statement_separators(&mut self) {
        let mut depth = 1i32;
        let mut paren_depth = 0i32;
        let mut brace_depth = 0i32;
        let mut bracket_depth = 0i32;
        let mut i = self.pos + 1;
        while i < self.tokens.len() {
            match self.tokens[i].kind {
                TokenKind::OpenParen => paren_depth += 1,
                TokenKind::CloseParen => {
                    if paren_depth == 0 {
                        return;
                    }
                    paren_depth -= 1;
                }
                TokenKind::OpenBrace => brace_depth += 1,
                TokenKind::CloseBrace => {
                    if brace_depth == 0 {
                        return;
                    }
                    brace_depth -= 1;
                }
                TokenKind::OpenBracket => bracket_depth += 1,
                TokenKind::CloseBracket => {
                    if bracket_depth == 0 {
                        return;
                    }
                    bracket_depth -= 1;
                }
                TokenKind::Question
                    if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 =>
                {
                    depth += 1;
                }
                TokenKind::Colon if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 => {
                    depth -= 1;
                    if depth == 0 {
                        let question_start = self.tokens[self.pos].span.start;
                        let question_nl = self.tokens[self.pos].preceded_by_line_break;
                        self.replace_token(
                            self.pos,
                            Token {
                                kind: TokenKind::Semicolon,
                                span: Span::new(question_start, question_start),
                                preceded_by_line_break: question_nl,
                            },
                        );
                        let colon_start = self.tokens[i].span.start;
                        let colon_nl = self.tokens[i].preceded_by_line_break;
                        self.replace_token(
                            i,
                            Token {
                                kind: TokenKind::Semicolon,
                                span: Span::new(colon_start, colon_start),
                                preceded_by_line_break: colon_nl,
                            },
                        );
                        return;
                    }
                }
                TokenKind::Semicolon | TokenKind::EndOfFile if brace_depth == 0 => return,
                _ => {}
            }
            i += 1;
        }
    }

    fn parse_var_stmt(&mut self, modifiers: ModifierFlags, kind: VarKind, start: u32) -> Stmt {
        // skip var/let/const/using keyword
        let keyword_span = self.bump();
        let declarations = self.parse_var_declarator_list(keyword_span.end);
        if self.ambient_depth > 0 {
            for declaration in &declarations {
                if let Some(initializer) = &declaration.init {
                    let allowed_const_initializer = kind == VarKind::Const
                        && Self::ambient_const_initializer_allowed(initializer);
                    if !allowed_const_initializer {
                        self.diagnostics.push(Diagnostic {
                            code: 1039,
                            message: "Initializers are not allowed in ambient contexts."
                                .to_string(),
                            category: DiagnosticCategory::Error,
                            file_name: None,
                            span: Some(initializer.span),
                            related: None,
                        });
                    }
                }
            }
        }
        self.eat_semicolon();
        Stmt {
            kind: StmtKind::Var(Box::new(VarStmt {
                kind,
                declarations,
                modifiers,
            })),
            span: self.span_from(start),
        }
    }

    fn ambient_const_initializer_allowed(initializer: &Expr) -> bool {
        match &initializer.kind {
            ExprKind::StrLit(_)
            | ExprKind::NumLit(_)
            | ExprKind::BoolLit(_)
            | ExprKind::NoSubstTemplate(_)
            | ExprKind::Member(_)
            | ExprKind::ElemAccess(_) => true,
            ExprKind::Template(template) => template.exprs.is_empty(),
            ExprKind::Unary(unary) => matches!(
                &unary.argument.kind,
                ExprKind::NumLit(_) | ExprKind::BigIntLit(_)
            ),
            ExprKind::BigIntLit(_) => true,
            _ => false,
        }
    }

    fn parse_var_declarator_list(&mut self, first_full_start: u32) -> Vec<VarDeclarator> {
        let mark = self.scratch_decls.len();
        let mut full_start = first_full_start;
        loop {
            // Error recovery: when the next token is a statement keyword
            // (class, function), stop the var declarator list. TypeScript
            // treats `var class;` as `var ;` then `class { }`.
            let cur = self.cur();
            if matches!(
                cur,
                TokenKind::Class | TokenKind::Function | TokenKind::OpenParen
            ) {
                if self.scratch_decls.len() == mark {
                    // Need at least one declarator — create an empty one
                    let cur_span = self.cur_span();
                    let sp = Span::new(cur_span.start, cur_span.start);
                    self.scratch_decls.push(VarDeclarator {
                        name: Pat {
                            kind: PatKind::Ident("<error>".into()),
                            span: sp,
                        },
                        type_ann: None,
                        definite: false,
                        init: None,
                        full_start,
                        binding_name_full_starts: Vec::new(),
                        span: sp,
                    });
                }
                break;
            }
            let decl = self.parse_var_declarator(full_start);
            let has_initializer = decl.init.is_some();
            self.scratch_decls.push(decl);
            if has_initializer && self.at(TokenKind::Colon) {
                self.rewrite_malformed_var_initializer_arrow_tail_as_declarator_split();
            }
            let cur = self.cur();
            // Error recovery: when comma is followed by a token that can't
            // be a binding name (string/numeric literal), don't consume the
            // comma. TypeScript treats `var x = 1, "";` as `var x = 1;` + `"";`.
            if cur == TokenKind::Comma
                && self.tokens.get(self.pos + 1).is_some_and(|t| {
                    matches!(t.kind, TokenKind::StringLiteral | TokenKind::NumericLiteral)
                })
            {
                break;
            }
            if cur == TokenKind::Comma {
                full_start = self.bump().end;
                continue;
            }
            // Inside a for-loop init (`disallow_in` is set), `of` and `in`
            // are for-loop separators, not declarator names. Stop the list
            // so the for-loop parser can handle them.
            if self.disallow_in && matches!(cur, TokenKind::Of | TokenKind::In) {
                break;
            }
            // Recovery: invalid identifier escapes like `var arg\u003` are
            // scanned as `arg`, `\`, `u003`. TypeScript keeps the trailing
            // identifier in the declarator list as `var arg, u003`.
            if self.try_skip_invalid_identifier_escape_separator_in_var_decl_list() {
                self.error_code(1005, "',' expected.".into());
                full_start = self.cur_span().start;
                continue;
            }
            // Recovery: missing comma between declarators (`var a b = 1`).
            // TypeScript keeps parsing as `var a, b = 1`.
            let private_name = self.at(TokenKind::Hash)
                && self.tokens.get(self.pos + 1).is_some_and(|next| {
                    next.kind.is_identifier_name() && next.span.start == self.cur_span().end
                });
            if !self.is_on_new_line() && (self.is_identifier() || private_name) {
                self.error_code(1005, "',' expected.".into());
                full_start = self.cur_span().start;
                continue;
            }
            // A same-line literal cannot start another declarator. Report the
            // missing separator, then leave it for expression-statement recovery.
            if !self.is_on_new_line()
                && matches!(cur, TokenKind::NumericLiteral | TokenKind::StringLiteral)
            {
                self.error_code(1005, "',' expected.".into());
            }
            // Recovery: `.identifier` after type annotation → treat as `, identifier`
            // (e.g. `var v: void.x;` → `var v, x;` — the `.x` from type leaks)
            if matches!(cur, TokenKind::Dot)
                && self.tokens.get(self.pos + 1).is_some_and(|t| {
                    t.kind == TokenKind::Identifier || t.kind.is_contextual_keyword()
                })
            {
                full_start = self.bump().end; // skip `.`
                continue;
            }
            break;
        }
        take_exact(&mut self.scratch_decls, mark)
    }

    fn try_skip_invalid_identifier_escape_separator_in_var_decl_list(&mut self) -> bool {
        if self.cur() != TokenKind::Unknown
            || self.text(self.cur_span()) != "\\"
            || self.is_on_new_line()
            || !self.peek_is_ident_or_keyword()
            || self.peek_is_on_new_line()
        {
            return false;
        }
        self.bump();
        true
    }

    fn rewrite_malformed_var_initializer_arrow_tail_as_declarator_split(&mut self) {
        if !self.at(TokenKind::Colon) || self.is_on_new_line() || !self.peek_is_ident_or_keyword() {
            return;
        }
        let mut paren_depth = 0i32;
        let mut brace_depth = 0i32;
        let mut bracket_depth = 0i32;
        let mut i = self.pos + 1;
        while i < self.tokens.len() {
            match self.tokens[i].kind {
                TokenKind::OpenParen => paren_depth += 1,
                TokenKind::CloseParen => {
                    if paren_depth == 0 {
                        return;
                    }
                    paren_depth -= 1;
                }
                TokenKind::OpenBrace => brace_depth += 1,
                TokenKind::CloseBrace => {
                    if brace_depth == 0 {
                        return;
                    }
                    brace_depth -= 1;
                }
                TokenKind::OpenBracket => bracket_depth += 1,
                TokenKind::CloseBracket => {
                    if bracket_depth == 0 {
                        return;
                    }
                    bracket_depth -= 1;
                }
                TokenKind::FatArrow
                    if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 =>
                {
                    let colon_start = self.tokens[self.pos].span.start;
                    let colon_nl = self.tokens[self.pos].preceded_by_line_break;
                    self.replace_token(
                        self.pos,
                        Token {
                            kind: TokenKind::Comma,
                            span: Span::new(colon_start, colon_start),
                            preceded_by_line_break: colon_nl,
                        },
                    );
                    let fat_arrow_start = self.tokens[i].span.start;
                    let fat_arrow_nl = self.tokens[i].preceded_by_line_break;
                    self.replace_token(
                        i,
                        Token {
                            kind: TokenKind::Semicolon,
                            span: Span::new(fat_arrow_start, fat_arrow_start),
                            preceded_by_line_break: fat_arrow_nl,
                        },
                    );
                    return;
                }
                TokenKind::Comma | TokenKind::Semicolon
                    if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 =>
                {
                    return;
                }
                _ => {}
            }
            i += 1;
        }
    }

    fn parse_var_declarator(&mut self, full_start: u32) -> VarDeclarator {
        let start = self.cur_span().start;
        let binding_start_mark = self.scratch_binding_name_full_starts.len();
        let is_destructuring = matches!(self.cur(), TokenKind::OpenBrace | TokenKind::OpenBracket);
        let name = self.parse_binding_pattern_at(full_start, is_destructuring);
        let definite = self.eat(TokenKind::Excl).is_some();
        let type_ann = if self.eat(TokenKind::Colon).is_some() {
            Some(self.parse_js_checked_type_annotation())
        } else {
            None
        };
        let init = if self.eat(TokenKind::Equals).is_some() {
            let initializer = self.parse_assignment_expr();
            Some(Box::new(initializer))
        } else {
            None
        };
        let binding_name_full_starts = take_exact(
            &mut self.scratch_binding_name_full_starts,
            binding_start_mark,
        );
        VarDeclarator {
            name,
            type_ann,
            init,
            full_start,
            binding_name_full_starts,
            span: self.span_from(start),
            definite,
        }
    }

    fn parse_return_stmt(&mut self, start: u32) -> Stmt {
        self.bump(); // return
        let arg = if self.at(TokenKind::Semicolon)
            || self.at(TokenKind::CloseBrace)
            || self.is_eof()
            || self.is_on_new_line()
        {
            None
        } else {
            Some(Box::new(self.parse_expression()))
        };
        self.eat_semicolon();
        Stmt {
            kind: StmtKind::Return(arg),
            span: self.span_from(start),
        }
    }

    fn parse_if_stmt(&mut self, start: u32) -> Stmt {
        self.bump(); // if
        let opening = self.expect_opening_delimiter(TokenKind::OpenParen);
        // Error recovery: if the next token cannot be part of a condition
        // (e.g. `}` closing the enclosing block), produce an empty condition.
        let test = if self.at(TokenKind::CloseBrace) || self.at(TokenKind::CloseParen) {
            Box::new(Expr {
                kind: ExprKind::Ident("".into()),
                span: self.cur_span(),
            })
        } else {
            Box::new(self.parse_expression())
        };
        // Error recovery: if `)` is missing but `}` is present, skip the `)`.
        if !self.at(TokenKind::CloseParen) && self.at(TokenKind::CloseBrace) {
            // Don't consume `}` — it closes the enclosing block
        } else {
            self.expect_matching_delimiter(opening, TokenKind::OpenParen, TokenKind::CloseParen);
        }
        // Error recovery: if the consequent starts with `}` (closing the
        // enclosing block), produce an empty statement without consuming `}`.
        let consequent = if self.at(TokenKind::CloseBrace) {
            Box::new(Stmt {
                kind: StmtKind::Empty,
                span: Span {
                    start: self.cur_span().start,
                    end: self.cur_span().start,
                },
            })
        } else {
            Box::new(self.parse_statement(true))
        };
        let alternate = if self.eat(TokenKind::Else).is_some() {
            Some(Box::new(self.parse_statement(true)))
        } else {
            None
        };
        Stmt {
            kind: StmtKind::If(IfStmt {
                test,
                consequent,
                alternate,
            }),
            span: self.span_from(start),
        }
    }

    fn parse_while_stmt(&mut self, start: u32) -> Stmt {
        self.bump(); // while
        let opening = self.expect_opening_delimiter(TokenKind::OpenParen);
        let test = Box::new(self.parse_expression());
        self.expect_matching_delimiter(opening, TokenKind::OpenParen, TokenKind::CloseParen);
        let body = Box::new(self.parse_statement(true));
        Stmt {
            kind: StmtKind::While(WhileStmt { test, body }),
            span: self.span_from(start),
        }
    }

    fn parse_do_while_stmt(&mut self, start: u32) -> Stmt {
        self.bump(); // do
        let body = Box::new(self.parse_statement(true));
        self.expect(TokenKind::While);
        let opening = self.expect_opening_delimiter(TokenKind::OpenParen);
        let test = Box::new(self.parse_expression());
        self.expect_matching_delimiter(opening, TokenKind::OpenParen, TokenKind::CloseParen);
        self.eat_semicolon();
        Stmt {
            kind: StmtKind::DoWhile(DoWhileStmt { body, test }),
            span: self.span_from(start),
        }
    }

    fn parse_for_stmt(&mut self, start: u32) -> Stmt {
        self.bump(); // for
        let is_await = self.eat(TokenKind::Await).is_some();
        self.expect(TokenKind::OpenParen);

        // `for (await using d1 of ...)`: the `await` before `(` was consumed
        // by the `for await` pattern, but if the next token is `using` this is
        // actually `await using` (a declaration keyword), not `for await`.
        // Also handle `for (await using` when `await` is INSIDE the `(`.
        let mut await_using_in_for = false;
        if !is_await && self.at(TokenKind::Await) && self.peek_is(TokenKind::Using) {
            // `for (await using ...)` — consume `await` as part of var decl
            self.bump(); // await
            await_using_in_for = true;
        }
        // Do NOT reinterpret `for await (using ...)` as `for (await using ...)`.
        // The `await` in `for await` is the for-of-await modifier, and `using`
        // is a plain `using` declaration in the for-of loop.
        if is_await && self.at(TokenKind::Await) && self.peek_is(TokenKind::Using) {
            // `for await (await using d1 of ...)` — consume inner `await`
            // as part of `await using` declaration keyword.
            self.bump(); // inner await
            await_using_in_for = true;
        }

        // for(;;), for(init;;), for(..in..), for(..of..)
        if self.at(TokenKind::Semicolon) {
            // for (;...)
            return self.parse_for_after_init(start, None);
        }

        let is_var = matches!(
            self.cur(),
            TokenKind::Var | TokenKind::Let | TokenKind::Const | TokenKind::Using
        );
        if is_var {
            let _var_start = self.cur_span().start;
            let kind = match self.cur() {
                TokenKind::Let => VarKind::Let,
                TokenKind::Const => VarKind::Const,
                TokenKind::Using => {
                    if await_using_in_for {
                        VarKind::AwaitUsing
                    } else {
                        VarKind::Using
                    }
                }
                _ => VarKind::Var,
            };
            let keyword_span = self.bump();
            // `for (var of X)` / `for (var in X)`: `of`/`in` directly after `var`
            // is the loop keyword, not a binding name. Create an error binding.
            // Only for `var` — `let` is treated as an identifier in `for (let of ...)`.
            // Only when the token AFTER `of`/`in` is NOT one of `of`, `in`, `;`,
            // `,`, `=`, `)` — those indicate `of`/`in` is a binding name:
            //   `for (var of;;)`  → binding=of, regular for-loop
            //   `for (var of of of)` → binding=of, for-of with iterable `of`
            //   `for (var of = 0 in of)` → binding=of with init, for-in
            if kind == VarKind::Var
                && matches!(self.cur(), TokenKind::Of | TokenKind::In)
                && !self.peek_is(TokenKind::Semicolon)
                && !self.peek_is(TokenKind::Comma)
                && !self.peek_is(TokenKind::Equals)
                && !self.peek_is(TokenKind::CloseParen)
                // `for (var of of of)` → first `of` is binding, second is keyword.
                // But `for (var of of)` → first `of` is keyword (iterable=`of`).
                // Distinguish: peek is `of`/`in` followed by `)` means first `of`
                // is keyword; followed by anything else means first `of` is binding.
                && (!self.peek_is(TokenKind::Of) || self.peek2_is(TokenKind::CloseParen))
                && (!self.peek_is(TokenKind::In) || self.peek2_is(TokenKind::CloseParen))
            {
                let span = self.cur_span();
                let decls = vec![VarDeclarator {
                    name: Pat {
                        kind: PatKind::Ident("<error>".into()),
                        span,
                    },
                    type_ann: None,
                    init: None,
                    full_start: keyword_span.end,
                    binding_name_full_starts: Vec::new(),
                    span,
                    definite: false,
                }];
                let var_stmt = VarStmt {
                    kind,
                    declarations: decls,
                    modifiers: MOD_NONE,
                };
                if self.eat(TokenKind::In).is_some() {
                    let right = Box::new(self.parse_expression());
                    self.expect(TokenKind::CloseParen);
                    let body = Box::new(self.parse_statement(true));
                    return Stmt {
                        kind: StmtKind::ForIn(Box::new(ForInStmt {
                            left: ForInOfLeft::Var(var_stmt),
                            right,
                            body,
                        })),
                        span: self.span_from(start),
                    };
                }
                if self.eat(TokenKind::Of).is_some() {
                    let right = Box::new(self.parse_assignment_expr());
                    self.expect(TokenKind::CloseParen);
                    let body = Box::new(self.parse_statement(true));
                    return Stmt {
                        kind: StmtKind::ForOf(Box::new(ForOfStmt {
                            is_await,
                            left: ForInOfLeft::Var(var_stmt),
                            right,
                            body,
                        })),
                        span: self.span_from(start),
                    };
                }
                // Shouldn't reach here since we checked for Of/In above
                return self.parse_for_after_init(start, Some(ForInit::Var(var_stmt)));
            }
            let saved_disallow_in = self.disallow_in;
            self.disallow_in = true;
            let decls = self.parse_var_declarator_list(keyword_span.end);
            self.disallow_in = saved_disallow_in;
            let var_stmt = VarStmt {
                kind,
                declarations: decls,
                modifiers: MOD_NONE,
            };

            if self.eat(TokenKind::In).is_some() {
                let right = Box::new(self.parse_expression());
                self.expect(TokenKind::CloseParen);
                let body = Box::new(self.parse_statement(true));
                return Stmt {
                    kind: StmtKind::ForIn(Box::new(ForInStmt {
                        left: ForInOfLeft::Var(var_stmt),
                        right,
                        body,
                    })),
                    span: self.span_from(start),
                };
            }
            if self.eat(TokenKind::Of).is_some() {
                let right = Box::new(self.parse_assignment_expr());
                self.expect(TokenKind::CloseParen);
                let body = Box::new(self.parse_statement(true));
                return Stmt {
                    kind: StmtKind::ForOf(Box::new(ForOfStmt {
                        is_await,
                        left: ForInOfLeft::Var(var_stmt),
                        right,
                        body,
                    })),
                    span: self.span_from(start),
                };
            }

            let init = ForInit::Var(var_stmt);
            return self.parse_for_after_init(start, Some(init));
        }
        let saved_disallow_in = self.disallow_in;
        self.disallow_in = true;
        let expr = self.parse_expression();
        self.disallow_in = saved_disallow_in;
        if self.eat(TokenKind::In).is_some() {
            let right = Box::new(self.parse_expression());
            self.expect(TokenKind::CloseParen);
            let body = Box::new(self.parse_statement(true));
            return Stmt {
                kind: StmtKind::ForIn(Box::new(ForInStmt {
                    left: expr_to_for_in_of_left(expr),
                    right,
                    body,
                })),
                span: self.span_from(start),
            };
        }
        if self.eat(TokenKind::Of).is_some() {
            let right = Box::new(self.parse_assignment_expr());
            self.expect(TokenKind::CloseParen);
            let body = Box::new(self.parse_statement(true));
            return Stmt {
                kind: StmtKind::ForOf(Box::new(ForOfStmt {
                    is_await,
                    left: expr_to_for_in_of_left(expr),
                    right,
                    body,
                })),
                span: self.span_from(start),
            };
        }

        self.parse_for_after_init(start, Some(ForInit::Expr(Box::new(expr))))
    }

    fn parse_for_after_init(&mut self, start: u32, init: Option<ForInit>) -> Stmt {
        self.expect(TokenKind::Semicolon);
        let test = if !self.at(TokenKind::Semicolon) {
            Some(Box::new(self.parse_expression()))
        } else {
            None
        };
        self.expect(TokenKind::Semicolon);
        let update = if !self.at(TokenKind::CloseParen) {
            Some(Box::new(self.parse_expression()))
        } else {
            None
        };
        self.expect(TokenKind::CloseParen);
        let body = Box::new(self.parse_statement(true));
        Stmt {
            kind: StmtKind::For(Box::new(ForStmt {
                init,
                test,
                update,
                body,
            })),
            span: self.span_from(start),
        }
    }

    fn parse_switch_stmt(&mut self, start: u32) -> Stmt {
        self.bump(); // switch
        self.expect(TokenKind::OpenParen);
        let discriminant = Box::new(self.parse_expression());
        self.expect(TokenKind::CloseParen);
        self.expect(TokenKind::OpenBrace);
        let mut cases = Vec::new();
        while !self.at(TokenKind::CloseBrace) && !self.is_eof() {
            // Error recovery: if neither `case` nor `default` starts a clause,
            // check for declaration keywords that indicate the switch body
            // should close (e.g. `class D {` after unclosed switch).
            if !self.at(TokenKind::Case) && !self.at(TokenKind::Default) {
                break;
            }
            let case_start = self.cur_span().start;
            let test = if self.eat(TokenKind::Case).is_some() {
                Some(Box::new(self.parse_expression()))
            } else {
                self.expect(TokenKind::Default);
                None
            };
            self.expect(TokenKind::Colon);
            let mut consequent = Vec::new();
            while !self.at(TokenKind::Case)
                && !self.at(TokenKind::Default)
                && !self.at(TokenKind::CloseBrace)
                && !self.is_eof()
            {
                consequent.push(self.parse_statement(self.current_token_starts_statement()));
            }
            cases.push(SwitchCase {
                test,
                consequent,
                span: self.span_from(case_start),
            });
        }
        self.expect(TokenKind::CloseBrace);
        Stmt {
            kind: StmtKind::Switch(Box::new(SwitchStmt {
                discriminant,
                cases,
            })),
            span: self.span_from(start),
        }
    }

    fn parse_catch_clause(&mut self) -> CatchClause {
        let catch_start = self.cur_span().start;
        let (param, param_type) = if self.eat(TokenKind::OpenParen).is_some() {
            let pat_start = self.cur_span().start;
            let p = self.parse_binding_pattern();
            // Handle `catch (e = 1)` — catch binding with initializer (JS quirk / error recovery).
            let p = if self.eat(TokenKind::Equals).is_some() {
                let init = self.parse_assignment_expr();
                Pat {
                    kind: PatKind::Assign(Box::new(p), Box::new(init)),
                    span: self.span_from(pat_start),
                }
            } else {
                p
            };
            let t = if self.eat(TokenKind::Colon).is_some() {
                Some(self.parse_js_checked_type_annotation())
            } else {
                None
            };
            self.expect(TokenKind::CloseParen);
            (Some(p), t)
        } else {
            (None, None)
        };
        let body = self.parse_block_body();
        CatchClause {
            param,
            param_type,
            body,
            span: self.span_from(catch_start),
        }
    }

    fn parse_try_stmt(&mut self, start: u32) -> Stmt {
        self.bump(); // try
        let block = self.parse_block_body();
        let handler = if self.eat(TokenKind::Catch).is_some() {
            Some(self.parse_catch_clause())
        } else {
            None
        };
        let finalizer = if self.eat(TokenKind::Finally).is_some() {
            Some(self.parse_block_body())
        } else if handler.is_none() {
            // Error recovery: `try { }` without catch or finally →
            // synthesize empty `finally { }`
            Some(Vec::new())
        } else {
            None
        };
        Stmt {
            kind: StmtKind::Try(Box::new(TryStmt {
                block,
                handler,
                finalizer,
            })),
            span: self.span_from(start),
        }
    }

    fn parse_throw_stmt(&mut self, start: u32) -> Stmt {
        self.bump(); // throw
                     // `throw` has [no LineTerminator here] before the expression.
                     // If there's a newline (or EOF/semicolon), treat as `throw;` (error recovery).
        let expr = if self.at(TokenKind::Semicolon) || self.is_eof() || self.is_on_new_line() {
            // Missing throw expression - create a synthetic empty ident
            let pos = self.cur_span().start;
            Box::new(Expr {
                kind: ExprKind::Ident("".into()),
                span: Span {
                    start: pos,
                    end: pos,
                },
            })
        } else {
            Box::new(self.parse_expression())
        };
        self.eat_semicolon();
        Stmt {
            kind: StmtKind::Throw(expr),
            span: self.span_from(start),
        }
    }

    fn parse_with_stmt(&mut self, start: u32) -> Stmt {
        self.bump(); // with
        let opening = self.expect_opening_delimiter(TokenKind::OpenParen);
        let object = Box::new(self.parse_expression());
        let close =
            self.expect_matching_delimiter(opening, TokenKind::OpenParen, TokenKind::CloseParen);
        if self.with_depth == 0 {
            self.diagnostics.push(Diagnostic {
                code: 2410,
                message:
                    "The 'with' statement is not supported. All symbols in a 'with' block will have type 'any'."
                        .to_string(),
                category: DiagnosticCategory::Error,
                file_name: None,
                span: Some(Span::new(start, close.end)),
                related: None,
            });
        }
        self.with_depth += 1;
        let body = Box::new(self.parse_statement(true));
        self.with_depth -= 1;
        Stmt {
            kind: StmtKind::With(Box::new(WithStmt { object, body })),
            span: self.span_from(start),
        }
    }

    // -----------------------------------------------------------------------
    // Function declaration
    // -----------------------------------------------------------------------

    fn parse_function_decl(
        &mut self,
        modifiers: ModifierFlags,
        decorators: Vec<Expr>,
        start: u32,
    ) -> Stmt {
        let is_async = modifiers & MOD_ASYNC != 0;
        if self.at(TokenKind::Async) {
            self.bump();
        }
        self.expect(TokenKind::Function);
        let generator_token = self.eat(TokenKind::Asterisk);
        let is_generator = generator_token.is_some();
        if let Some(star) = generator_token {
            if self.ambient_depth > 0 || modifiers & MOD_DECLARE != 0 {
                self.diagnostics.push(Diagnostic {
                    code: 1221,
                    message: "Generators are not allowed in an ambient context.".to_string(),
                    category: DiagnosticCategory::Error,
                    file_name: None,
                    span: Some(star),
                    related: None,
                });
            }
        }
        let (name, name_span) = if self.is_identifier() {
            let (n, s) = self.parse_identifier();
            (Some(n), Some(s))
        } else {
            (None, None)
        };
        let type_params = self.try_parse_js_checked_type_params();
        // Error recovery: `function* gen { }` — missing parens.
        // When `{` follows a generator function name directly, skip param
        // parsing and use empty params. Only for generators to avoid
        // breaking `function boo { static test() }` patterns.
        let params = if self.at(TokenKind::OpenBrace) && name.is_some() && is_generator {
            Vec::new()
        } else {
            self.parse_param_list()
        };
        // A predicate with no type followed directly by a return block,
        // `function f(x): x is { return true; }`, abandons the function
        // declaration in TypeScript recovery and reparses the block contents
        // as top-level statements.
        if self.at(TokenKind::Colon)
            && self
                .tokens
                .get(self.pos + 1)
                .is_some_and(|token| token.kind.is_identifier_name())
            && self
                .tokens
                .get(self.pos + 2)
                .is_some_and(|token| token.kind == TokenKind::Is)
            && self.tokens.get(self.pos + 3).is_some_and(|token| {
                token.kind == TokenKind::OpenBrace
                    && self
                        .tokens
                        .get(self.pos + 4)
                        .is_some_and(|next| next.kind == TokenKind::Return)
                    && !self
                        .tokens
                        .get(self.pos + 5)
                        .is_some_and(|next| next.kind == TokenKind::Colon)
            })
        {
            self.bump(); // :
            self.bump(); // predicate parameter
            self.bump(); // is
            let body = self.parse_block_body();
            return Stmt {
                kind: StmtKind::Block(body),
                span: self.span_from(start),
            };
        }
        let return_type = if self.eat(TokenKind::Colon).is_some() {
            Some(self.parse_js_checked_return_type_annotation())
        } else {
            None
        };
        let body = if self.at(TokenKind::OpenBrace) {
            let brace = self.cur_span();
            if self.ambient_depth > 0 || modifiers & MOD_DECLARE != 0 {
                self.diagnostics.push(Diagnostic {
                    code: 1183,
                    message: "An implementation cannot be declared in ambient contexts."
                        .to_string(),
                    category: DiagnosticCategory::Error,
                    file_name: None,
                    span: Some(brace),
                    related: None,
                });
            }
            Some(self.parse_block_body())
        } else if self.at(TokenKind::FatArrow) && name.is_some() && params.is_empty() {
            // Error recovery: `function f() => expr` with no params.
            // Keep the function with empty body; skip `=>` so the
            // expression becomes a separate statement.
            // Only for empty params to avoid breaking `function f(x)=>expr`
            // patterns in other contexts.
            self.bump(); // skip `=>`
            Some(Vec::new())
        } else {
            self.eat_semicolon();
            None
        };
        if body.is_none() {
            self.check_signature_parameter_defaults(&params);
        }
        Stmt {
            kind: StmtKind::FnDecl(Box::new(FnDecl {
                name: name.map(Into::into),
                name_span,
                type_params,
                params,
                return_type,
                body,
                modifiers,
                is_generator,
                is_async,
                decorators,
                span: self.span_from(start),
            })),
            span: self.span_from(start),
        }
    }

    // -----------------------------------------------------------------------
    // Class declaration
    // -----------------------------------------------------------------------

    fn parse_class_decl(
        &mut self,
        modifiers: ModifierFlags,
        decorators: Vec<Expr>,
        start: u32,
    ) -> Stmt {
        if self.at(TokenKind::Abstract) {
            self.bump();
        }
        self.expect(TokenKind::Class);
        let (name, name_span) = if self.is_identifier() {
            // `implements` after `class` is treated as the heritage clause keyword
            // (not the class name) when followed by something other than `{`, `extends`, or `implements`.
            // E.g. `class implements number {}` → name=None, implements=[number]
            //       `class implements {}` → name="implements"
            if self.at(TokenKind::Implements)
                && !self.peek_is(TokenKind::OpenBrace)
                && !self.peek_is(TokenKind::Extends)
                && !self.peek_is(TokenKind::Implements)
            {
                (None, None)
            } else {
                let (n, s) = self.parse_identifier();
                (Some(n), Some(s))
            }
        } else {
            (None, None)
        };
        let type_params = self.try_parse_js_checked_type_params();
        let (mut extends, mut extends_type_args) = if self.eat(TokenKind::Extends).is_some() {
            // Error recovery: `class C extends {}` — `{` after `extends` is the
            // class body, not an object literal. Keep extends with empty expression.
            // `class C extends implements` — `implements` is the heritage clause.
            // Only treat `{` as class body when it's `{}` (empty) — `{ foo }` is
            // a real extends expression (object type that gets erased).
            let is_empty_brace =
                self.at(TokenKind::OpenBrace) && self.peek_is(TokenKind::CloseBrace);
            if is_empty_brace || self.at(TokenKind::Implements) {
                let pos = self.cur_span().start;
                let empty = Expr {
                    kind: ExprKind::Ident(AstString::new("")),
                    span: Span::new(pos, pos),
                };
                (Some(Box::new(empty)), None)
            } else {
                let expr = self.parse_left_hand_side_expr();
                let ta = self.try_parse_js_checked_type_args();
                (Some(Box::new(expr)), ta)
            }
        } else {
            (None, None)
        };
        // `class C extends await<T> {}` in an await context is recovered by
        // TypeScript as `class C extends T {}`: the invalid `await<` prefix
        // and closing `>` are discarded, while the type argument becomes the
        // heritage expression.
        if matches!(extends.as_deref().map(|expr| &expr.kind), Some(ExprKind::Ident(name)) if name == "await")
            && extends_type_args
                .as_ref()
                .is_some_and(|args| args.len() == 1)
        {
            let ty = extends_type_args.as_ref().unwrap()[0].clone();
            let recovered = match ty.kind {
                TypeNodeKind::Reference(type_ref) if type_ref.type_args.is_none() => *type_ref.name,
                _ => Expr {
                    kind: ExprKind::Ident(self.text(ty.span).into()),
                    span: ty.span,
                },
            };
            extends = Some(Box::new(recovered));
            extends_type_args = None;
        }
        let implements = if let Some(implements_token) = self.eat(TokenKind::Implements) {
            let types = self.parse_heritage_type_list();
            if self.is_js_file {
                self.error_at_span(
                    8005,
                    "'implements' clauses can only be used in TypeScript files.".to_string(),
                    self.span_from(implements_token.start),
                );
            }
            types
        } else {
            Vec::new()
        };
        // Error recovery: `class D implements C extends C { }` — implements before extends
        if extends.is_none() && self.eat(TokenKind::Extends).is_some() {
            let expr = self.parse_left_hand_side_expr();
            let ta = self.try_parse_js_checked_type_args();
            extends = Some(Box::new(expr));
            extends_type_args = ta;
        }
        self.skip_extra_class_heritage_clauses();
        let establish_ambient_class = modifiers & MOD_DECLARE != 0 && self.ambient_depth == 0;
        if establish_ambient_class {
            self.ambient_depth += 1;
        }
        let members = self.parse_class_body(modifiers & MOD_ABSTRACT != 0);
        if establish_ambient_class {
            self.ambient_depth -= 1;
        }
        Stmt {
            kind: StmtKind::ClassDecl(Box::new(ClassDecl {
                name: name.map(Into::into),
                name_span,
                type_params,
                extends,
                extends_type_args,
                implements,
                members,
                modifiers,
                decorators,
                span: self.span_from(start),
            })),
            span: self.span_from(start),
        }
    }

    fn skip_extra_class_heritage_clauses(&mut self) {
        while !self.at(TokenKind::OpenBrace) && !self.is_eof() {
            let consumed_sep =
                self.eat(TokenKind::Extends).is_some() || self.eat(TokenKind::Comma).is_some();
            if !consumed_sep {
                break;
            }
            let before = self.pos;
            let _expr = self.parse_left_hand_side_expr();
            let _type_args = self.try_parse_type_args();
            if self.pos == before {
                break;
            }
        }
    }

    fn parse_class_body(&mut self, is_abstract: bool) -> Vec<ClassMember> {
        self.class_depth += 1;
        let result = self.parse_class_body_inner(is_abstract);
        self.class_depth -= 1;
        result
    }

    fn parse_class_body_inner(&mut self, is_abstract: bool) -> Vec<ClassMember> {
        if self.eat(TokenKind::OpenBrace).is_none() {
            self.error_code(1005, "'{' expected.".into());
            if !self.recover_to_class_body_start() {
                return Vec::new();
            }
        }
        let mut members = Vec::new();
        while !self.at(TokenKind::CloseBrace) && !self.is_eof() {
            if self.eat(TokenKind::Semicolon).is_some() {
                members.push(ClassMember {
                    kind: ClassMemberKind::SemicolonClassElement,
                    span: self.span_from(self.cur_span().start),
                });
                continue;
            }
            // Error recovery: `enum Name {` or `namespace Name {` at the
            // start of a class member should close the class body. These
            // are declaration keywords followed by an identifier and `{`,
            // indicating a new declaration, not a class member.
            if matches!(
                self.cur(),
                TokenKind::Enum | TokenKind::Namespace | TokenKind::Module
            ) && self
                .tokens
                .get(self.pos + 1)
                .is_some_and(|t| t.kind == TokenKind::Identifier)
                && self
                    .tokens
                    .get(self.pos + 2)
                    .is_some_and(|t| t.kind == TokenKind::OpenBrace)
            {
                break;
            }
            let before = self.pos;
            if let Some(member) = self.parse_class_member(is_abstract) {
                members.push(member);
            }
            if self.abandon_class_body {
                self.abandon_class_body = false;
                break;
            }
            // Error recovery: if we didn't advance, skip the current token to avoid
            // an infinite loop. This mirrors the guard in parse_source_elements().
            if self.pos == before && !self.is_eof() {
                self.error_code(
                    1068,
                    "Unexpected token. A constructor, method, accessor, or property was expected."
                        .into(),
                );
                self.bump();
            }
        }
        self.expect(TokenKind::CloseBrace);
        members
    }

    fn recover_to_class_body_start(&mut self) -> bool {
        while !self.at(TokenKind::OpenBrace) && !self.is_eof() {
            match self.cur() {
                TokenKind::Semicolon
                | TokenKind::OpenParen
                | TokenKind::CloseBrace
                | TokenKind::Function
                | TokenKind::Class
                | TokenKind::Interface
                | TokenKind::Type
                | TokenKind::Enum
                | TokenKind::Namespace
                | TokenKind::Module
                | TokenKind::Var
                | TokenKind::Let
                | TokenKind::Const
                | TokenKind::Using
                | TokenKind::If
                | TokenKind::While
                | TokenKind::Do
                | TokenKind::For
                | TokenKind::Switch
                | TokenKind::Try
                | TokenKind::Throw
                | TokenKind::Return
                | TokenKind::Break
                | TokenKind::Continue
                | TokenKind::Debugger
                | TokenKind::With
                | TokenKind::Export
                | TokenKind::Import
                | TokenKind::At
                // Expression keywords that start statements should stop
                // recovery (e.g. `class void {}` → void is a separate stmt)
                | TokenKind::Void
                | TokenKind::Delete
                | TokenKind::TypeOf => return false,
                TokenKind::Await if self.peek_is(TokenKind::Using) => return false,
                TokenKind::Async if self.peek_is(TokenKind::Function) => return false,
                TokenKind::Abstract if self.peek_is(TokenKind::Class) => return false,
                _ => {
                    self.bump();
                }
            }
        }
        self.eat(TokenKind::OpenBrace).is_some()
    }

    fn parse_class_member(&mut self, is_abstract: bool) -> Option<ClassMember> {
        let mut modifiers = 0..0;
        let member = self.parse_class_member_inner(&mut modifiers)?;
        if !self.is_js_file {
            self.check_class_member_modifiers(&member, modifiers, is_abstract);
        }
        Some(member)
    }

    fn parse_class_member_inner(
        &mut self,
        modifier_range: &mut std::ops::Range<usize>,
    ) -> Option<ClassMember> {
        let start = self.cur_span().start;
        let mut decorators = if self.at(TokenKind::At) {
            self.parse_decorators()
        } else {
            Vec::new()
        };
        let mut recovered_name_from_decorator: Option<PropName> = None;
        let mut recovered_empty_param_list_from_decorator = false;
        if let Some(last) = decorators.last_mut() {
            // Recovery: `@dec [computed]` in class members should be parsed as
            // decorator `@dec` plus member name `[computed]`. Without this,
            // the decorator parser can absorb the computed name as `@dec[...]`.
            let looks_like_member_sig = self.at(TokenKind::Colon)
                || self.at(TokenKind::Question)
                || self.at(TokenKind::Equals)
                || self.at(TokenKind::OpenParen)
                || self.at(TokenKind::LessThan)
                || self.at(TokenKind::OpenBrace)
                || self.at(TokenKind::Semicolon);
            if looks_like_member_sig {
                if let Some((decorator_expr, recovered_name, consumed_empty_params)) =
                    Self::split_decorator_consumed_member_name(last)
                {
                    *last = decorator_expr;
                    recovered_name_from_decorator = Some(recovered_name);
                    recovered_empty_param_list_from_decorator = consumed_empty_params;
                }
            }
            *last = Self::normalize_invalid_await_decorator(last.clone());
        }
        let modifier_pos = self.pos;
        let modifiers = self.parse_member_modifiers();
        *modifier_range = modifier_pos..self.pos;
        if self.is_js_file {
            for index in modifier_pos..self.pos {
                let token = &self.tokens[index];
                let name = match token.kind {
                    TokenKind::Public => Some("public"),
                    TokenKind::Private => Some("private"),
                    TokenKind::Protected => Some("protected"),
                    TokenKind::Readonly => Some("readonly"),
                    TokenKind::Abstract => Some("abstract"),
                    TokenKind::Override => Some("override"),
                    TokenKind::Declare => Some("declare"),
                    TokenKind::Const => Some("const"),
                    TokenKind::Export => Some("export"),
                    _ => None,
                };
                if let Some(name) = name {
                    self.error_at_span(
                        8009,
                        format!("The '{name}' modifier can only be used in TypeScript files."),
                        token.span,
                    );
                }
            }
        }

        // Static block
        if modifiers & MOD_STATIC != 0 && self.at(TokenKind::OpenBrace) {
            if let Some(decorator) = decorators.first() {
                let span = self.decorator_diagnostic_span(decorator, self.is_js_file);
                self.error_at_span(1206, "Decorators are not valid here.".to_string(), span);
            }
            let body = self.parse_block_body();
            return Some(ClassMember {
                kind: ClassMemberKind::StaticBlock(body),
                span: self.span_from(start),
            });
        }

        // Constructor
        if self.at(TokenKind::Constructor) && !self.peek_is(TokenKind::Colon) {
            return Some(self.parse_class_constructor(modifiers, decorators, start));
        }

        // Index signature: [key: type]: type
        if self.at(TokenKind::OpenBracket) && self.is_index_signature() {
            let signature = self.parse_index_signature(start, modifiers);
            return Some(ClassMember {
                kind: ClassMemberKind::IndexSignature(signature),
                span: self.span_from(start),
            });
        }

        // Get/Set accessor
        if (self.at(TokenKind::Get) || self.at(TokenKind::Set)) && self.peek_could_start_prop_name()
        {
            let is_get = self.at(TokenKind::Get);
            self.bump();
            // TODO: Handle failure here? But peek check makes it safe?
            let name = self
                .parse_property_name()
                .unwrap_or(PropName::Ident("<error>".into(), self.cur_span()));
            if is_get {
                return Some(self.parse_get_accessor(modifiers, decorators, name, start));
            } else {
                return Some(self.parse_set_accessor(modifiers, decorators, name, start));
            }
        }

        // Method or property
        let generator_token = self.eat(TokenKind::Asterisk);
        let is_generator = generator_token.is_some();
        if let Some(star) = generator_token {
            if self.ambient_depth > 0 {
                self.diagnostics.push(Diagnostic {
                    code: 1221,
                    message: "Generators are not allowed in an ambient context.".to_string(),
                    category: DiagnosticCategory::Error,
                    file_name: None,
                    span: Some(star),
                    related: None,
                });
            }
        }
        let name = if let Some(n) = recovered_name_from_decorator {
            n
        } else if let Some(n) = self.parse_property_name() {
            n
        } else {
            // Failed to parse name.
            // If we had modifiers, emit error.
            if modifiers != MOD_NONE || !decorators.is_empty() {
                if self.at(TokenKind::OpenBrace) && self.speculation_depth == 0 {
                    // tsc: `public {` gets "Declaration expected." and the
                    // class body is ABANDONED — the `{...}` re-parses as a
                    // block statement after the (force-closed) class.
                    self.error_code(1146, "Declaration expected.".into());
                    self.abandon_class_body = true;
                } else {
                    self.error_code(1003, "Identifier expected.".into());
                }
            }
            // Return None so caller can try recovery
            return None;
        };

        let optional_token = self.eat(TokenKind::Question);
        let optional = optional_token.is_some();
        if self.is_js_file {
            if let Some(span) = optional_token {
                self.error_at_span(
                    8009,
                    "The '?' modifier can only be used in TypeScript files.".to_string(),
                    span,
                );
            }
        }

        if is_generator
            || self.at(TokenKind::OpenParen)
            || self.at(TokenKind::LessThan)
            || recovered_empty_param_list_from_decorator
        {
            // Method
            let type_params = if recovered_empty_param_list_from_decorator {
                None
            } else {
                self.try_parse_js_checked_type_params()
            };
            let params = if recovered_empty_param_list_from_decorator {
                Vec::new()
            } else {
                self.parse_param_list()
            };
            // Handle `x()?: Type` — optional method return type (error recovery).
            // Consume `?:` and parse the type annotation so the method is properly
            // recognized even with the invalid optional syntax.
            let mut optional_after_params = false;
            let return_type = if self.eat(TokenKind::Colon).is_some() {
                Some(self.parse_js_checked_return_type_annotation())
            } else if self.at(TokenKind::Question)
                && self
                    .tokens
                    .get(self.pos + 1)
                    .is_some_and(|t| t.kind == TokenKind::Colon)
            {
                optional_after_params = true;
                self.bump(); // consume `?`
                self.bump(); // consume `:`
                Some(self.parse_js_checked_return_type_annotation())
            } else {
                None
            };
            let body = if self.at(TokenKind::OpenBrace) {
                self.block_recovery_end = None;
                Some(self.parse_block_body())
            } else if optional_after_params {
                self.eat_semicolon();
                Some(Vec::new())
            } else {
                self.eat_semicolon();
                None
            };
            if body.is_none() {
                self.check_signature_parameter_defaults(&params);
            }
            let is_async = modifiers & MOD_ASYNC != 0;
            // When block body recovery closed the body early (unclosed method
            // terminated by a class modifier), extend the span to cover the
            // whitespace/newlines so the emitter formats a multi-line body.
            let mut span = self.span_from(start);
            if let Some(end) = self.block_recovery_end.take() {
                if end > span.end {
                    span = Span::new(span.start, end);
                }
            }
            Some(ClassMember {
                kind: ClassMemberKind::Method(ClassMethod {
                    name,
                    type_params,
                    params,
                    return_type,
                    body,
                    modifiers,
                    is_generator,
                    is_async,
                    optional,
                    decorators,
                }),
                span,
            })
        } else {
            // Property
            let definite = self.eat(TokenKind::Excl).is_some();
            let type_ann = if self.eat(TokenKind::Colon).is_some() {
                Some(self.parse_js_checked_type_annotation())
            } else {
                None
            };
            let initializer = if self.eat(TokenKind::Equals).is_some() {
                Some(Box::new(self.parse_assignment_expr()))
            } else {
                None
            };
            if type_ann.is_none()
                && initializer.is_none()
                && !optional
                && !definite
                && matches!(name, PropName::Ident(_, _))
                && self.is_identifier()
                && !self.is_on_new_line()
            {
                self.error_at_span(
                    1434,
                    "Unexpected keyword or identifier.".into(),
                    name.span(),
                );
            }
            self.eat_semicolon();
            Some(ClassMember {
                kind: ClassMemberKind::Property(ClassProp {
                    name,
                    type_ann,
                    initializer,
                    modifiers,
                    optional,
                    definite,
                    decorators,
                }),
                span: self.span_from(start),
            })
        }
    }

    fn parse_class_constructor(
        &mut self,
        modifiers: ModifierFlags,
        decorators: Vec<Expr>,
        start: u32,
    ) -> ClassMember {
        let keyword_span = self.cur_span();
        self.bump(); // constructor
                     // Recovery: constructors cannot have type parameters, but tsc still
                     // consumes them so the following parameter list/body stay attached.
        let type_parameter_open = self.cur_span();
        let type_parameters = self.try_parse_js_checked_type_params();
        let type_parameter_range = type_parameters.as_ref().map(|parameters| {
            let range = if parameters.is_empty() {
                Span::new(type_parameter_open.end, type_parameter_open.end)
            } else {
                // Nested type references can split a >> token in place. Rescan
                // this invalid list to retain consumed closers and exclude
                // trailing trivia from the diagnostic span.
                let close_start = self.tokens[self.pos - 1].span.start;
                let contents = &self.source[type_parameter_open.end as usize..close_start as usize];
                let end = Scanner::new(contents)
                    .scan_all()
                    .into_iter()
                    .filter(|token| token.kind != TokenKind::EndOfFile)
                    .last()
                    .map_or(0, |token| token.span.end);
                Span::new(parameters[0].span.start, type_parameter_open.end + end)
            };
            (range, self.span_from(type_parameter_open.start))
        });
        let params = self.parse_param_list();
        // Recovery: constructors also can't have return types. Consume a stray
        // type annotation so the body isn't emitted as trailing class noise.
        let return_type = if self.eat(TokenKind::Colon).is_some()
            && !matches!(
                self.cur(),
                TokenKind::OpenBrace | TokenKind::Semicolon | TokenKind::CloseBrace
            ) {
            Some(self.parse_return_type())
        } else {
            None
        };
        let body = if self.at(TokenKind::OpenBrace) {
            Some(self.parse_block_body())
        } else {
            self.eat_semicolon();
            None
        };
        if body.is_none() {
            self.check_signature_parameter_defaults(&params);
        }
        if !self.is_js_file {
            if let Some((range, list_span)) = type_parameter_range {
                if range.start == range.end {
                    self.grammar_error_at_span(
                        1098,
                        "Type parameter list cannot be empty.".into(),
                        list_span,
                    );
                }
                self.grammar_error_at_span(
                    1092,
                    "Type parameters cannot appear on a constructor declaration.".into(),
                    range,
                );
            } else if let Some(return_type) = return_type {
                self.grammar_error_at_span(
                    1093,
                    "Type annotation cannot appear on a constructor declaration.".into(),
                    return_type.span,
                );
            }
        }
        ClassMember {
            kind: ClassMemberKind::Constructor(ClassConstructor {
                keyword_span,
                params,
                body,
                modifiers,
                decorators,
            }),
            span: self.span_from(start),
        }
    }

    fn split_decorator_consumed_member_name(decorator: &Expr) -> Option<(Expr, PropName, bool)> {
        match &decorator.kind {
            ExprKind::ElemAccess(ea) if !ea.optional => Some((
                Self::with_decorator_start((*ea.object).clone(), decorator.span.start),
                PropName::Computed(Box::new((*ea.index).clone()), ea.index.span),
                false,
            )),
            ExprKind::Member(mem) if !mem.optional => Some((
                Self::with_decorator_start((*mem.object).clone(), decorator.span.start),
                PropName::Ident(mem.property.clone(), decorator.span),
                false,
            )),
            ExprKind::Call(call) if !call.optional && call.args.is_empty() => {
                match &call.callee.kind {
                    ExprKind::ElemAccess(ea) if !ea.optional => Some((
                        Self::with_decorator_start((*ea.object).clone(), decorator.span.start),
                        PropName::Computed(Box::new((*ea.index).clone()), ea.index.span),
                        true,
                    )),
                    ExprKind::Member(mem) if !mem.optional => Some((
                        Self::with_decorator_start((*mem.object).clone(), decorator.span.start),
                        PropName::Ident(mem.property.clone(), call.callee.span),
                        true,
                    )),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn with_decorator_start(mut expression: Expr, start: u32) -> Expr {
        expression.span.start = start;
        expression
    }

    /// Recover a destructured parameter name swallowed by decorator member
    /// access, e.g. `method(@dec [x])` is decorator `@dec` plus parameter
    /// binding `[x]`, not decorator expression `@dec[x]` with no parameter.
    fn split_decorator_consumed_parameter_name(decorator: &Expr) -> Option<(Expr, Pat)> {
        let ExprKind::ElemAccess(access) = &decorator.kind else {
            return None;
        };
        if access.optional {
            return None;
        }
        let element = expr_to_pat(&access.index);
        let name = Pat {
            kind: PatKind::Array(vec![Some(ArrayPatElem::Pat(element))]),
            span: access.index.span,
        };
        Some((
            Self::with_decorator_start((*access.object).clone(), decorator.span.start),
            name,
        ))
    }

    fn parse_get_accessor(
        &mut self,
        modifiers: ModifierFlags,
        decorators: Vec<Expr>,
        name: PropName,
        start: u32,
    ) -> ClassMember {
        let type_params = self.try_parse_js_checked_type_params();
        let params = self.parse_param_list();
        let return_type = if self.eat(TokenKind::Colon).is_some() {
            Some(self.parse_js_checked_return_type_annotation())
        } else {
            None
        };
        let body = if self.at(TokenKind::OpenBrace) {
            Some(self.parse_block_body())
        } else {
            self.eat_semicolon();
            None
        };
        if body.is_none() {
            self.check_signature_parameter_defaults(&params);
        }
        ClassMember {
            kind: ClassMemberKind::GetAccessor(ClassAccessor {
                name,
                type_params,
                params,
                return_type,
                body,
                modifiers,
                decorators,
            }),
            span: self.span_from(start),
        }
    }

    fn parse_set_accessor(
        &mut self,
        modifiers: ModifierFlags,
        decorators: Vec<Expr>,
        name: PropName,
        start: u32,
    ) -> ClassMember {
        let type_params = self.try_parse_js_checked_type_params();
        let params = self.parse_param_list();
        let return_type = if self.eat(TokenKind::Colon).is_some() {
            Some(self.parse_js_checked_return_type_annotation())
        } else {
            None
        };
        let body = if self.at(TokenKind::OpenBrace) {
            Some(self.parse_block_body())
        } else {
            self.eat_semicolon();
            None
        };
        if body.is_none() {
            self.check_signature_parameter_defaults(&params);
        }
        ClassMember {
            kind: ClassMemberKind::SetAccessor(ClassAccessor {
                name,
                type_params,
                params,
                return_type,
                body,
                modifiers,
                decorators,
            }),
            span: self.span_from(start),
        }
    }

    // -----------------------------------------------------------------------
    // Interface declaration
    // -----------------------------------------------------------------------

    fn parse_interface_decl(&mut self, modifiers: ModifierFlags, start: u32) -> Stmt {
        self.bump(); // interface
        let (name, name_span) = self.parse_identifier();
        if self.is_js_file {
            self.error_at_span(
                8006,
                "'interface' declarations can only be used in TypeScript files.".to_string(),
                name_span,
            );
        }
        let type_params = self.try_parse_type_params();
        let extends = if self.eat(TokenKind::Extends).is_some() {
            self.parse_type_list()
        } else {
            Vec::new()
        };
        let members = self.parse_object_type_members();
        Stmt {
            kind: StmtKind::InterfaceDecl(Box::new(InterfaceDecl {
                name: name.into(),
                name_span: Some(name_span),
                type_params,
                extends,
                members,
                modifiers,
                span: self.span_from(start),
            })),
            span: self.span_from(start),
        }
    }

    fn parse_object_type_members(&mut self) -> Vec<TypeMember> {
        self.expect(TokenKind::OpenBrace);
        let mut members = Vec::new();
        while !self.at(TokenKind::CloseBrace) && !self.is_eof() {
            let before = self.pos;
            let was_index_signature = self.at(TokenKind::OpenBracket) && self.is_index_signature();
            members.push(self.parse_type_member(true));
            // A type predicate is not permitted as an index-signature value
            // type.  Keep the rejected `is Type` tail outside the interface,
            // matching TypeScript's declaration-list recovery, rather than
            // absorbing both identifiers as additional interface members.
            if was_index_signature && self.at(TokenKind::Is) {
                break;
            }
            // eat ; or ,
            if !self.at(TokenKind::CloseBrace) && self.eat(TokenKind::Semicolon).is_none() {
                self.eat(TokenKind::Comma);
            }
            // Error recovery: if we didn't advance, skip the current token to avoid
            // an infinite loop.
            if self.pos == before && !self.is_eof() {
                self.error_code(1131, "Property or signature expected.".into());
                self.bump();
            }
        }
        self.expect(TokenKind::CloseBrace);
        members
    }

    fn parse_type_member(&mut self, in_interface: bool) -> TypeMember {
        let start = self.cur_span().start;
        // `readonly` is a modifier only when what follows can start a property
        // name (or an index signature `[`). When it is followed by `?`, `:`,
        // `(`, `<`, `,`, `;` or `}`, `readonly` is itself the member name — e.g.
        // `interface I { readonly?: boolean }`. Same disambiguation as get/set
        // below. Without this guard the modifier is eaten and the following `?`
        // trips "expected type member name" (TS1002).
        let readonly = if self.at(TokenKind::Readonly) && self.peek_could_start_prop_name() {
            self.bump();
            true
        } else {
            false
        };

        // Index signature: [key: type]: type
        if self.at(TokenKind::OpenBracket) && self.is_index_signature() {
            return self.parse_index_signature_member(start, readonly);
        }

        // Call/construct signature
        if self.at(TokenKind::OpenParen) || self.at(TokenKind::LessThan) {
            let type_params = self.try_parse_type_params();
            let params = self.parse_param_list();
            self.check_signature_parameter_defaults(&params);
            let return_type = if self.eat(TokenKind::Colon).is_some() {
                Some(self.parse_return_type())
            } else {
                None
            };
            return TypeMember {
                kind: TypeMemberKind::CallSig(CallSig {
                    type_params,
                    params,
                    return_type,
                }),
                span: self.span_from(start),
            };
        }

        if self.at(TokenKind::New)
            && (self.peek_is(TokenKind::OpenParen) || self.peek_is(TokenKind::LessThan))
        {
            self.bump();
            let type_params = self.try_parse_type_params();
            let params = self.parse_param_list();
            self.check_signature_parameter_defaults(&params);
            let return_type = if self.eat(TokenKind::Colon).is_some() {
                Some(self.parse_return_type())
            } else {
                None
            };
            return TypeMember {
                kind: TypeMemberKind::ConstructSig(ConstructSig {
                    type_params,
                    params,
                    return_type,
                }),
                span: self.span_from(start),
            };
        }

        // Get/set accessor signature
        if (self.at(TokenKind::Get) || self.at(TokenKind::Set)) && self.peek_could_start_prop_name()
        {
            let is_get = self.at(TokenKind::Get);
            self.bump();
            let name = self
                .parse_property_name()
                .unwrap_or(PropName::Ident("<error>".into(), self.cur_span()));
            let params = self.parse_param_list();
            let return_type = if self.eat(TokenKind::Colon).is_some() {
                Some(self.parse_return_type())
            } else {
                None
            };
            // Error recovery: if there's a body `{ ... }` after the accessor
            // signature in a type context, skip it. Accessor bodies are invalid
            // in type/interface positions but TypeScript still parses them.
            if self.at(TokenKind::OpenBrace) {
                self.skip_block();
            } else {
                self.check_signature_parameter_defaults(&params);
            }
            let kind = if is_get {
                TypeMemberKind::GetAccessorSig(AccessorSig {
                    name,
                    params,
                    return_type,
                })
            } else {
                TypeMemberKind::SetAccessorSig(AccessorSig {
                    name,
                    params,
                    return_type,
                })
            };
            return TypeMember {
                kind,
                span: self.span_from(start),
            };
        }

        // Property or method signature
        let name = if let Some(n) = self.parse_property_name() {
            n
        } else {
            self.error_code(1003, "Identifier expected.".into());
            PropName::Ident("<error>".into(), self.cur_span())
        };
        let optional = self.eat(TokenKind::Question).is_some();

        if self.at(TokenKind::OpenParen) || self.at(TokenKind::LessThan) {
            // Method signature
            let type_params = self.try_parse_type_params();
            let params = self.parse_param_list();
            self.check_signature_parameter_defaults(&params);
            let return_type = if self.eat(TokenKind::Colon).is_some() {
                Some(self.parse_return_type())
            } else {
                None
            };
            TypeMember {
                kind: TypeMemberKind::MethodSig(MethodSig {
                    name,
                    type_params,
                    params,
                    return_type,
                    optional,
                }),
                span: self.span_from(start),
            }
        } else {
            // Property signature
            let type_ann = if self.eat(TokenKind::Colon).is_some() {
                Some(self.parse_type())
            } else {
                None
            };
            if self.eat(TokenKind::Equals).is_some() {
                let initializer = self.parse_assignment_expr();
                let (code, message) = if in_interface {
                    (1246, "An interface property cannot have an initializer.")
                } else {
                    (1247, "A type literal property cannot have an initializer.")
                };
                self.error_at_span(code, message.to_string(), initializer.span);
            }
            TypeMember {
                kind: TypeMemberKind::PropertySig(PropertySig {
                    name,
                    type_ann,
                    optional,
                    readonly,
                }),
                span: self.span_from(start),
            }
        }
    }

    fn parse_index_signature_member(&mut self, start: u32, readonly: bool) -> TypeMember {
        let modifiers = if readonly { MOD_READONLY } else { MOD_NONE };
        let signature = self.parse_index_signature(start, modifiers);
        TypeMember {
            kind: TypeMemberKind::IndexSig(signature),
            span: self.span_from(start),
        }
    }

    // -----------------------------------------------------------------------
    // Type alias
    // -----------------------------------------------------------------------

    fn parse_type_alias_decl(&mut self, modifiers: ModifierFlags, start: u32) -> Stmt {
        self.bump(); // type
        let (name, name_span) = self.parse_identifier();
        if self.is_js_file {
            self.error_at_span(
                8008,
                "Type aliases can only be used in TypeScript files.".to_string(),
                name_span,
            );
        }
        let type_params = self.try_parse_type_params();
        // Recovery: if type params failed to parse (backtracked) but we're still
        // at `<`, consume the broken type parameter list so the rest of the type
        // alias (= Type;) is properly consumed and the whole declaration stays as
        // a single TypeAlias node.
        if type_params.is_none() && self.at(TokenKind::LessThan) {
            self.bump(); // <
            let mut depth = 1u32;
            while depth > 0 && !self.is_eof() {
                match self.cur() {
                    TokenKind::LessThan => {
                        depth += 1;
                        self.bump();
                    }
                    TokenKind::GreaterThan => {
                        depth -= 1;
                        self.bump();
                    }
                    _ => {
                        self.bump();
                    }
                }
            }
        }
        self.expect(TokenKind::Equals);
        let type_ann = self.parse_type();
        self.eat_semicolon();
        Stmt {
            kind: StmtKind::TypeAlias(Box::new(TypeAliasDecl {
                name: name.into(),
                name_span: Some(name_span),
                type_params,
                type_ann,
                modifiers,
                span: self.span_from(start),
            })),
            span: self.span_from(start),
        }
    }

    // -----------------------------------------------------------------------
    // Enum declaration
    // -----------------------------------------------------------------------

    fn parse_enum_decl(&mut self, modifiers: ModifierFlags, start: u32) -> Stmt {
        let is_const = modifiers & MOD_CONST != 0;
        self.expect(TokenKind::Enum);
        let (name, name_span) = self.parse_identifier();
        if self.is_js_file {
            self.error_at_span(
                8006,
                "'enum' declarations can only be used in TypeScript files.".to_string(),
                name_span,
            );
        }
        self.expect(TokenKind::OpenBrace);
        let mut members = Vec::new();
        while !self.at(TokenKind::CloseBrace) && !self.is_eof() {
            let mem_start = self.cur_span().start;
            let mem_name = if let Some(n) = self.parse_property_name() {
                n
            } else {
                self.error_code(1003, "Identifier expected.".into());
                PropName::Ident("<error>".into(), self.cur_span())
            };
            // Error recovery: `:` in enum members acts as a separator
            // (like `,`), NOT as an initializer. `a: 1` → member `a` (no init)
            // followed by member `1` (numeric name). TypeScript treats the value
            // after `:` as a separate member.
            let init = if self.at(TokenKind::Colon) {
                // `:` terminates this member; the value after `:` starts a new member.
                None
            } else if self.eat(TokenKind::Equals).is_some() {
                Some(Box::new(self.parse_assignment_expr()))
            } else {
                None
            };
            members.push(EnumMember {
                name: mem_name,
                initializer: init,
                span: self.span_from(mem_start),
            });
            if self.eat(TokenKind::Comma).is_none()
                && self.eat(TokenKind::Colon).is_none()
                && self.eat(TokenKind::Semicolon).is_none()
            {
                // Error recovery: compound assignment operators (+=, -=, etc.)
                // between enum members are treated as separators.
                if self.cur().is_assignment() && self.cur() != TokenKind::Equals {
                    self.bump(); // skip the compound assignment operator
                    continue;
                }
                // No comma/colon: continue if next token looks like another member
                // (error recovery for missing separators between enum members).
                let next_could_be_member = matches!(
                    self.cur(),
                    TokenKind::OpenBracket
                        | TokenKind::Identifier
                        | TokenKind::StringLiteral
                        | TokenKind::NumericLiteral
                ) && !self.at(TokenKind::CloseBrace);
                if !next_could_be_member {
                    break;
                }
            }
        }
        self.expect(TokenKind::CloseBrace);
        Stmt {
            kind: StmtKind::EnumDecl(Box::new(EnumDecl {
                name: name.into(),
                name_span: Some(name_span),
                members,
                modifiers,
                is_const,
                span: self.span_from(start),
            })),
            span: self.span_from(start),
        }
    }

    // -----------------------------------------------------------------------
    // Module/Namespace declaration
    // -----------------------------------------------------------------------

    fn parse_module_decl(&mut self, modifiers: ModifierFlags, start: u32) -> Stmt {
        let is_global = self.at(TokenKind::Global);
        let keyword = self.text(self.cur_span()).to_string();
        // skip namespace/module/global keyword
        self.bump();
        if is_global {
            // `declare global { ... }` — the name is "global" and the next
            // token is `{`. Don't try to parse another identifier.
            let body = if self.at(TokenKind::OpenBrace) {
                let body = if modifiers & MOD_DECLARE != 0 || self.ambient_depth > 0 {
                    self.parse_ambient_block_body()
                } else {
                    self.parse_block_body()
                };
                Some(ModuleBody::Block(body))
            } else {
                None
            };
            return Stmt {
                kind: StmtKind::ModuleDecl(Box::new(ModuleDecl {
                    name: ModuleName::Ident("global".to_string()),
                    name_span: None,
                    body,
                    modifiers,
                    span: self.span_from(start),
                })),
                span: self.span_from(start),
            };
        }
        let stmt = self.parse_module_decl_name_and_body(modifiers, start);
        if self.is_js_file && !is_global {
            if let StmtKind::ModuleDecl(module) = &stmt.kind {
                if let Some(name_span) = module.name_span {
                    self.error_at_span(
                        8006,
                        format!("'{keyword}' declarations can only be used in TypeScript files."),
                        name_span,
                    );
                }
            }
        }
        stmt
    }

    /// Parse the name and body of a module/namespace declaration.
    /// Separated from `parse_module_decl` so that recursive calls for dotted
    /// names (e.g. `Foo.Bar.Baz`) don't re-bump the keyword.
    fn parse_module_decl_name_and_body(&mut self, modifiers: ModifierFlags, start: u32) -> Stmt {
        let (name, name_span) = if self.at(TokenKind::OpenBrace) {
            (ModuleName::Ident("<error>".to_string()), None)
        } else if self.at(TokenKind::StringLiteral) {
            let span = self.bump_string_literal();
            let raw = self.text(span);
            let unquoted = &raw[1..raw.len() - 1];
            (ModuleName::String(unquoted.to_string()), Some(span))
        } else {
            let (n, s) = self.parse_identifier();
            (ModuleName::Ident(n.into()), Some(s))
        };
        let body = if self.at(TokenKind::OpenBrace) {
            let body = if modifiers & MOD_DECLARE != 0 || self.ambient_depth > 0 {
                self.parse_ambient_block_body()
            } else {
                self.parse_block_body()
            };
            Some(ModuleBody::Block(body))
        } else if self.eat(TokenKind::Dot).is_some() {
            let inner = self.parse_module_decl_name_and_body(modifiers, self.cur_span().start);
            if let StmtKind::ModuleDecl(md) = inner.kind {
                Some(ModuleBody::Module(md))
            } else {
                None
            }
        } else {
            self.eat_semicolon();
            None
        };
        Stmt {
            kind: StmtKind::ModuleDecl(Box::new(ModuleDecl {
                name,
                name_span,
                body,
                modifiers,
                span: self.span_from(start),
            })),
            span: self.span_from(start),
        }
    }

    // -----------------------------------------------------------------------
    // Import declaration
    // -----------------------------------------------------------------------

    fn parse_import_or_import_equals(&mut self, modifiers: ModifierFlags, start: u32) -> Stmt {
        self.bump(); // import
                     // Note: `import.meta` and `import(...)` are handled as expression
                     // statements by `parse_statement` (the Import arm has a guard that
                     // excludes `.` and `(` following `import`).

        // import type ...
        // `import type` is a type-only import when the token after `type` is:
        //   - an identifier (not `from`), e.g. `import type Foo from "m"`
        //   - `{`, e.g. `import type { A } from "m"`
        //   - `*`, e.g. `import type * as ns from "m"`
        // If the token after `type` is `from`, then `type` is used as the default import name.
        let type_only = if self.at(TokenKind::Type) {
            let next_kind = self.tokens.get(self.pos + 1).map(|t| t.kind);
            match next_kind {
                Some(TokenKind::OpenBrace) | Some(TokenKind::Asterisk) => {
                    self.bump(); // consume `type`
                    true
                }
                Some(k) if k == TokenKind::Identifier || k.is_contextual_keyword() => {
                    // Check if the next token is `from` — if so, it could be:
                    //   `import type from "m"` → `type` is the binding name (NOT type-only)
                    //   `import type from from "m"` → `type` is modifier, `from` is binding (type-only)
                    //   `import type from = require("m")` → `type` is modifier (type-only)
                    let is_from = self.tokens.get(self.pos + 1).is_some_and(|t| {
                        &self.source[t.span.start as usize..t.span.end as usize] == "from"
                    });
                    if !is_from {
                        self.bump(); // consume `type`
                        true
                    } else {
                        // Look further ahead: if the token after `from` is also `from`
                        // or `=`, then `type` is the modifier and `from` is a binding name
                        let after_from = self.tokens.get(self.pos + 2).map(|t| t.kind);
                        if matches!(after_from, Some(TokenKind::From) | Some(TokenKind::Equals)) {
                            self.bump(); // consume `type`
                            true
                        } else {
                            false
                        }
                    }
                }
                _ => false,
            }
        } else {
            false
        };

        // `import type defer * as ns from "m"` is an invalid ordering of
        // `type` and `defer`. TypeScript abandons the import after `defer` and
        // reparses the remaining `* as ns from "m"` tokens as statements.
        // Keep an empty type-only import node so this abandoned clause does
        // not make the output an external module.
        if type_only
            && self.is_identifier()
            && self.text(self.cur_span()) == "defer"
            && self.tokens.get(self.pos + 1).map(|token| token.kind) == Some(TokenKind::Asterisk)
        {
            self.bump();
            return Stmt {
                kind: StmtKind::Import(Box::new(ImportDecl {
                    specifiers: ImportClause::Named {
                        default: None,
                        named: Vec::new(),
                        namespace: None,
                    },
                    source: String::new(),
                    type_only: true,
                    defer: false,
                    is_side_effect: false,
                    span: self.span_from(start),
                    source_span: Span::new(0, 0),
                    has_resolution_mode: false,
                })),
                span: self.span_from(start),
            };
        }

        // `import defer ...` — parse the `defer` contextual keyword.
        // `defer` can appear in import declarations:
        //   `import defer * as ns from "m"`
        //   `import defer Foo from "m"`
        //   `import defer { A } from "m"`
        let is_defer = if type_only {
            // `import type defer ...` — with a type-only clause, `defer` can
            // only be a BINDING NAME (tsc); the keyword reading is invalid.
            false
        } else if self.is_identifier() {
            let cur_text = self.text(self.cur_span());
            if cur_text == "defer" {
                let next_kind = self.tokens.get(self.pos + 1).map(|t| t.kind);
                // `import defer from "m"` — `defer` is the default binding name,
                // NOT the `defer` keyword.  Disambiguate by checking whether the
                // token immediately after `defer` is `from` followed by a string
                // literal (module specifier).  In keyword position the `from` slot
                // is always occupied by a binding (`*`, `{`, or identifier) instead.
                let next_is_from_string = next_kind == Some(TokenKind::From)
                    && self.tokens.get(self.pos + 2).map(|t| t.kind)
                        == Some(TokenKind::StringLiteral);
                if !next_is_from_string
                    && (matches!(
                        next_kind,
                        Some(TokenKind::OpenBrace)
                            | Some(TokenKind::Asterisk)
                            | Some(TokenKind::Identifier)
                    ) || next_kind.is_some_and(|k| k.is_contextual_keyword()))
                {
                    self.bump(); // consume `defer`
                    true
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        };

        // `import defer type ...` — invalid combination: tsc consumes through
        // `* as` (or the brace) and abandons the clause; the remaining tokens
        // re-parse as statements and the empty import elides.
        if is_defer && self.at(TokenKind::Type) {
            self.error("'defer' imports cannot be type-only.".into());
            self.bump(); // `type`
            if self.at(TokenKind::Asterisk) {
                self.bump();
                let _ = self.eat(TokenKind::As);
            } else if self.at(TokenKind::OpenBrace) {
                self.bump();
            }
            return Stmt {
                kind: StmtKind::Import(Box::new(ImportDecl {
                    specifiers: ImportClause::Named {
                        default: None,
                        named: Vec::new(),
                        namespace: None,
                    },
                    source: String::new(),
                    // NOT type_only: the file must still count as a module
                    // (gold keeps the `export {}` marker without a prologue).
                    type_only: false,
                    defer: true,
                    is_side_effect: false,
                    span: self.span_from(start),
                    source_span: Span::new(0, 0),
                    has_resolution_mode: false,
                })),
                span: self.span_from(start),
            };
        }

        // import "source"
        if self.at(TokenKind::StringLiteral) {
            let (src, src_span) = self.parse_string_literal_with_span();
            let has_resolution_mode = self.skip_import_attributes();
            self.eat_semicolon();
            return Stmt {
                kind: StmtKind::Import(Box::new(ImportDecl {
                    specifiers: ImportClause::Named {
                        default: None,
                        named: Vec::new(),
                        namespace: None,
                    },
                    source: src,
                    type_only: false,
                    defer: false,
                    is_side_effect: true,
                    span: self.span_from(start),
                    source_span: src_span,
                    has_resolution_mode,
                })),
                span: self.span_from(start),
            };
        }

        let mut default_import = None;
        let mut named = Vec::new();
        let mut namespace = None;

        // import * as name
        if self.at(TokenKind::Asterisk) {
            self.bump();
            self.expect(TokenKind::As);
            let (n, _) = self.parse_identifier();
            namespace = Some(n.into());
        } else if self.at(TokenKind::OpenBrace) {
            let excessive_type_as_chain = self.tokens.get(self.pos + 1).map(|t| t.kind)
                == Some(TokenKind::Type)
                && (2..=5).all(|offset| {
                    self.tokens.get(self.pos + offset).map(|t| t.kind) == Some(TokenKind::As)
                })
                && self.tokens.get(self.pos + 6).map(|t| t.kind) == Some(TokenKind::CloseBrace)
                && self.tokens.get(self.pos + 7).map(|t| t.kind) == Some(TokenKind::From)
                && self.tokens.get(self.pos + 8).map(|t| t.kind) == Some(TokenKind::StringLiteral);
            if excessive_type_as_chain {
                for _ in 0..7 {
                    self.bump();
                }
                self.expect(TokenKind::From);
                let (source, source_span) = self.parse_string_literal_with_span();
                let has_resolution_mode = self.skip_import_attributes();
                self.eat_semicolon();
                return Stmt {
                    kind: StmtKind::Import(Box::new(ImportDecl {
                        specifiers: ImportClause::Named {
                            default: None,
                            named: Vec::new(),
                            namespace: None,
                        },
                        source,
                        type_only: true,
                        defer: false,
                        is_side_effect: false,
                        span: self.span_from(start),
                        source_span,
                        has_resolution_mode,
                    })),
                    span: self.span_from(start),
                };
            }
            named = self.parse_named_imports();
        } else if self.is_identifier() {
            let (n, _) = self.parse_identifier();

            // Check for `import x = require("...")` (TypeScript import-equals)
            if self.at(TokenKind::Equals) {
                self.bump(); // consume `=`
                let is_require = (self.at(TokenKind::Identifier) || self.at(TokenKind::Require))
                    && self.text(self.cur_span()) == "require";
                if is_require {
                    self.bump(); // consume `require`
                    self.expect(TokenKind::OpenParen);
                    let (src, src_span) = if self.at(TokenKind::StringLiteral) {
                        self.parse_string_literal_with_span()
                    } else {
                        // Non-string require argument (e.g. `require(x)`).
                        // Consume tokens until `)` so they don't leak as
                        // stray expression statements.
                        self.error_code(1141, "String literal expected.".into());
                        let fallback_span = self.cur_span();
                        while !self.at(TokenKind::CloseParen) && !self.at(TokenKind::EndOfFile) {
                            self.bump();
                        }
                        (String::new(), fallback_span)
                    };
                    self.expect(TokenKind::CloseParen);
                    self.eat_semicolon();
                    let statement_span = self.span_from(start);
                    if self.is_js_file {
                        self.error_at_span(
                            8002,
                            "'import ... =' can only be used in TypeScript files.".to_string(),
                            statement_span,
                        );
                    }
                    return Stmt {
                        kind: StmtKind::Import(Box::new(ImportDecl {
                            specifiers: ImportClause::Require(n.into()),
                            source: src,
                            type_only,
                            defer: false,
                            is_side_effect: false,
                            span: statement_span,
                            source_span: src_span,
                            has_resolution_mode: false,
                        })),
                        span: statement_span,
                    };
                }
                // `import x = M.N;` — import equals namespace reference
                // Parse the right-hand side as a dotted identifier chain
                // and emit as `var x = M.N;`
                let rhs_start = self.cur_span().start;
                let (first_id, _) = self.parse_identifier();
                let mut expr = Expr {
                    kind: ExprKind::Ident(first_id.into()),
                    span: self.span_from(rhs_start),
                };
                while self.eat(TokenKind::Dot).is_some() {
                    let prop_name = if let Some((name, _)) = self.parse_identifier_name() {
                        name
                    } else {
                        self.error_code(1003, "Identifier expected.".into());
                        "<error>".into()
                    };
                    expr = Expr {
                        kind: ExprKind::Member(Box::new(MemberExpr {
                            object: Box::new(expr),
                            property: prop_name.into(),
                            optional: false,
                        })),
                        span: self.span_from(rhs_start),
                    };
                }
                self.eat_semicolon();
                let statement_span = self.span_from(start);
                if self.is_js_file {
                    self.error_at_span(
                        8002,
                        "'import ... =' can only be used in TypeScript files.".to_string(),
                        statement_span,
                    );
                }
                return Stmt {
                    kind: StmtKind::ImportEquals(Box::new(ImportEqualsDecl {
                        name: n.into(),
                        module_ref: Box::new(expr),
                        modifiers,
                    })),
                    span: statement_span,
                };
            }

            default_import = Some(n.into());

            if self.eat(TokenKind::Comma).is_some() {
                if self.at(TokenKind::Asterisk) {
                    self.bump();
                    self.expect(TokenKind::As);
                    let (n, _) = self.parse_identifier();
                    namespace = Some(n.into());
                } else if self.at(TokenKind::OpenBrace) {
                    named = self.parse_named_imports();
                }
            }
        }

        // from "source"
        let (source, source_span) = if self.eat(TokenKind::From).is_some() {
            self.parse_string_literal_with_span()
        } else if (namespace.is_some() || !named.is_empty()) && self.is_identifier() {
            // `import * from Zero from "./0"` — tsc treats the stray
            // identifier as the (invalid) module specifier: consume it and
            // mark the source MISSING (CJS emits `require()`); the remaining
            // tokens re-parse as statements.
            self.error_code(1005, "'from' expected.".into());
            self.bump();
            (String::new(), Span::new(0, 0))
        } else {
            (String::new(), self.cur_span())
        };

        let has_resolution_mode = self.skip_import_attributes();
        self.eat_semicolon();
        let statement_span = self.span_from(start);
        if self.is_js_file && type_only {
            self.error_at_span(
                8006,
                "'import type' declarations can only be used in TypeScript files.".to_string(),
                statement_span,
            );
        }

        Stmt {
            kind: StmtKind::Import(Box::new(ImportDecl {
                specifiers: ImportClause::Named {
                    default: default_import,
                    named,
                    namespace,
                },
                source,
                type_only,
                defer: is_defer,
                is_side_effect: false,
                span: statement_span,
                source_span,
                has_resolution_mode,
            })),
            span: statement_span,
        }
    }

    fn parse_named_imports(&mut self) -> Vec<ImportSpecifier> {
        self.expect(TokenKind::OpenBrace);
        let mut specs = Vec::new();
        while !self.at(TokenKind::CloseBrace) && !self.is_eof() {
            let spec_start = self.cur_span().start;
            let is_type = self.at(TokenKind::Type)
                && (self.peek_is_ident_or_keyword() || self.peek_is(TokenKind::StringLiteral))
                && !self.peek_is_type_as_alias_start();
            if is_type {
                self.bump();
            }
            if let Some((first, first_is_string)) = self.parse_module_export_name() {
                let (local, imported, imported_is_string) = if self.eat(TokenKind::As).is_some() {
                    let (local, _) = self.parse_identifier();
                    if local == "<error>" {
                        self.recover_named_import_local_error_tail();
                    }
                    (local.into(), Some(first), first_is_string)
                } else if first_is_string {
                    self.error_code(1005, "'as' expected.".into());
                    ("<error>".to_string(), Some(first), first_is_string)
                } else {
                    (first, None, false)
                };
                let spec_span = self.span_from(spec_start);
                if self.is_js_file && is_type {
                    self.error_at_span(
                        8006,
                        "'import...type' declarations can only be used in TypeScript files."
                            .to_string(),
                        spec_span,
                    );
                }
                specs.push(ImportSpecifier {
                    local,
                    imported,
                    is_type,
                    imported_is_string,
                    span: spec_span,
                });
            } else {
                self.error_code(1003, "Identifier expected.".into());
                // Recovery: consume malformed specifier tail (`<lit> as name`)
                // so it doesn't leak as standalone statements.
                if !self.is_eof() {
                    self.bump();
                }
                if self.eat(TokenKind::As).is_some() {
                    if self.at(TokenKind::StringLiteral) {
                        self.parse_string_literal();
                    } else if self.parse_identifier_name().is_none()
                        && !self.at(TokenKind::Comma)
                        && !self.at(TokenKind::CloseBrace)
                        && !self.is_eof()
                    {
                        self.bump();
                    }
                }
                while !self.at(TokenKind::Comma)
                    && !self.at(TokenKind::CloseBrace)
                    && !self.is_eof()
                {
                    self.bump();
                }
                specs.push(ImportSpecifier {
                    local: "<error>".to_string(),
                    imported: None,
                    is_type,
                    imported_is_string: false,
                    span: self.span_from(spec_start),
                });
            }

            if self.eat(TokenKind::Comma).is_none() {
                break;
            }
        }
        self.expect(TokenKind::CloseBrace);
        specs
    }

    // -----------------------------------------------------------------------
    // Export declaration
    // -----------------------------------------------------------------------

    fn parse_export_decl(
        &mut self,
        modifiers: ModifierFlags,
        mut decorators: Vec<Expr>,
        start: u32,
    ) -> Stmt {
        self.bump(); // export

        // export default ...
        if self.eat(TokenKind::Default).is_some() {
            return self.parse_export_default(modifiers, decorators, start);
        }

        // Handle decorators between `export` and `default`/`class`/etc.
        // e.g., `export @dec default class C3 {}` or `export @dec class C4 {}`
        if self.at(TokenKind::At) {
            let inner_decs = self.parse_decorators();
            if self.at(TokenKind::Default) {
                let span = self.decorator_diagnostic_span(&inner_decs[0], true);
                self.error_at_span(1206, "Decorators are not valid here.".to_string(), span);
            }
            decorators.extend(inner_decs);

            // export @dec default ...
            if self.eat(TokenKind::Default).is_some() {
                return self.parse_export_default(modifiers, decorators, start);
            }
        }

        // export = expr;
        if self.eat(TokenKind::Equals).is_some() {
            let expr = Box::new(self.parse_expression());
            self.eat_semicolon();
            let statement_span = self.span_from(start);
            if self.is_js_file {
                self.error_at_span(
                    8003,
                    "'export =' can only be used in TypeScript files.".to_string(),
                    statement_span,
                );
            }
            return Stmt {
                kind: StmtKind::ExportAssign(expr),
                span: statement_span,
            };
        }

        // export as namespace X;  (UMD namespace export — TypeScript-only, erased)
        if self.at(TokenKind::As) && self.peek_is(TokenKind::Namespace) {
            self.bump(); // as
            self.bump(); // namespace
            let name = if self.is_identifier() {
                let (n, _) = self.parse_identifier();
                String::from(n)
            } else {
                "<error>".to_string()
            };
            self.eat_semicolon();
            return Stmt {
                kind: StmtKind::ModuleDecl(Box::new(ModuleDecl {
                    name: ModuleName::Ident(name),
                    name_span: None,
                    body: None,
                    modifiers: MOD_DECLARE,
                    span: self.span_from(start),
                })),
                span: self.span_from(start),
            };
        }

        let type_only = self.at(TokenKind::Type)
            && (self.peek_is(TokenKind::OpenBrace) || self.peek_is(TokenKind::Asterisk));
        if type_only {
            self.bump();
        }

        // export * from "source"
        if self.at(TokenKind::Asterisk) {
            self.bump();
            let (alias, alias_is_string) = if self.eat(TokenKind::As).is_some() {
                if let Some((n, is_string)) = self.parse_module_export_name() {
                    (Some(n), is_string)
                } else {
                    self.error_code(1003, "Identifier expected.".into());
                    (Some("<error>".to_string()), false)
                }
            } else {
                (None, false)
            };
            self.expect(TokenKind::From);
            let source = if self.at(TokenKind::StringLiteral) {
                self.parse_string_literal()
            } else if self.is_identifier() {
                // Error recovery: `export * from Identifier` (e.g. in a
                // namespace). Consume the identifier so it doesn't become a
                // stray expression statement.
                let (name, _) = self.parse_identifier();
                String::from(name)
            } else {
                self.parse_string_literal()
            };
            let _ = self.skip_import_attributes();
            self.eat_semicolon();
            let statement_span = self.span_from(start);
            if self.is_js_file && type_only {
                self.error_at_span(
                    8006,
                    "'export type' declarations can only be used in TypeScript files.".to_string(),
                    statement_span,
                );
            }
            return Stmt {
                kind: StmtKind::Export(Box::new(ExportDecl {
                    kind: ExportDeclKind::All {
                        source,
                        alias,
                        alias_is_string,
                        type_only,
                    },
                    span: statement_span,
                })),
                span: statement_span,
            };
        }

        // export { ... } [from "source"]
        if self.at(TokenKind::OpenBrace) {
            let specifiers = self.parse_named_exports();
            let source = if self.eat(TokenKind::From).is_some() {
                Some(self.parse_string_literal())
            } else {
                None
            };
            let _ = self.skip_import_attributes();
            self.eat_semicolon();
            let statement_span = self.span_from(start);
            if self.is_js_file && type_only {
                self.error_at_span(
                    8006,
                    "'export type' declarations can only be used in TypeScript files.".to_string(),
                    statement_span,
                );
            }
            return Stmt {
                kind: StmtKind::Export(Box::new(ExportDecl {
                    kind: ExportDeclKind::Named {
                        specifiers,
                        source,
                        type_only,
                    },
                    span: statement_span,
                })),
                span: statement_span,
            };
        }

        // export var/let/const/function/class/interface/type/enum/namespace/...
        // For class and function, pass through any decorators from `@dec export class ...`
        let decl = if !decorators.is_empty()
            && matches!(
                self.cur(),
                TokenKind::Class
                    | TokenKind::Abstract
                    | TokenKind::Declare
                    | TokenKind::Function
                    | TokenKind::Async
            ) {
            let decl_start = self.cur_span().start;
            let inner_modifiers = self.parse_modifiers();
            match self.cur() {
                TokenKind::Class => self.parse_class_decl(inner_modifiers, decorators, decl_start),
                TokenKind::Function => {
                    self.parse_function_decl(inner_modifiers, decorators, decl_start)
                }
                _ => self.parse_statement(true),
            }
        } else {
            self.parse_statement(true)
        };
        Stmt {
            kind: StmtKind::Export(Box::new(ExportDecl {
                kind: ExportDeclKind::Decl(Box::new(decl)),
                span: self.span_from(start),
            })),
            span: self.span_from(start),
        }
    }

    fn parse_export_default(
        &mut self,
        modifiers: ModifierFlags,
        mut decorators: Vec<Expr>,
        start: u32,
    ) -> Stmt {
        // Handle decorators between `default` and `class`/`abstract`, but only
        // when we already have decorators from before `export` (e.g.,
        // `@dec export default @dec class C7 {}`). Without this guard,
        // `export default @dec class {}` changes from Default(expr) to
        // DefaultDecl which breaks standard decorator IIFE wrappers.
        if !decorators.is_empty() && self.at(TokenKind::At) {
            let inner_decs = self.parse_decorators();
            decorators.extend(inner_decs);
        }
        match self.cur() {
            TokenKind::Function => {
                let decl = self.parse_function_decl(
                    modifiers | MOD_EXPORT | MOD_DEFAULT,
                    decorators,
                    start,
                );
                Stmt {
                    kind: StmtKind::Export(Box::new(ExportDecl {
                        kind: ExportDeclKind::DefaultDecl(Box::new(decl)),
                        span: self.span_from(start),
                    })),
                    span: self.span_from(start),
                }
            }
            TokenKind::Async if self.peek_is(TokenKind::Function) => {
                let decl = self.parse_function_decl(
                    modifiers | MOD_EXPORT | MOD_DEFAULT | MOD_ASYNC,
                    decorators,
                    start,
                );
                Stmt {
                    kind: StmtKind::Export(Box::new(ExportDecl {
                        kind: ExportDeclKind::DefaultDecl(Box::new(decl)),
                        span: self.span_from(start),
                    })),
                    span: self.span_from(start),
                }
            }
            TokenKind::Class => {
                let decl =
                    self.parse_class_decl(modifiers | MOD_EXPORT | MOD_DEFAULT, decorators, start);
                Stmt {
                    kind: StmtKind::Export(Box::new(ExportDecl {
                        kind: ExportDeclKind::DefaultDecl(Box::new(decl)),
                        span: self.span_from(start),
                    })),
                    span: self.span_from(start),
                }
            }
            TokenKind::Abstract if self.peek_is(TokenKind::Class) => {
                let decl =
                    self.parse_class_decl(modifiers | MOD_EXPORT | MOD_DEFAULT, decorators, start);
                Stmt {
                    kind: StmtKind::Export(Box::new(ExportDecl {
                        kind: ExportDeclKind::DefaultDecl(Box::new(decl)),
                        span: self.span_from(start),
                    })),
                    span: self.span_from(start),
                }
            }
            TokenKind::Interface if self.is_interface_declaration() => {
                let decl = self.parse_interface_decl(modifiers | MOD_EXPORT | MOD_DEFAULT, start);
                Stmt {
                    kind: StmtKind::Export(Box::new(ExportDecl {
                        kind: ExportDeclKind::DefaultDecl(Box::new(decl)),
                        span: self.span_from(start),
                    })),
                    span: self.span_from(start),
                }
            }
            _ => {
                let expr = Box::new(self.parse_assignment_expr());
                self.eat_semicolon();
                Stmt {
                    kind: StmtKind::Export(Box::new(ExportDecl {
                        kind: ExportDeclKind::Default(expr),
                        span: self.span_from(start),
                    })),
                    span: self.span_from(start),
                }
            }
        }
    }

    fn parse_named_exports(&mut self) -> Vec<ExportSpecifier> {
        self.expect(TokenKind::OpenBrace);
        let mut specs = Vec::new();
        while !self.at(TokenKind::CloseBrace) && !self.is_eof() {
            // If `from` is followed by a string literal, treat it as the module
            // source clause rather than a specifier name.  This handles unclosed
            // export clauses like `export { x, from "./t1"`.
            if self.at(TokenKind::From) && self.peek_is(TokenKind::StringLiteral) {
                break;
            }
            let spec_start = self.cur_span().start;
            let is_type = self.at(TokenKind::Type)
                && (self.peek_is_ident_or_keyword() || self.peek_is(TokenKind::StringLiteral))
                && !self.peek_is_type_as_alias_start();
            if is_type {
                self.bump();
            }
            if let Some((first, first_is_string)) = self.parse_module_export_name() {
                let (local, exported, local_is_string, exported_is_string) =
                    if self.eat(TokenKind::As).is_some() {
                        if let Some((exp, exp_is_string)) = self.parse_module_export_name() {
                            (first, Some(exp), first_is_string, exp_is_string)
                        } else {
                            self.error_code(1003, "Identifier expected.".into());
                            if !self.at(TokenKind::Comma)
                                && !self.at(TokenKind::CloseBrace)
                                && !self.is_eof()
                            {
                                self.bump();
                            }
                            // Preserve `as` for emission recovery: `export { foo as  };`
                            (first, Some(String::new()), first_is_string, false)
                        }
                    } else {
                        (first, None, first_is_string, false)
                    };
                let spec_span = self.span_from(spec_start);
                if self.is_js_file && is_type {
                    self.error_at_span(
                        8006,
                        "'export...type' declarations can only be used in TypeScript files."
                            .to_string(),
                        spec_span,
                    );
                }
                specs.push(ExportSpecifier {
                    local,
                    exported,
                    is_type,
                    local_is_string,
                    exported_is_string,
                    span: spec_span,
                });
            } else {
                self.error_code(1003, "Identifier expected.".into());
                // Recovery: consume malformed specifier tail (`<lit> as name`)
                // so it doesn't leak as standalone statements.
                if !self.is_eof() {
                    self.bump();
                }
                if self.eat(TokenKind::As).is_some() {
                    if self.at(TokenKind::StringLiteral) {
                        self.parse_string_literal();
                    } else if self.parse_identifier_name().is_none()
                        && !self.at(TokenKind::Comma)
                        && !self.at(TokenKind::CloseBrace)
                        && !self.is_eof()
                    {
                        self.bump();
                    }
                }
                while !self.at(TokenKind::Comma)
                    && !self.at(TokenKind::CloseBrace)
                    && !self.is_eof()
                {
                    self.bump();
                }
            }

            if self.eat(TokenKind::Comma).is_none() {
                break;
            }
        }
        self.expect(TokenKind::CloseBrace);
        specs
    }

    // -----------------------------------------------------------------------
    // Parameters
    // -----------------------------------------------------------------------

    fn parse_param_list(&mut self) -> Vec<Param> {
        self.expect(TokenKind::OpenParen);
        let mark = self.scratch_params.len();
        let mut last_pos = usize::MAX;
        let mut recovering_type_predicate_as_params = false;
        while !self.at(TokenKind::CloseParen) && !self.is_eof() {
            let before_pos = self.pos;
            let param = self.parse_param();
            self.scratch_params.push(param);
            if self.eat(TokenKind::Comma).is_none() {
                // A type predicate is not permitted in a parameter type. TSC
                // recovers `function f(a: b is A) {}` as parameters
                // `(a, is, A)` so the function body remains attached.
                if (self.at(TokenKind::Is)
                    || recovering_type_predicate_as_params && self.is_identifier())
                    && self.pos != before_pos
                {
                    recovering_type_predicate_as_params = true;
                    continue;
                }
                // Error recovery: `@` starts a new parameter decorator even without comma.
                // `constructor(public @dec p)` → two params: `public` and `@dec p`.
                if self.at(TokenKind::At) {
                    continue;
                }
                // Missing-comma recovery (tsc-style), never during speculation
                // (consuming junk would make a doomed arrow-params attempt
                // look successful). Only after a REST param — tsc recovers
                // `(...public rest)` / `(...foo[]: any)` but deliberately
                // mangles junk after normal params (`(x!: T)`).
                if self.speculation_depth == 0
                    && !self.at(TokenKind::CloseParen)
                    && self.scratch_params[mark..]
                        .last()
                        .is_some_and(|prm| prm.dotdotdot)
                {
                    // Narrow missing-comma recovery: continue the list only
                    // for an identifier straight after a rest param
                    // (`(...public rest: string[])` -> `...public`, `rest`,
                    // matching tsc's emit) or a `[` binding-pattern start
                    // (`(...foo[]: any)`). Broader continuation swallows
                    // function bodies in `function boo { ... }` recovery.
                    if (self.is_identifier() || self.at(TokenKind::OpenBracket))
                        && self.pos != before_pos
                        && self.pos != last_pos
                    {
                        last_pos = self.pos;
                        continue;
                    }
                    // Anything else: LOOK AHEAD for a `,` or `)` at depth 0
                    // before any `{`/`;`/EOF, and only then commit to skipping
                    // the junk (`(...foo[]: any)` still closes cleanly). If no
                    // clean close is reachable, leave the cursor for the
                    // fn-body recovery paths (e.g. `function boo{ ... }`).
                    if !self.at(TokenKind::OpenBrace) {
                        let mut look = self.pos;
                        let mut depth = 0i32;
                        let target: Option<usize> = loop {
                            let Some(tok) = self.tokens.get(look) else {
                                break None;
                            };
                            match tok.kind {
                                TokenKind::CloseParen | TokenKind::Comma if depth == 0 => {
                                    break Some(look);
                                }
                                TokenKind::OpenBrace | TokenKind::Semicolon if depth == 0 => {
                                    break None;
                                }
                                TokenKind::OpenParen | TokenKind::OpenBracket => depth += 1,
                                TokenKind::CloseParen | TokenKind::CloseBracket => {
                                    depth -= 1;
                                    if depth < 0 {
                                        break None;
                                    }
                                }
                                _ => {}
                            }
                            look += 1;
                        };
                        if let Some(idx) = target {
                            self.pos = idx;
                            if self.eat(TokenKind::Comma).is_some() {
                                continue;
                            }
                        }
                    }
                }
                break;
            }
        }
        self.expect(TokenKind::CloseParen);
        let params = take_exact(&mut self.scratch_params, mark);
        self.check_parameter_grammar(&params);
        params
    }

    fn parse_param(&mut self) -> Param {
        let start = self.cur_span().start;
        let mut decorators = if self.at(TokenKind::At) {
            self.parse_decorators()
        } else {
            Vec::new()
        };
        let mut recovered_name_from_decorator = None;
        if let Some(last) = decorators.last_mut() {
            if matches!(
                self.cur(),
                TokenKind::CloseParen
                    | TokenKind::Comma
                    | TokenKind::Question
                    | TokenKind::Colon
                    | TokenKind::Equals
            ) {
                if let Some((decorator_expr, recovered_name)) =
                    Self::split_decorator_consumed_parameter_name(last)
                {
                    *last = decorator_expr;
                    recovered_name_from_decorator = Some(recovered_name);
                }
            }
            *last = Self::normalize_invalid_await_decorator(last.clone());
        }
        // Only parse modifiers if the keyword is followed by a name-like token,
        // not by `)`, `,`, `?`, `:`, or `=` which indicate the keyword IS the parameter name.
        let modifier_pos = self.pos;
        let modifiers = if self.is_modifier_keyword() && self.peek_is_param_name_follower() {
            MOD_NONE
        } else {
            self.parse_member_modifiers()
        };
        if self.is_js_file {
            let modifier_spans: Vec<Span> = self.tokens[modifier_pos..self.pos]
                .iter()
                .map(|token| token.span)
                .collect();
            for span in modifier_spans {
                self.error_at_span(
                    8012,
                    "Parameter modifiers can only be used in TypeScript files.".to_string(),
                    span,
                );
            }
        }
        let dotdotdot = self.eat(TokenKind::DotDotDot).is_some();
        // Error recovery: parameter starts with `:` (no binding name) followed by
        // an identifier, e.g. `function a(\n    : T) { }`. TypeScript treats the
        // identifier after the colon as the parameter name. Skip the leading
        // colon so parse_binding_pattern picks up the identifier.
        if self.at(TokenKind::Colon) && self.peek_is_ident_or_keyword() {
            self.bump(); // consume stray `:`
        }
        let name = recovered_name_from_decorator.unwrap_or_else(|| self.parse_binding_pattern());
        if matches!(&name.kind, PatKind::Ident(name) if name == "this") {
            if let Some(decorator) = decorators.first() {
                let span = self.decorator_diagnostic_span(decorator, true);
                self.error_at_span(
                    1433,
                    "Neither decorators nor modifiers may be applied to 'this' parameters."
                        .to_string(),
                    span,
                );
            }
        }
        let optional_token = self.eat(TokenKind::Question);
        let optional = optional_token.is_some();
        if self.is_js_file {
            if let Some(span) = optional_token {
                self.error_at_span(
                    8009,
                    "The '?' modifier can only be used in TypeScript files.".to_string(),
                    span,
                );
            }
        }
        let type_ann = if self.eat(TokenKind::Colon).is_some() {
            Some(self.parse_js_checked_type_annotation())
        } else {
            None
        };
        let initializer = if self.eat(TokenKind::Equals).is_some() {
            Some(Box::new(self.parse_assignment_expr()))
        } else {
            None
        };
        Param {
            name,
            type_ann,
            initializer,
            dotdotdot,
            optional,
            modifiers,
            decorators,
            span: self.span_from(start),
        }
    }

    // -----------------------------------------------------------------------
    // Expressions
    // -----------------------------------------------------------------------

    fn parse_expression(&mut self) -> Expr {
        let expr = self.parse_assignment_expr();
        if self.at(TokenKind::Comma) {
            let start = expr.span.start;
            let mut exprs = vec![Box::new(expr)];
            while self.eat(TokenKind::Comma).is_some() {
                exprs.push(Box::new(self.parse_assignment_expr()));
            }
            return Expr {
                kind: ExprKind::Comma(exprs),
                span: self.span_from(start),
            };
        }
        expr
    }

    /// Like `parse_expression` but tolerates a trailing comma — used inside
    /// parenthesized expressions where `(X, )` is a recoverable malformed
    /// comma operator with a missing right-hand operand.
    fn parse_expression_with_trailing_comma(&mut self) -> Expr {
        let expr = self.parse_assignment_expr();
        if self.at(TokenKind::Comma) {
            let start = expr.span.start;
            let mut exprs = vec![Box::new(expr)];
            while self.eat(TokenKind::Comma).is_some() {
                if self.at(TokenKind::CloseParen) {
                    let pos = self.cur_span().start;
                    exprs.push(Box::new(Expr {
                        kind: ExprKind::Omitted,
                        span: Span::new(pos, pos),
                    }));
                    break;
                }
                exprs.push(Box::new(self.parse_assignment_expr()));
            }
            if exprs.len() == 1 {
                return *exprs.pop().unwrap();
            }
            return Expr {
                kind: ExprKind::Comma(exprs),
                span: self.span_from(start),
            };
        }
        expr
    }

    fn parse_assignment_expr(&mut self) -> Expr {
        // Check for arrow function
        if let Some(arrow) = self.try_parse_arrow_function() {
            return arrow;
        }

        let start = self.cur_span().start;

        // yield expression
        if self.at(TokenKind::Yield) {
            return self.parse_yield_expr(start);
        }

        let expr = self.parse_conditional_expr();

        // Assignment — skip if LHS is an update expression (++x, x++) or
        // await expression since these are not valid assignment targets.
        // `await x = y` should be `await x;` then `y;`, not `await (x = y)`.
        if self.cur().is_assignment()
            && !matches!(expr.kind, ExprKind::Update(_))
            && !matches!(expr.kind, ExprKind::Await(_))
        {
            let op = self.parse_assign_op();
            self.bump();
            let right = Box::new(self.parse_assignment_expr());
            return Expr {
                kind: ExprKind::Assign(AssignExpr {
                    left: Box::new(expr),
                    op,
                    right,
                }),
                span: self.span_from(start),
            };
        }

        expr
    }
}

impl<'a> Parser<'a> {
    fn parse_yield_expr(&mut self, start: u32) -> Expr {
        self.bump(); // yield
        let delegate = self.eat(TokenKind::Asterisk).is_some();
        let argument = if !self.at(TokenKind::Semicolon)
            && !self.at(TokenKind::CloseParen)
            && !self.at(TokenKind::CloseBracket)
            && !self.at(TokenKind::CloseBrace)
            && !self.at(TokenKind::Comma)
            && !self.at(TokenKind::Colon)
            && !self.is_eof()
            && !self.is_on_new_line()
        {
            Some(Box::new(self.parse_assignment_expr()))
        } else {
            None
        };
        Expr {
            kind: ExprKind::Yield(delegate, argument),
            span: self.span_from(start),
        }
    }

    fn parse_conditional_expr(&mut self) -> Expr {
        let start = self.cur_span().start;
        let expr = self.parse_binary_expr(0);
        if self.eat(TokenKind::Question).is_some() {
            if self.at(TokenKind::CloseBrace) {
                self.error_code(1109, "Expression expected.".into());
                let missing = Expr {
                    kind: ExprKind::Omitted,
                    span: Span::new(self.cur_span().start, self.cur_span().start),
                };
                return Expr {
                    kind: ExprKind::Cond(CondExpr {
                        test: Box::new(expr),
                        consequent: Box::new(missing.clone()),
                        alternate: Box::new(missing),
                    }),
                    span: self.span_from(start),
                };
            }
            let saved_ternary = self.in_ternary_consequent;
            self.in_ternary_consequent = true;
            let consequent = Box::new(self.parse_assignment_expr());
            self.in_ternary_consequent = saved_ternary;
            self.expect(TokenKind::Colon);
            let alternate = Box::new(self.parse_assignment_expr());
            return Expr {
                kind: ExprKind::Cond(CondExpr {
                    test: Box::new(expr),
                    consequent,
                    alternate,
                }),
                span: self.span_from(start),
            };
        }
        expr
    }

    fn parse_binary_expr(&mut self, min_prec: u8) -> Expr {
        let start = self.cur_span().start;
        let mut left = self.parse_unary_expr();

        loop {
            // Adjacent JSX elements are not valid operands of `<`, because the
            // second `<` starts another JSX element. TypeScript recovers the
            // missing separator as a comma expression.
            let left_ends_in_jsx = self.at(TokenKind::LessThan)
                && match &left.kind {
                    ExprKind::JsxElement(_)
                    | ExprKind::JsxFragment(_)
                    | ExprKind::JsxSelfClosing(_) => true,
                    ExprKind::Comma(exprs) => exprs.last().is_some_and(|expr| {
                        matches!(
                            expr.kind,
                            ExprKind::JsxElement(_)
                                | ExprKind::JsxFragment(_)
                                | ExprKind::JsxSelfClosing(_)
                        )
                    }),
                    _ => false,
                };
            if self.at(TokenKind::LessThan) && self.is_jsx_start() && left_ends_in_jsx {
                let right = Box::new(self.parse_unary_expr());
                let span = self.span_from(start);
                left = match left.kind {
                    ExprKind::Comma(mut exprs) => {
                        exprs.push(right);
                        Expr {
                            kind: ExprKind::Comma(exprs),
                            span,
                        }
                    }
                    _ => Expr {
                        kind: ExprKind::Comma(vec![Box::new(left), right]),
                        span,
                    },
                };
                continue;
            }

            let (op, prec) = match self.cur() {
                TokenKind::AsteriskAsterisk => (BinaryOp::Exp, 14),
                TokenKind::Asterisk => (BinaryOp::Mul, 13),
                TokenKind::Slash => (BinaryOp::Div, 13),
                TokenKind::Percent => (BinaryOp::Mod, 13),
                TokenKind::Plus => (BinaryOp::Add, 12),
                TokenKind::Minus => (BinaryOp::Sub, 12),
                TokenKind::LessLess => (BinaryOp::Shl, 11),
                TokenKind::GreaterGreater => (BinaryOp::Shr, 11),
                TokenKind::GreaterGreaterGreater => (BinaryOp::UShr, 11),
                TokenKind::LessThan => (BinaryOp::Lt, 10),
                TokenKind::GreaterThan => (BinaryOp::Gt, 10),
                TokenKind::LessEqual => (BinaryOp::Le, 10),
                TokenKind::GreaterEqual => (BinaryOp::Ge, 10),
                TokenKind::InstanceOf => (BinaryOp::InstanceOf, 10),
                TokenKind::In if !self.disallow_in => (BinaryOp::In, 10),
                TokenKind::EqualsEquals => (BinaryOp::Eq, 9),
                TokenKind::ExclEquals => (BinaryOp::Ne, 9),
                TokenKind::EqualsEqualsEquals => (BinaryOp::StrictEq, 9),
                TokenKind::ExclEqualsEquals => (BinaryOp::StrictNe, 9),
                TokenKind::Ampersand => (BinaryOp::BitAnd, 8),
                TokenKind::Caret => (BinaryOp::BitXor, 7),
                TokenKind::Bar => (BinaryOp::BitOr, 6),
                TokenKind::AmpersandAmpersand => (BinaryOp::LogAnd, 5),
                TokenKind::BarBar => (BinaryOp::LogOr, 4),
                TokenKind::QuestionQuestion => (BinaryOp::NullCoal, 4),
                // Handle `as` and `satisfies` as type assertion operators.
                // ASI: `as`/`satisfies` do not bind across a newline.
                TokenKind::As if !self.is_on_new_line() => {
                    self.bump();
                    if self.eat(TokenKind::Const).is_some() {
                        left = Expr {
                            kind: ExprKind::As(Box::new(AsExpr {
                                expr: Box::new(left),
                                type_node: TypeNode {
                                    kind: TypeNodeKind::Reference(Box::new(TypeRef {
                                        name: Box::new(Expr {
                                            kind: ExprKind::Ident("const".into()),
                                            span: self.span_from(start),
                                        }),
                                        type_args: None,
                                    })),
                                    span: self.span_from(start),
                                },
                            })),
                            span: self.span_from(start),
                        };
                    } else {
                        let ty = self.parse_type();
                        left = Expr {
                            kind: ExprKind::As(Box::new(AsExpr {
                                expr: Box::new(left),
                                type_node: ty,
                            })),
                            span: self.span_from(start),
                        };
                    }
                    continue;
                }
                TokenKind::Satisfies if !self.is_on_new_line() => {
                    self.bump();
                    let ty = self.parse_type();
                    left = Expr {
                        kind: ExprKind::Satisfies(Box::new(SatisfiesExpr {
                            expr: Box::new(left),
                            type_node: ty,
                        })),
                        span: self.span_from(start),
                    };
                    continue;
                }
                _ => break,
            };

            if prec <= min_prec {
                break;
            }

            if op == BinaryOp::Exp {
                let unary_operator = match &left.kind {
                    ExprKind::Unary(unary) => Some(match unary.op {
                        UnaryOp::Pos => "+",
                        UnaryOp::Neg => "-",
                        UnaryOp::BitNot => "~",
                        UnaryOp::LogNot => "!",
                        UnaryOp::Typeof => "typeof",
                        UnaryOp::Void => "void",
                        UnaryOp::Delete => "delete",
                    }),
                    ExprKind::Typeof(_) => Some("typeof"),
                    ExprKind::Void(_) => Some("void"),
                    ExprKind::Delete(_) => Some("delete"),
                    ExprKind::Await(_) => Some("await"),
                    _ => None,
                };
                if let Some(operator) = unary_operator {
                    self.error_at_span(
                        17006,
                        format!(
                            "An unary expression with the '{operator}' operator is not allowed in the left-hand side of an exponentiation expression. Consider enclosing the expression in parentheses."
                        ),
                        left.span,
                    );
                }
            }

            self.bump();
            // Exponentiation is right-associative
            let next_prec = if prec == 14 { prec - 1 } else { prec };
            // Error recovery: if the next token is a statement keyword (return,
            // break, continue, etc.), produce an empty right operand and let the
            // keyword start a new statement. E.g. `1 + \n return;` → `1 + ; return;`.
            let right = if matches!(
                self.cur(),
                TokenKind::Return
                    | TokenKind::Break
                    | TokenKind::Continue
                    | TokenKind::Throw
                    | TokenKind::Case
                    | TokenKind::Default
            ) {
                Box::new(Expr {
                    kind: ExprKind::Ident("".into()),
                    span: self.cur_span(),
                })
            } else {
                Box::new(self.parse_binary_expr(next_prec))
            };
            left = Expr {
                kind: ExprKind::Binary(BinaryExpr {
                    left: Box::new(left),
                    op,
                    right,
                }),
                span: self.span_from(start),
            };
        }

        left
    }

    fn parse_unary_expr(&mut self) -> Expr {
        let start = self.cur_span().start;
        match self.cur() {
            TokenKind::Plus => {
                self.bump();
                let arg = Box::new(self.parse_unary_expr());
                Expr {
                    kind: ExprKind::Unary(UnaryExpr {
                        op: UnaryOp::Pos,
                        argument: arg,
                    }),
                    span: self.span_from(start),
                }
            }
            TokenKind::Minus => {
                self.bump();
                let arg = Box::new(self.parse_unary_expr());
                Expr {
                    kind: ExprKind::Unary(UnaryExpr {
                        op: UnaryOp::Neg,
                        argument: arg,
                    }),
                    span: self.span_from(start),
                }
            }
            TokenKind::Tilde => {
                self.bump();
                let arg = Box::new(self.parse_unary_expr());
                Expr {
                    kind: ExprKind::Unary(UnaryExpr {
                        op: UnaryOp::BitNot,
                        argument: arg,
                    }),
                    span: self.span_from(start),
                }
            }
            TokenKind::Excl => {
                self.bump();
                let arg = Box::new(self.parse_unary_expr());
                Expr {
                    kind: ExprKind::Unary(UnaryExpr {
                        op: UnaryOp::LogNot,
                        argument: arg,
                    }),
                    span: self.span_from(start),
                }
            }
            TokenKind::TypeOf => {
                self.bump();
                let arg = Box::new(self.parse_unary_expr());
                Expr {
                    kind: ExprKind::Typeof(arg),
                    span: self.span_from(start),
                }
            }
            TokenKind::Void => {
                self.bump();
                let arg = Box::new(self.parse_unary_expr());
                Expr {
                    kind: ExprKind::Void(arg),
                    span: self.span_from(start),
                }
            }
            TokenKind::Delete => {
                self.bump();
                let arg = Box::new(self.parse_unary_expr());
                Expr {
                    kind: ExprKind::Delete(arg),
                    span: self.span_from(start),
                }
            }
            TokenKind::Await => {
                self.bump();
                // In an await context, a malformed generic-looking operand such as
                // `await <T, U>(x)` is reparsed by TypeScript as a missing await
                // operand followed by the comma expression `, U > (x)`.  Consume
                // the rejected `<T` prefix, but leave the comma for the ordinary
                // expression parser.  This also covers the tagged-template form.
                if self.at(TokenKind::LessThan) && self.await_type_prefix_has_comma() {
                    self.bump(); // <
                    while !self.at(TokenKind::Comma) && !self.is_eof() {
                        self.bump();
                    }
                    let arg = Box::new(Expr {
                        kind: ExprKind::Omitted,
                        span: self.span_from(start),
                    });
                    return Expr {
                        kind: ExprKind::Await(arg),
                        span: self.span_from(start),
                    };
                }
                // If `await` is followed by a closing delimiter or statement
                // terminator, it has no argument (error recovery context like
                // default parameter `a = await)`).  Emit an Omitted arg.
                let arg = if self.is_eof()
                    || matches!(
                        self.cur(),
                        TokenKind::CloseParen
                            | TokenKind::CloseBracket
                            | TokenKind::Comma
                            | TokenKind::Semicolon
                    ) {
                    Box::new(Expr {
                        kind: ExprKind::Omitted,
                        span: self.span_from(start),
                    })
                } else {
                    Box::new(self.parse_unary_expr())
                };
                Expr {
                    kind: ExprKind::Await(arg),
                    span: self.span_from(start),
                }
            }
            TokenKind::PlusPlus => {
                self.bump();
                // TypeScript uses parseSimpleUnaryExpression for the operand of prefix ++/--.
                // This recurses through other unary operators (typeof, void, delete, -, ~, !)
                // but does NOT check for postfix ++/-- on the result. So `++x++` does NOT
                // parse as `++(x++)` — instead TypeScript splits it into `++x; ++;`.
                // We use parse_simple_unary_expr which skips postfix check.
                //
                // Error recovery: `++ delete foo.bar` → `++; delete foo.bar;`
                // When the next token is a keyword unary operator (delete, typeof, void)
                // or another prefix ++/--, produce an empty operand and let the next
                // token start a new expression/statement.
                let arg = if matches!(
                    self.cur(),
                    TokenKind::Delete
                        | TokenKind::TypeOf
                        | TokenKind::Void
                        | TokenKind::PlusPlus
                        | TokenKind::MinusMinus
                ) {
                    Box::new(Expr {
                        kind: ExprKind::Ident("".into()),
                        span: self.cur_span(),
                    })
                } else {
                    Box::new(self.parse_simple_unary_expr())
                };
                Expr {
                    kind: ExprKind::Update(UpdateExpr {
                        op: UpdateOp::PreInc,
                        argument: arg,
                    }),
                    span: self.span_from(start),
                }
            }
            TokenKind::MinusMinus => {
                self.bump();
                let arg = if matches!(
                    self.cur(),
                    TokenKind::Delete
                        | TokenKind::TypeOf
                        | TokenKind::Void
                        | TokenKind::PlusPlus
                        | TokenKind::MinusMinus
                ) {
                    Box::new(Expr {
                        kind: ExprKind::Ident("".into()),
                        span: self.cur_span(),
                    })
                } else {
                    Box::new(self.parse_simple_unary_expr())
                };
                Expr {
                    kind: ExprKind::Update(UpdateExpr {
                        op: UpdateOp::PreDec,
                        argument: arg,
                    }),
                    span: self.span_from(start),
                }
            }
            TokenKind::LessThan => {
                // Try JSX before type assertion
                if self.is_jsx_start() {
                    return self.parse_postfix_expr();
                }
                // Type assertion <T>expr
                if self.try_parse_type_assertion() {
                    let saved = self.pos;
                    self.bump(); // <
                    if let Some(ty) = self.try_parse_type_for_assertion() {
                        let has_gt = self.eat(TokenKind::GreaterThan).is_some();
                        if has_gt || self.can_recover_missing_type_assertion_gt() {
                            let expr = if self.is_type_assertion_expr_terminator() {
                                Box::new(Expr {
                                    kind: ExprKind::Omitted,
                                    span: self.cur_span(),
                                })
                            } else {
                                Box::new(self.parse_unary_expr())
                            };
                            return Expr {
                                kind: ExprKind::TypeAssertion(Box::new(TypeAssertionExpr {
                                    type_node: ty,
                                    expr,
                                })),
                                span: self.span_from(start),
                            };
                        }
                    }
                    self.rewind_to(saved);
                }
                self.parse_postfix_expr()
            }
            _ => self.parse_postfix_expr(),
        }
    }

    fn await_type_prefix_has_comma(&self) -> bool {
        debug_assert!(self.at(TokenKind::LessThan));
        let mut angle_depth = 0u32;
        let mut paren_depth = 0u32;
        let mut bracket_depth = 0u32;
        let mut brace_depth = 0u32;
        for token in self.tokens.iter().skip(self.pos + 1) {
            if token.preceded_by_line_break {
                return false;
            }
            match token.kind {
                TokenKind::LessThan => angle_depth += 1,
                TokenKind::GreaterThan if angle_depth > 0 => angle_depth -= 1,
                TokenKind::GreaterThan if angle_depth == 0 => return false,
                TokenKind::OpenParen => paren_depth += 1,
                TokenKind::CloseParen if paren_depth > 0 => paren_depth -= 1,
                TokenKind::OpenBracket => bracket_depth += 1,
                TokenKind::CloseBracket if bracket_depth > 0 => bracket_depth -= 1,
                TokenKind::OpenBrace => brace_depth += 1,
                TokenKind::CloseBrace if brace_depth > 0 => brace_depth -= 1,
                TokenKind::Comma
                    if angle_depth == 0
                        && paren_depth == 0
                        && bracket_depth == 0
                        && brace_depth == 0 =>
                {
                    return true;
                }
                TokenKind::Semicolon | TokenKind::EndOfFile => return false,
                _ => {}
            }
        }
        false
    }

    fn parse_postfix_expr(&mut self) -> Expr {
        let start = self.cur_span().start;
        let mut expr = self.parse_left_hand_side_expr();

        if !self.is_on_new_line() {
            if self.at(TokenKind::PlusPlus) {
                self.bump();
                expr = Expr {
                    kind: ExprKind::Update(UpdateExpr {
                        op: UpdateOp::PostInc,
                        argument: Box::new(expr),
                    }),
                    span: self.span_from(start),
                };
            } else if self.at(TokenKind::MinusMinus) {
                self.bump();
                expr = Expr {
                    kind: ExprKind::Update(UpdateExpr {
                        op: UpdateOp::PostDec,
                        argument: Box::new(expr),
                    }),
                    span: self.span_from(start),
                };
            }
        }

        expr
    }

    /// Like `parse_unary_expr` but skips postfix `++`/`--` check.
    /// Used for the operand of prefix `++`/`--` to match TypeScript's
    /// `parseSimpleUnaryExpression` which doesn't recurse through
    /// `parseUpdateExpression` (and thus doesn't add postfix operators).
    fn parse_simple_unary_expr(&mut self) -> Expr {
        // Handle other unary operators (typeof, void, delete, -, ~, !, await)
        // but NOT prefix ++/-- (those would recurse) and NOT postfix.
        match self.cur() {
            TokenKind::TypeOf
            | TokenKind::Void
            | TokenKind::Delete
            | TokenKind::Plus
            | TokenKind::Minus
            | TokenKind::Tilde
            | TokenKind::Excl
            | TokenKind::Await => {
                // These go through the normal unary path
                self.parse_unary_expr()
            }
            _ => {
                // For other cases, parse LHS without postfix check
                self.parse_left_hand_side_expr()
            }
        }
    }

    fn parse_element_access_index(&mut self) -> Box<Expr> {
        if self.at(TokenKind::CloseBracket) {
            // Recover `expr[]` without letting the following token sequence
            // (often a comma starting the next object-literal member) get
            // absorbed into the index expression.
            self.error_code(
                1011,
                "An element access expression should take an argument.".into(),
            );
            let close_bracket = self.bump();
            Box::new(Expr {
                kind: ExprKind::Omitted,
                span: Span::new(close_bracket.start, close_bracket.start),
            })
        } else {
            let index = Box::new(self.parse_expression());
            self.expect(TokenKind::CloseBracket);
            index
        }
    }

    /// Apply the call/member chain loop on an already-parsed expression.
    fn parse_left_hand_side_expr(&mut self) -> Expr {
        let start = self.cur_span().start;
        let mut expr = if self.at(TokenKind::New) {
            self.parse_new_expr()
        } else {
            self.parse_primary_expr()
        };

        loop {
            let cur = self.cur();
            match cur {
                TokenKind::Dot => {
                    self.bump();
                    // Handle private field access: obj.#field
                    let prop: AstString = if self.at(TokenKind::Hash) {
                        let hash_span = self.bump();
                        if self.private_name_can_consume_identifier(hash_span.end) {
                            if let Some((name, _)) = self.parse_identifier_name() {
                                format!("#{}", name).into()
                            } else {
                                self.error_code(1003, "Identifier expected.".into());
                                "#".into()
                            }
                        } else {
                            self.error_code(1003, "Identifier expected.".into());
                            "#".into()
                        }
                    } else {
                        // When the token after the dot is on a new line and is a
                        // statement-starting keyword (var, let, const, function,
                        // class, enum), don't consume it as a property name.
                        // This matches tsc's error recovery for cases like
                        // `expr.\nvar y = 1;`.
                        let is_newline_stmt_kw = self.is_on_new_line()
                            && matches!(
                                self.cur(),
                                TokenKind::Var
                                    | TokenKind::Let
                                    | TokenKind::Const
                                    | TokenKind::Function
                                    | TokenKind::Class
                                    | TokenKind::Enum
                                    | TokenKind::Namespace
                                    | TokenKind::Module
                            );
                        if is_newline_stmt_kw {
                            "<error>".into()
                        } else if let Some((name, _)) = self.parse_identifier_name() {
                            name
                        } else {
                            self.error_code(1003, "Identifier expected.".into());
                            "<error>".into()
                        }
                    };
                    expr = Expr {
                        kind: ExprKind::Member(Box::new(MemberExpr {
                            object: Box::new(expr),
                            property: prop,
                            optional: false,
                        })),
                        span: self.span_from(start),
                    };
                }
                TokenKind::QuestionDot => {
                    self.bump();
                    if self.at(TokenKind::OpenBracket) {
                        self.bump();
                        let index = self.parse_element_access_index();
                        expr = Expr {
                            kind: ExprKind::ElemAccess(ElemAccessExpr {
                                object: Box::new(expr),
                                index,
                                optional: true,
                            }),
                            span: self.span_from(start),
                        };
                    } else if self.at(TokenKind::OpenParen) || self.at(TokenKind::LessThan) {
                        // Optional call: x?.() or x?.<T>()
                        let type_args = self.try_parse_type_args();
                        let args = if self.at(TokenKind::OpenParen) {
                            self.parse_arguments()
                        } else {
                            Vec::new()
                        };
                        expr = Expr {
                            kind: ExprKind::Call(Box::new(CallExpr {
                                callee: Box::new(expr),
                                type_args,
                                args,
                                optional: true,
                            })),
                            span: self.span_from(start),
                        };
                    } else {
                        let prop: AstString = if self.at(TokenKind::Hash) {
                            let hash_span = self.bump();
                            if self.private_name_can_consume_identifier(hash_span.end) {
                                if let Some((name, _)) = self.parse_identifier_name() {
                                    format!("#{}", name).into()
                                } else {
                                    self.error_code(1003, "Identifier expected.".into());
                                    "#".into()
                                }
                            } else {
                                self.error_code(1003, "Identifier expected.".into());
                                "#".into()
                            }
                        } else if let Some((name, _)) = self.parse_identifier_name() {
                            name
                        } else {
                            self.error_code(1003, "Identifier expected.".into());
                            "<error>".into()
                        };
                        expr = Expr {
                            kind: ExprKind::Member(Box::new(MemberExpr {
                                object: Box::new(expr),
                                property: prop,
                                optional: true,
                            })),
                            span: self.span_from(start),
                        };
                    }
                }
                TokenKind::OpenBracket => {
                    self.bump();
                    let index = self.parse_element_access_index();
                    expr = Expr {
                        kind: ExprKind::ElemAccess(ElemAccessExpr {
                            object: Box::new(expr),
                            index,
                            optional: false,
                        }),
                        span: self.span_from(start),
                    };
                }
                TokenKind::OpenParen => {
                    let args = self.parse_arguments();
                    expr = Expr {
                        kind: ExprKind::Call(Box::new(CallExpr {
                            callee: Box::new(expr),
                            type_args: None,
                            args,
                            optional: false,
                        })),
                        span: self.span_from(start),
                    };
                }
                // Try type arguments for generic call.
                TokenKind::LessThan | TokenKind::LessLess => {
                    // Skip if LHS is a literal (comparison, not generic)
                    if matches!(
                        expr.kind,
                        ExprKind::NumLit(_)
                            | ExprKind::BigIntLit(_)
                            | ExprKind::StrLit(_)
                            | ExprKind::BoolLit(_)
                            | ExprKind::NullLit
                    ) {
                        break;
                    }
                    // `<<` only for generic after identifier/member (foo<<T>)
                    if cur == TokenKind::LessLess
                        && !matches!(expr.kind, ExprKind::Ident(_) | ExprKind::Member(_))
                    {
                        break;
                    }
                    // Save tokens that might be mutated by >>-splitting in
                    // eat_greater_than so we can restore them on rollback.
                    let saved = self.pos;
                    let saved_diag_len = self.diagnostics.len();
                    let split_mark = self.type_arg_split_rollback.len();
                    // Keep the split records alive after try_parse_type_args
                    // succeeds: this expression-level speculation may still
                    // reject the parsed arguments based on the following
                    // token (class heritage is a common example).
                    self.type_arg_split_rollback_depth += 1;
                    if let Some(type_args) = self.try_parse_type_args() {
                        if self.at(TokenKind::OpenParen) {
                            let args = self.parse_arguments();
                            expr = Expr {
                                kind: ExprKind::Call(Box::new(CallExpr {
                                    callee: Box::new(expr),
                                    type_args: Some(type_args),
                                    args,
                                    optional: false,
                                })),
                                span: self.span_from(start),
                            };
                            self.type_arg_split_rollback_depth -= 1;
                            if self.type_arg_split_rollback_depth == 0 {
                                self.type_arg_split_rollback.truncate(split_mark);
                            }
                            continue;
                        }
                        if self.at(TokenKind::NoSubstitutionTemplate)
                            || self.at(TokenKind::TemplateHead)
                        {
                            // Tagged template with type args
                            let quasi = self.parse_template_literal(true);
                            expr = Expr {
                                kind: ExprKind::TaggedTemplate(Box::new(TaggedTemplateLit {
                                    tag: Box::new(expr),
                                    quasi,
                                    type_args: Some(type_args),
                                })),
                                span: self.span_from(start),
                            };
                            self.type_arg_split_rollback_depth -= 1;
                            if self.type_arg_split_rollback_depth == 0 {
                                self.type_arg_split_rollback.truncate(split_mark);
                            }
                            continue;
                        }
                        // Instantiation expression: expr<TypeArgs> not followed
                        // by ( or template. Only commit when the current token
                        // cannot start a new sub-expression.
                        if self.can_follow_type_arguments_in_expression() {
                            expr = Expr {
                                kind: ExprKind::Instantiation(Box::new(InstantiationExpr {
                                    expr: Box::new(expr),
                                    type_args,
                                })),
                                span: self.span_from(start),
                            };
                            self.type_arg_split_rollback_depth -= 1;
                            if self.type_arg_split_rollback_depth == 0 {
                                self.type_arg_split_rollback.truncate(split_mark);
                            }
                            continue;
                        }
                    }
                    self.rewind_to(saved);
                    let rollback: Vec<_> =
                        self.type_arg_split_rollback.drain(split_mark..).collect();
                    for (idx, tok) in rollback.into_iter().rev() {
                        self.replace_token(idx, tok);
                    }
                    self.type_arg_split_rollback_depth -= 1;
                    // Discard errors from failed speculative type arg parse
                    self.diagnostics.truncate(saved_diag_len);
                    break;
                }
                TokenKind::NoSubstitutionTemplate | TokenKind::TemplateHead => {
                    let quasi_start = self.cur_span().start;
                    let missing_tag = matches!(&expr.kind, ExprKind::Ident(name) if name == "<error>" || name.is_empty());
                    let quasi = self.parse_template_literal(!missing_tag);
                    if missing_tag {
                        expr.span = Span::new(quasi_start, quasi_start);
                    }
                    let tagged_start = if missing_tag { quasi_start } else { start };
                    expr = Expr {
                        kind: ExprKind::TaggedTemplate(Box::new(TaggedTemplateLit {
                            tag: Box::new(expr),
                            quasi,
                            type_args: None,
                        })),
                        span: self.span_from(tagged_start),
                    };
                }
                // Non-null assertion postfix `!` — parse inside the
                // call/member chain so `expr!(args)` and `expr!.prop`
                // continue as a single expression.
                TokenKind::Excl if !self.is_on_new_line() => {
                    self.bump();
                    expr = Expr {
                        kind: ExprKind::NonNull(Box::new(expr)),
                        span: self.span_from(start),
                    };
                }
                _ => break,
            }
        }

        expr
    }
    fn parse_new_expr(&mut self) -> Expr {
        let start = self.cur_span().start;
        self.bump(); // new

        if self.at(TokenKind::Dot) {
            self.bump();
            let (prop, _) = self.parse_identifier();
            return Expr {
                kind: ExprKind::MetaProp(Box::new(MetaPropExpr {
                    meta: "new".into(),
                    property: prop.into(),
                })),
                span: self.span_from(start),
            };
        }

        let callee = if self.at(TokenKind::New) {
            self.parse_new_expr()
        } else if self.at(TokenKind::LessThan) && !self.is_jsx_start() {
            self.parse_unary_expr()
        } else {
            self.parse_primary_expr_with_member_access()
        };

        let type_args = self.try_parse_type_args();
        let args = if self.at(TokenKind::OpenParen) {
            Some(self.parse_arguments())
        } else {
            None
        };

        Expr {
            kind: ExprKind::New(Box::new(NewExpr {
                callee: Box::new(callee),
                type_args,
                args,
            })),
            span: self.span_from(start),
        }
    }

    fn parse_primary_expr_with_member_access(&mut self) -> Expr {
        let start = self.cur_span().start;
        let mut expr = self.parse_primary_expr();
        loop {
            let cur = self.cur();
            match cur {
                TokenKind::Dot => {
                    self.bump();
                    // Handle private field access: obj.#field
                    let prop: AstString = if self.at(TokenKind::Hash) {
                        let hash_span = self.bump();
                        if self.private_name_can_consume_identifier(hash_span.end) {
                            if let Some((name, _)) = self.parse_identifier_name() {
                                format!("#{}", name).into()
                            } else {
                                self.error_code(1003, "Identifier expected.".into());
                                "#".into()
                            }
                        } else {
                            self.error_code(1003, "Identifier expected.".into());
                            "#".into()
                        }
                    } else if let Some((name, _)) = self.parse_identifier_name() {
                        name
                    } else {
                        self.error_code(1003, "Identifier expected.".into());
                        "<error>".into()
                    };
                    expr = Expr {
                        kind: ExprKind::Member(Box::new(MemberExpr {
                            object: Box::new(expr),
                            property: prop,
                            optional: false,
                        })),
                        span: self.span_from(start),
                    };
                }
                TokenKind::OpenBracket => {
                    self.bump();
                    let index = self.parse_element_access_index();
                    expr = Expr {
                        kind: ExprKind::ElemAccess(ElemAccessExpr {
                            object: Box::new(expr),
                            index,
                            optional: false,
                        }),
                        span: self.span_from(start),
                    };
                }
                _ => break,
            }
        }
        expr
    }

    fn parse_primary_expr(&mut self) -> Expr {
        let start = self.cur_span().start;
        match self.cur() {
            TokenKind::NumericLiteral => {
                let span = self.bump();
                Expr {
                    kind: ExprKind::NumLit(self.text(span).into()),
                    span,
                }
            }
            TokenKind::BigIntLiteral => {
                let span = self.bump();
                Expr {
                    kind: ExprKind::BigIntLit(self.text(span).into()),
                    span,
                }
            }
            TokenKind::StringLiteral => {
                let span = self.bump_string_literal();
                let raw = self.text(span);
                Expr {
                    kind: ExprKind::StrLit(Self::string_literal_content(raw).into()),
                    span,
                }
            }
            TokenKind::NoSubstitutionTemplate => {
                let span = self.bump_template_chunk(false);
                let raw = self.text(span);
                let content = if raw.len() >= 2 {
                    &raw[1..raw.len() - 1]
                } else {
                    ""
                };
                Expr {
                    kind: ExprKind::NoSubstTemplate(content.into()),
                    span,
                }
            }
            TokenKind::TemplateHead => {
                let tpl = self.parse_template_literal(false);
                Expr {
                    kind: ExprKind::Template(Box::new(tpl)),
                    span: self.span_from(start),
                }
            }
            TokenKind::True => {
                self.bump();
                Expr {
                    kind: ExprKind::BoolLit(true),
                    span: self.span_from(start),
                }
            }
            TokenKind::False => {
                self.bump();
                Expr {
                    kind: ExprKind::BoolLit(false),
                    span: self.span_from(start),
                }
            }
            TokenKind::Null => {
                self.bump();
                Expr {
                    kind: ExprKind::NullLit,
                    span: self.span_from(start),
                }
            }
            TokenKind::This => {
                self.bump();
                Expr {
                    kind: ExprKind::This,
                    span: self.span_from(start),
                }
            }
            TokenKind::Super => {
                self.bump();
                Expr {
                    kind: ExprKind::Super,
                    span: self.span_from(start),
                }
            }
            TokenKind::OpenParen => {
                self.bump();
                if self.at(TokenKind::CloseParen) {
                    // empty parens - this is arrow params, will be handled
                    self.bump();
                    Expr {
                        kind: ExprKind::Paren(Box::new(Expr {
                            kind: ExprKind::Omitted,
                            span: self.span_from(start),
                        })),
                        span: self.span_from(start),
                    }
                } else if self.at(TokenKind::Comma) {
                    // Error recovery for malformed comma expressions like
                    // `(, ANY)` and `(, )`. The leading operand is missing —
                    // synthesize an Omitted, then collect the rest as a
                    // standard comma chain (which itself tolerates trailing
                    // empty operands via parse_expression below).
                    let inner_start = self.cur_span().start;
                    let mut exprs: Vec<Box<Expr>> = vec![Box::new(Expr {
                        kind: ExprKind::Omitted,
                        span: Span::new(inner_start, inner_start),
                    })];
                    while self.eat(TokenKind::Comma).is_some() {
                        if self.at(TokenKind::CloseParen) {
                            let pos = self.cur_span().start;
                            exprs.push(Box::new(Expr {
                                kind: ExprKind::Omitted,
                                span: Span::new(pos, pos),
                            }));
                            break;
                        }
                        exprs.push(Box::new(self.parse_assignment_expr()));
                    }
                    let inner = Expr {
                        kind: ExprKind::Comma(exprs),
                        span: self.span_from(inner_start),
                    };
                    self.expect(TokenKind::CloseParen);
                    Expr {
                        kind: ExprKind::Paren(Box::new(inner)),
                        span: self.span_from(start),
                    }
                } else {
                    let expr = self.parse_expression_with_trailing_comma();
                    self.expect(TokenKind::CloseParen);
                    Expr {
                        kind: ExprKind::Paren(Box::new(expr)),
                        span: self.span_from(start),
                    }
                }
            }
            TokenKind::OpenBracket => self.parse_array_literal(start),
            TokenKind::OpenBrace => self.parse_object_literal(start),
            TokenKind::Function => self.parse_function_expr(start),
            TokenKind::Async if self.peek_is(TokenKind::Function) => {
                self.parse_function_expr(start)
            }
            TokenKind::Class => {
                let decl = self.parse_class_decl(MOD_NONE, Vec::new(), start);
                if let StmtKind::ClassDecl(cd) = decl.kind {
                    Expr {
                        kind: ExprKind::ClassExpr(cd),
                        span: self.span_from(start),
                    }
                } else {
                    Expr {
                        kind: ExprKind::Ident("<error>".into()),
                        span: self.span_from(start),
                    }
                }
            }
            TokenKind::At => {
                let saved = self.pos;
                let decorators = self.parse_decorators();
                let modifiers = self.parse_modifiers();
                if self.at(TokenKind::Class) {
                    // TC39 decorators on class expressions: `@dec class {}`
                    let decl = self.parse_class_decl(modifiers, decorators, start);
                    if let StmtKind::ClassDecl(cd) = decl.kind {
                        Expr {
                            kind: ExprKind::ClassExpr(cd),
                            span: self.span_from(start),
                        }
                    } else {
                        Expr {
                            kind: ExprKind::Ident("<error>".into()),
                            span: self.span_from(start),
                        }
                    }
                } else {
                    // Error recovery: `@dec expr` where `expr` is not a class.
                    // Restore position, skip `@` and the decorator identifier.
                    if self.is_js_file {
                        if let Some(decorator) = decorators.first() {
                            let span = self.decorator_diagnostic_span(decorator, true);
                            self.error_at_span(
                                1206,
                                "Decorators are not valid here.".to_string(),
                                span,
                            );
                        }
                    }
                    self.rewind_to(saved);
                    self.error_code(1128, "Declaration or statement expected.".into());
                    self.bump(); // skip `@`
                                 // Skip the decorator name identifier (but not keywords like
                                 // `#` or other special tokens that might be meaningful).
                    if self.at(TokenKind::Identifier) {
                        self.bump();
                        // Also skip call parens if present: `@dec(args)`
                        if self.at(TokenKind::OpenParen) {
                            // Consume balanced parens
                            let mut depth = 0i32;
                            loop {
                                if self.is_eof() {
                                    break;
                                }
                                if self.at(TokenKind::OpenParen) {
                                    depth += 1;
                                } else if self.at(TokenKind::CloseParen) {
                                    depth -= 1;
                                    if depth <= 0 {
                                        self.bump(); // consume `)`
                                        break;
                                    }
                                }
                                self.bump();
                            }
                        }
                    }
                    let cur_pos = self.cur_span().start;
                    Expr {
                        kind: ExprKind::Ident("<error>".into()),
                        span: Span {
                            start: cur_pos,
                            end: cur_pos,
                        },
                    }
                }
            }
            TokenKind::DotDotDot => {
                self.bump();
                let arg = Box::new(self.parse_assignment_expr());
                Expr {
                    kind: ExprKind::Spread(arg),
                    span: self.span_from(start),
                }
            }
            TokenKind::Slash | TokenKind::SlashEquals => {
                // Re-scan as regex
                self.parse_regexp_literal(start)
            }
            TokenKind::RegExpLiteral => {
                // Scanner already produced a complete regex token.
                let span = self.bump();
                let raw = self.text(span);
                let (body, flags) = if let Some(last_slash) = raw.rfind('/') {
                    if last_slash > 0 {
                        (&raw[1..last_slash], &raw[last_slash + 1..])
                    } else {
                        (&raw[1..], "")
                    }
                } else {
                    (&raw[1..], "")
                };
                Expr {
                    kind: ExprKind::RegexpLit(Box::new(RegexpLitExpr {
                        pattern: body.to_string().into(),
                        flags: flags.to_string().into(),
                    })),
                    span,
                }
            }
            TokenKind::Import => {
                // `var x = import { foo } from './0'` — an import DECLARATION
                // in expression position. tsc leaves the expression missing
                // (`var x = ;`) and re-parses the import as a statement (which
                // then elides normally). Don't consume anything.
                if self.peek_is(TokenKind::OpenBrace) {
                    self.error_code(1109, "Expression expected.".into());
                    return Expr {
                        kind: ExprKind::Omitted,
                        span: Span::new(start, start),
                    };
                }
                self.bump();
                if self.at(TokenKind::Dot) {
                    self.bump();
                    let (prop, _) = self.parse_identifier();
                    Expr {
                        kind: ExprKind::MetaProp(Box::new(MetaPropExpr {
                            meta: "import".into(),
                            property: prop.into(),
                        })),
                        span: self.span_from(start),
                    }
                } else {
                    // dynamic import: import(source) or import(source, options)
                    // Skip type arguments if present: import<T>("./0")
                    // Only consume type args when followed by `(` to avoid
                    // greedily consuming `<` in error recovery contexts like
                    // `import<string, number>` without a call.
                    if self.at(TokenKind::LessThan) {
                        let saved = self.pos;
                        let saved_tok = self.tokens.get(self.pos).cloned();
                        if self.try_parse_type_args().is_some() && !self.at(TokenKind::OpenParen) {
                            // Type args parsed but no `(` follows — backtrack
                            self.rewind_to(saved);
                            if let Some(tok) = saved_tok {
                                self.replace_token(saved, tok);
                            }
                        }
                    }
                    self.expect(TokenKind::OpenParen);
                    let arg = self.parse_assignment_expr();
                    let mut args = vec![Box::new(arg)];
                    // Parse optional additional arguments (import attributes/options,
                    // plus any extra error arguments)
                    while self.eat(TokenKind::Comma).is_some() && !self.at(TokenKind::CloseParen) {
                        args.push(Box::new(self.parse_assignment_expr()));
                    }
                    self.expect(TokenKind::CloseParen);
                    Expr {
                        kind: ExprKind::Call(Box::new(CallExpr {
                            callee: Box::new(Expr {
                                kind: ExprKind::Ident("import".into()),
                                span: self.span_from(start),
                            }),
                            type_args: None,
                            args,
                            optional: false,
                        })),
                        span: self.span_from(start),
                    }
                }
            }
            TokenKind::LessThan if self.is_tsx && self.peek_is(TokenKind::Colon) => {
                // Recovery for malformed TSX like `<:a .../>`:
                // consume `<` as an error expression and reinterpret the following
                // `:` as `,` so var declarator recovery can continue as tsc does.
                self.bump();
                if self.at(TokenKind::Colon) {
                    self.set_token_kind(self.pos, TokenKind::Comma);
                }
                let missing_left = Expr {
                    kind: ExprKind::Omitted,
                    span: Span::new(start, start),
                };
                let missing_right = Expr {
                    kind: ExprKind::Omitted,
                    span: Span::new(start, start),
                };
                Expr {
                    kind: ExprKind::Binary(BinaryExpr {
                        left: Box::new(missing_left),
                        op: BinaryOp::Lt,
                        right: Box::new(missing_right),
                    }),
                    span: self.span_from(start),
                }
            }
            TokenKind::LessThan if self.is_jsx_start() => self.parse_jsx_element_or_fragment(start),
            TokenKind::GreaterThan => {
                // Recovery: when `>` appears in primary-expression position
                // (e.g. as RHS of `1 > > 2;`), check whether a valid
                // expression follows. If so, return Omitted WITHOUT consuming
                // the `>` so the binary-expression parser treats it as an
                // operator, keeping `2` in the same expression.
                let next_could_start_expr = self.tokens.get(self.pos + 1).is_some_and(|t| {
                    matches!(
                        t.kind,
                        TokenKind::NumericLiteral
                            | TokenKind::StringLiteral
                            | TokenKind::BigIntLiteral
                            | TokenKind::NoSubstitutionTemplate
                            | TokenKind::TemplateHead
                            | TokenKind::True
                            | TokenKind::False
                            | TokenKind::Null
                            | TokenKind::This
                            | TokenKind::Super
                            | TokenKind::OpenParen
                            | TokenKind::OpenBracket
                            | TokenKind::OpenBrace
                            | TokenKind::Function
                            | TokenKind::Class
                            | TokenKind::New
                            | TokenKind::Void
                            | TokenKind::TypeOf
                            | TokenKind::Delete
                            | TokenKind::Plus
                            | TokenKind::Minus
                            | TokenKind::Excl
                            | TokenKind::Tilde
                            | TokenKind::PlusPlus
                            | TokenKind::MinusMinus
                            | TokenKind::Slash
                            | TokenKind::SlashEquals
                            | TokenKind::Identifier
                            | TokenKind::Async
                            | TokenKind::Await
                            | TokenKind::Yield
                            | TokenKind::Let
                    )
                });
                if next_could_start_expr {
                    // Don't consume `>` — the binary expression parser will
                    // handle it as an operator.
                    Expr {
                        kind: ExprKind::Omitted,
                        span: Span::new(start, start),
                    }
                } else {
                    // No valid RHS follows: consume `>` and produce a
                    // standalone `> ` expression for cases like `> ;`.
                    self.bump();
                    let missing_left = Expr {
                        kind: ExprKind::Omitted,
                        span: Span::new(start, start),
                    };
                    let missing_right = Expr {
                        kind: ExprKind::Omitted,
                        span: Span::new(start, start),
                    };
                    Expr {
                        kind: ExprKind::Binary(BinaryExpr {
                            left: Box::new(missing_left),
                            op: BinaryOp::Gt,
                            right: Box::new(missing_right),
                        }),
                        span: self.span_from(start),
                    }
                }
            }
            // Standalone private identifier: `#field in obj` (ergonomic brand check)
            TokenKind::Hash => {
                let hash_span = self.bump();
                if self.private_name_can_consume_identifier(hash_span.end) {
                    if let Some((name, _)) = self.parse_identifier_name() {
                        Expr {
                            kind: ExprKind::Ident(format!("#{}", name).into()),
                            span: self.span_from(start),
                        }
                    } else {
                        self.error_code(1003, "Identifier expected.".into());
                        Expr {
                            kind: ExprKind::Ident(
                                if self.at(TokenKind::Excl) {
                                    "<error>"
                                } else {
                                    "#"
                                }
                                .into(),
                            ),
                            span: self.span_from(start),
                        }
                    }
                } else {
                    self.error_code(1003, "Identifier expected.".into());
                    Expr {
                        kind: ExprKind::Ident(
                            if self.at(TokenKind::Excl) {
                                "<error>"
                            } else {
                                "#"
                            }
                            .into(),
                        ),
                        span: self.span_from(start),
                    }
                }
            }
            _ if self.is_identifier() => {
                let span = self.bump();
                Expr {
                    kind: ExprKind::Ident(self.text(span).into()),
                    span,
                }
            }
            _ => {
                // Don't consume declaration keywords — they should be left
                // for the statement parser. This prevents `@enum E {}` from
                // consuming `enum` as a decorator expression.
                if matches!(
                    self.cur(),
                    TokenKind::Enum
                        | TokenKind::Interface
                        | TokenKind::Namespace
                        | TokenKind::Finally
                        | TokenKind::Catch
                        // Statement keywords should never be primary expressions.
                        | TokenKind::Return
                        | TokenKind::Break
                        | TokenKind::Continue
                        | TokenKind::Throw
                        // Declaration keywords should not be consumed as expressions
                        // (e.g. `var x = var y` → `var x = ;` then `var y;`).
                        | TokenKind::Var
                        // Binary operators should never be primary expressions.
                        | TokenKind::BarBar
                        | TokenKind::AmpersandAmpersand
                        | TokenKind::QuestionQuestion
                ) || (
                    // Module should not be consumed as identifier UNLESS
                    // followed by a template literal (tagged template).
                    matches!(self.cur(), TokenKind::Module)
                        && !self.peek_is(TokenKind::NoSubstitutionTemplate)
                        && !self.peek_is(TokenKind::TemplateHead)
                ) {
                    let sp = self.cur_span();
                    return Expr {
                        kind: ExprKind::Ident("<error>".into()),
                        span: Span {
                            start: sp.start,
                            end: sp.start,
                        },
                    };
                }
                self.error_code(1109, "Expression expected.".into());
                let span = self.bump();
                Expr {
                    kind: ExprKind::Ident("<error>".into()),
                    span,
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Arrow function
    // -----------------------------------------------------------------------

    fn try_parse_arrow_function(&mut self) -> Option<Expr> {
        // Most assignment expressions cannot start an arrow. Reject them before
        // setting up rollback state or repeating the candidate checks below.
        // `async`, `(`, and `<` need the full path; other identifier-like tokens
        // only do when immediately followed by `=>`.
        let first = self.cur();
        if first != TokenKind::Async
            && first != TokenKind::OpenParen
            && first != TokenKind::LessThan
            && (!self.is_identifier() || !self.peek_is(TokenKind::FatArrow))
        {
            return None;
        }

        let start = self.cur_span().start;

        // In TSX, a single unconstrained type parameter is parsed as a JSX
        // tag even when `()` and `=>` follow.  A constraint or a top-level
        // comma disambiguates the construct as a generic arrow.  Invalid
        // `extends=` / `extends>` forms remain JSX recovery shapes.
        if self.is_tsx && self.at(TokenKind::LessThan) && !self.tsx_type_params_disambiguate_arrow()
        {
            return None;
        }

        // Cheap lookahead first for parenthesized/type-parameter candidates.
        // Most `(` expressions are not arrows; avoiding rollback bookkeeping
        // until after this pass keeps the common case cheaper.
        if (self.at(TokenKind::OpenParen) || self.at(TokenKind::LessThan))
            && !self.looks_like_arrow_params()
        {
            return None;
        }

        let saved = self.pos;
        // Discard diagnostics emitted while speculatively parsing a param list
        // that turns out NOT to be an arrow (rolled back below). Without this,
        // `looks_like_arrow_params` can mis-commit on a ternary with a
        // parenthesized consequent — `a ? (b ? c : d) : e` parses `(b?` as an
        // optional param, leaks a spurious "expected CloseParen" (TS1002), then
        // rolls back. The stray diagnostic flips file_has_recovery_errors on,
        // which disables the emitter's recovery fast-outs for the whole file.
        // Same rollback-truncate pattern used by the other speculative parses.
        let saved_diag_len = self.diagnostics.len();

        let async_span = self.eat(TokenKind::Async);
        let is_async = async_span.is_some();

        // `async =>` is a non-async arrow with parameter named "async"
        if is_async && self.at(TokenKind::FatArrow) {
            let name_span = async_span.unwrap();
            self.bump();
            let body = self.parse_arrow_body();
            return Some(Expr {
                kind: ExprKind::Arrow(Box::new(ArrowFn {
                    type_params: None,
                    params: vec![Param {
                        name: Pat {
                            kind: PatKind::Ident("async".into()),
                            span: name_span,
                        },
                        type_ann: None,
                        initializer: None,
                        dotdotdot: false,
                        optional: false,
                        modifiers: MOD_NONE,
                        decorators: Vec::new(),
                        span: name_span,
                    }],
                    return_type: None,
                    body,
                    is_async: false,
                    span: self.span_from(start),
                })),
                span: self.span_from(start),
            });
        }

        // Simple: async? ident =>
        if self.is_identifier() && self.peek_is(TokenKind::FatArrow) {
            let (name, name_span) = self.parse_identifier();
            self.bump();
            let body = self.parse_arrow_body();
            return Some(Expr {
                kind: ExprKind::Arrow(Box::new(ArrowFn {
                    type_params: None,
                    params: vec![Param {
                        name: Pat {
                            kind: PatKind::Ident(name.into()),
                            span: name_span,
                        },
                        type_ann: None,
                        initializer: None,
                        dotdotdot: false,
                        optional: false,
                        modifiers: MOD_NONE,
                        decorators: Vec::new(),
                        span: name_span,
                    }],
                    return_type: None,
                    body,
                    is_async,
                    span: self.span_from(start),
                })),
                span: self.span_from(start),
            });
        }

        // Complex: (params) => or <T>(params) =>
        // Use a lookahead: check if the parens are followed by => before
        // committing to a full param parse.
        if self.at(TokenKind::LessThan)
            || (self.at(TokenKind::OpenParen) && (!is_async || self.looks_like_arrow_params()))
        {
            let type_params = self.try_parse_js_checked_type_params();
            if self.at(TokenKind::OpenParen) {
                self.speculation_depth += 1;
                let params = self.parse_param_list();
                self.speculation_depth -= 1;
                let has_ts_only_param_syntax = Self::params_have_ts_only_syntax(&params);
                let return_type = if self.eat(TokenKind::Colon).is_some() {
                    Some(self.parse_js_checked_return_type_annotation())
                } else {
                    None
                };
                if self.eat(TokenKind::FatArrow).is_some() {
                    let body = self.parse_arrow_body();
                    return Some(Expr {
                        kind: ExprKind::Arrow(Box::new(ArrowFn {
                            type_params,
                            params,
                            return_type,
                            body,
                            is_async,
                            span: self.span_from(start),
                        })),
                        span: self.span_from(start),
                    });
                }
                if self.at(TokenKind::OpenBrace) {
                    let body = ArrowBody::Block(self.parse_block_body());
                    return Some(Expr {
                        kind: ExprKind::Arrow(Box::new(ArrowFn {
                            type_params,
                            params,
                            return_type,
                            body,
                            is_async,
                            span: self.span_from(start),
                        })),
                        span: self.span_from(start),
                    });
                }
                if has_ts_only_param_syntax && self.at(TokenKind::Dot) {
                    let dot_start = self.cur_span().start;
                    self.bump();
                    let tail = if let Some((name, span)) = self.parse_identifier_name() {
                        Expr {
                            kind: ExprKind::Ident(name.into()),
                            span,
                        }
                    } else {
                        self.error_code(1003, "Identifier expected.".into());
                        Expr {
                            kind: ExprKind::Ident("<error>".into()),
                            span: self.cur_span(),
                        }
                    };
                    return Some(Expr {
                        kind: ExprKind::Arrow(Box::new(ArrowFn {
                            type_params,
                            params,
                            return_type,
                            body: ArrowBody::Expr(Box::new(Expr {
                                kind: ExprKind::Comma(vec![
                                    Box::new(Expr {
                                        kind: ExprKind::Omitted,
                                        span: Span::new(dot_start, dot_start),
                                    }),
                                    Box::new(tail),
                                ]),
                                span: self.span_from(dot_start),
                            })),
                            is_async,
                            span: self.span_from(start),
                        })),
                        span: self.span_from(start),
                    });
                }
                // Incomplete-arrow recovery: `(params): T` / `<T>(params)` at a
                // terminator with no `=>`. A bare `return_type.is_some()` is NOT
                // enough inside a ternary consequent — there `(expr): alt` is a
                // valid ternary, and the `:` is the ternary colon, not a return
                // type (e.g. `a ? (b) : c`). Real arrows-with-return-type in a
                // ternary reach the FatArrow path above, so excluding this here
                // only rejects the ambiguous no-`=>` case. ts-only param syntax
                // and type params stay unambiguous (can't be a paren expr).
                if (has_ts_only_param_syntax
                    || (return_type.is_some() && !self.in_ternary_consequent)
                    || (!is_async && type_params.as_ref().is_some_and(|tp| !tp.is_empty())))
                    && self.is_arrow_body_terminator()
                {
                    return Some(Expr {
                        kind: ExprKind::Arrow(Box::new(ArrowFn {
                            type_params,
                            params,
                            return_type,
                            body: ArrowBody::Expr(Box::new(Expr {
                                kind: ExprKind::Omitted,
                                span: self.cur_span(),
                            })),
                            is_async,
                            span: self.span_from(start),
                        })),
                        span: self.span_from(start),
                    });
                }
            }
        }

        // Not an arrow function
        self.rewind_to(saved);
        self.diagnostics.truncate(saved_diag_len);
        None
    }

    fn tsx_type_params_disambiguate_arrow(&self) -> bool {
        let mut i = self.pos + 1;
        let mut depth = 1i32;
        while i < self.tokens.len() && depth > 0 {
            match self.tokens[i].kind {
                TokenKind::LessThan => depth += 1,
                TokenKind::GreaterThan => depth -= 1,
                TokenKind::GreaterGreater => depth -= 2,
                TokenKind::GreaterGreaterGreater => depth -= 3,
                TokenKind::Comma if depth == 1 => return true,
                TokenKind::Extends if depth == 1 => {
                    return self.tokens.get(i + 1).is_some_and(|next| {
                        !matches!(next.kind, TokenKind::Equals | TokenKind::GreaterThan)
                    });
                }
                _ => {}
            }
            i += 1;
        }
        false
    }

    /// Lookahead: check if the current position is at `(...)` that is followed
    /// by `=>` or `: type =>`. This avoids mis-parsing grouping parens as arrow
    /// params (e.g., `(expr)` in an IIFE `(() => {})();`).
    fn looks_like_arrow_params(&self) -> bool {
        if !self.at(TokenKind::OpenParen) && !self.at(TokenKind::LessThan) {
            return false;
        }

        // Skip type params first.
        let mut i = self.pos;
        if self
            .tokens
            .get(i)
            .is_some_and(|t| t.kind == TokenKind::LessThan)
        {
            let mut depth: i32 = 1;
            i += 1;
            while i < self.tokens.len() && depth > 0 {
                match self.tokens[i].kind {
                    TokenKind::LessThan => depth += 1,
                    TokenKind::GreaterThan => depth -= 1,
                    // `>>` and `>>>` can close nested type params (e.g.,
                    // `<T extends Array<U>>` where `>>` closes both).
                    TokenKind::GreaterGreater => depth -= 2,
                    TokenKind::GreaterGreaterGreater => depth -= 3,
                    _ => {}
                }
                i += 1;
            }
        }

        // Now skip the paren-enclosed param list.
        let params_start = i;
        if self
            .tokens
            .get(i)
            .is_none_or(|t| t.kind != TokenKind::OpenParen)
        {
            return false;
        }
        i += 1; // skip (

        // Reject arrow parse when the first token inside `(` is obviously
        // not a valid parameter start (e.g., `(1) =>` should be a grouping
        // paren + comparison, not an arrow function).
        if let Some(first) = self.tokens.get(i) {
            if matches!(
                first.kind,
                TokenKind::NumericLiteral
                    | TokenKind::BigIntLiteral
                    | TokenKind::StringLiteral
                    | TokenKind::NoSubstitutionTemplate
                    | TokenKind::TemplateHead
                    | TokenKind::RegExpLiteral
                    | TokenKind::True
                    | TokenKind::False
                    | TokenKind::Null
                    // Double open paren `((` — grouping expression, not arrow params
                    | TokenKind::OpenParen
            ) {
                return false;
            }
            // In .tsx files, `(<` starts a JSX element inside parens, not
            // arrow function parameters.  e.g. `x ? (<div/>) : y`
            if self.is_tsx && first.kind == TokenKind::LessThan {
                return false;
            }
            // In ternary consequent, `({...} as T)` is a type assertion, not
            // destructured arrow params.  Scan ahead: if the brace-matched
            // content is followed by `as`/`)`, reject arrow; if by `)` `=>`
            // keep as arrow.
            if self.in_ternary_consequent && first.kind == TokenKind::OpenBrace {
                // Scan through the brace-enclosed content
                let mut j = i + 1;
                let mut bd = 1i32;
                while j < self.tokens.len() && bd > 0 {
                    match self.tokens[j].kind {
                        TokenKind::OpenBrace => bd += 1,
                        TokenKind::CloseBrace => bd -= 1,
                        _ => {}
                    }
                    j += 1;
                }
                // j is now past the matching `}`.  Check what follows.
                if let Some(after_brace) = self.tokens.get(j) {
                    // `{...} as T)` — type assertion, not arrow
                    if after_brace.kind == TokenKind::As {
                        return false;
                    }
                    // `{...})` immediately closed — could be obj literal in parens
                    // Check if `) =>` follows → arrow; `) :` → ternary
                    if after_brace.kind == TokenKind::CloseParen {
                        if let Some(after_close) = self.tokens.get(j + 1) {
                            if after_close.kind != TokenKind::FatArrow {
                                return false;
                            }
                        }
                    }
                }
            }
        }

        let mut depth = 1;
        let mut brace_depth = 0i32;
        let mut bracket_depth = 0i32;
        let mut angle_depth = 0i32;
        // Top-level operators are expression-only while scanning a plain
        // parameter name, but become valid once we are inside that
        // parameter's type annotation or initializer.  In particular, do not
        // reject arrows such as `(x: A | B) => x` or
        // `(x: ns.T, y = a || b) => y` before the real parameter parser gets
        // a chance to consume them.
        let mut in_param_type_or_initializer = false;
        while i < self.tokens.len() && depth > 0 {
            let kind = self.tokens[i].kind;
            if depth == 1 && brace_depth == 0 && bracket_depth == 0 {
                match kind {
                    TokenKind::Colon | TokenKind::Equals => {
                        in_param_type_or_initializer = true;
                    }
                    TokenKind::Comma if angle_depth == 0 => {
                        in_param_type_or_initializer = false;
                    }
                    _ => {}
                }
                match kind {
                    TokenKind::Dot
                    | TokenKind::QuestionDot
                    | TokenKind::EqualsEqualsEquals
                    | TokenKind::ExclEqualsEquals
                    | TokenKind::EqualsEquals
                    | TokenKind::ExclEquals
                    | TokenKind::BarBar
                    | TokenKind::AmpersandAmpersand
                    | TokenKind::InstanceOf
                    | TokenKind::In
                    | TokenKind::As
                    | TokenKind::QuestionQuestion
                    | TokenKind::GreaterEqual
                    | TokenKind::LessEqual
                    | TokenKind::Plus
                    | TokenKind::Minus
                    | TokenKind::Asterisk
                    | TokenKind::Slash
                    | TokenKind::Percent
                    | TokenKind::Caret
                    | TokenKind::Ampersand
                    | TokenKind::Bar
                    | TokenKind::AsteriskAsterisk
                    | TokenKind::PlusPlus
                    | TokenKind::MinusMinus
                    | TokenKind::Excl
                    | TokenKind::Tilde
                        if !in_param_type_or_initializer =>
                    {
                        return false;
                    }
                    TokenKind::Question => {
                        if let Some(next) = self.tokens.get(i + 1) {
                            if matches!(
                                next.kind,
                                TokenKind::StringLiteral
                                    | TokenKind::NumericLiteral
                                    | TokenKind::True
                                    | TokenKind::False
                                    | TokenKind::Null
                                    | TokenKind::OpenBrace
                                    | TokenKind::OpenBracket
                                    | TokenKind::OpenParen
                                    | TokenKind::LessThan
                                    | TokenKind::Excl
                            ) {
                                return false;
                            }
                        }
                    }
                    _ => {}
                }
            }
            match kind {
                TokenKind::OpenParen => depth += 1,
                TokenKind::CloseParen => depth -= 1,
                TokenKind::OpenBrace => brace_depth += 1,
                TokenKind::CloseBrace => brace_depth -= 1,
                TokenKind::OpenBracket => bracket_depth += 1,
                TokenKind::CloseBracket => bracket_depth -= 1,
                TokenKind::LessThan if in_param_type_or_initializer => angle_depth += 1,
                TokenKind::GreaterThan | TokenKind::GreaterEqual if angle_depth > 0 => {
                    angle_depth -= 1;
                }
                TokenKind::GreaterGreater | TokenKind::GreaterGreaterEquals if angle_depth > 0 => {
                    angle_depth = (angle_depth - 2).max(0);
                }
                TokenKind::GreaterGreaterGreater | TokenKind::GreaterGreaterGreaterEquals
                    if angle_depth > 0 =>
                {
                    angle_depth = (angle_depth - 3).max(0);
                }
                _ => {}
            }
            i += 1;
        }

        // In ternary consequent, check if the paren content contains
        // expression-only operators at the top level (not inside nested parens).
        // Arrow params can't contain `.`, `?.`, `===`, `||`, `&&`, `instanceof`, `as`, `[`.
        if self.in_ternary_consequent {
            let inner = &self.tokens[params_start + 1..i.saturating_sub(1)];
            // `(fn(...))` — function call in parens, not arrow params
            if inner.len() >= 2
                && inner[0].kind == TokenKind::Identifier
                && inner[1].kind == TokenKind::OpenParen
            {
                return false;
            }
            let mut pd = 0i32;
            let mut bd = 0i32;
            let mut bk = 0i32;
            for (idx, tok) in inner.iter().enumerate() {
                match tok.kind {
                    TokenKind::OpenParen => pd += 1,
                    TokenKind::CloseParen => pd -= 1,
                    TokenKind::OpenBrace => bd += 1,
                    TokenKind::CloseBrace => bd -= 1,
                    TokenKind::OpenBracket => bk += 1,
                    TokenKind::CloseBracket => bk -= 1,
                    // Expression operators at top level → not arrow params
                    TokenKind::Dot
                    | TokenKind::QuestionDot
                    | TokenKind::EqualsEqualsEquals
                    | TokenKind::ExclEqualsEquals
                    | TokenKind::EqualsEquals
                    | TokenKind::ExclEquals
                    | TokenKind::BarBar
                    | TokenKind::AmpersandAmpersand
                    | TokenKind::InstanceOf
                    | TokenKind::In
                    | TokenKind::As
                    | TokenKind::QuestionQuestion
                    | TokenKind::GreaterEqual
                    | TokenKind::LessEqual
                        if pd == 0 && bd == 0 && bk == 0 =>
                    {
                        return false;
                    }
                    // `? literal` at top level → ternary, not optional param
                    TokenKind::Question if pd == 0 && bd == 0 && bk == 0 => {
                        if let Some(next) = inner.get(idx + 1) {
                            if matches!(
                                next.kind,
                                TokenKind::StringLiteral
                                    | TokenKind::NumericLiteral
                                    | TokenKind::True
                                    | TokenKind::False
                                    | TokenKind::Null
                                    | TokenKind::OpenBrace
                                    | TokenKind::OpenBracket
                                    | TokenKind::OpenParen
                                    | TokenKind::LessThan
                                    | TokenKind::Excl
                            ) {
                                return false;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // After the matching `)`, look for `=>` or `: type =>`
        if i < self.tokens.len() {
            match self.tokens[i].kind {
                TokenKind::FatArrow => return true,
                TokenKind::OpenBrace => return true,
                TokenKind::Dot
                    if Self::paren_list_has_ts_only_syntax(
                        &self.tokens[params_start + 1..i - 1],
                    ) =>
                {
                    return true;
                }
                // Allow `:` followed by type tokens before `=>`
                TokenKind::Colon => {
                    // Disambiguate ternary `:` from return type `:`.
                    // When inside a ternary consequent, `: ident => body` is
                    // likely a ternary colon + arrow alternate, not a return
                    // type annotation.  Only applies when the params have no
                    // TypeScript-only syntax (pure JS expressions).
                    if self.in_ternary_consequent
                        && !Self::paren_list_has_ts_only_syntax(
                            &self.tokens[params_start + 1..i.saturating_sub(1)],
                        )
                    {
                        // Check if `: identifier =>` immediately follows
                        if let Some(id_tok) = self.tokens.get(i + 1) {
                            if matches!(id_tok.kind, TokenKind::Identifier | TokenKind::OpenParen) {
                                if let Some(after) = self.tokens.get(i + 2) {
                                    if after.kind != TokenKind::FatArrow
                                        && id_tok.kind == TokenKind::OpenParen
                                    {
                                        // `: ( <not-arrow>` — this is the ternary
                                        // colon followed by a paren expression,
                                        // not a return type annotation.
                                        return false;
                                    }
                                    if after.kind == TokenKind::FatArrow {
                                        // `: Identifier =>` in ternary consequent.
                                        // If the identifier is PascalCase (type name convention),
                                        // treat as arrow return type annotation — don't check for
                                        // a later colon (the forward scan fails on deeply nested
                                        // arrow bodies with object literals).
                                        let is_pascal_return_type =
                                            id_tok.kind == TokenKind::Identifier && {
                                                let id_text = &self.source[id_tok.span.start
                                                    as usize
                                                    ..id_tok.span.end as usize];
                                                id_text
                                                    .starts_with(|c: char| c.is_ascii_uppercase())
                                            };
                                        let mut has_later_colon = is_pascal_return_type;
                                        if !is_pascal_return_type {
                                            // For camelCase identifiers, use forward scan
                                            // to distinguish ternary colon from return type.
                                            let mut j = i + 3;
                                            let mut pd = 0i32;
                                            let mut bd = 0i32;
                                            while j < self.tokens.len() {
                                                match self.tokens[j].kind {
                                                    TokenKind::OpenParen => pd += 1,
                                                    TokenKind::CloseParen => {
                                                        pd -= 1;
                                                        if pd < 0 {
                                                            break;
                                                        }
                                                    }
                                                    TokenKind::OpenBrace => bd += 1,
                                                    TokenKind::CloseBrace => {
                                                        bd -= 1;
                                                        if bd < 0 {
                                                            break;
                                                        }
                                                    }
                                                    TokenKind::Colon if pd == 0 && bd == 0 => {
                                                        has_later_colon = true;
                                                        break;
                                                    }
                                                    TokenKind::Semicolon => break,
                                                    _ => {}
                                                }
                                                j += 1;
                                            }
                                        }
                                        if !has_later_colon {
                                            return false;
                                        }
                                        // has_later_colon: this `:` is return type,
                                        // continue scanning as arrow.
                                    }
                                }
                            }
                        }
                    }
                    // Skip the type annotation — just scan until `=>`
                    i += 1;
                    let mut brace_depth = 0;
                    let mut paren_depth = 0;
                    let mut bracket_depth = 0;
                    while i < self.tokens.len() {
                        match self.tokens[i].kind {
                            TokenKind::FatArrow
                                if brace_depth == 0 && paren_depth == 0 && bracket_depth == 0 =>
                            {
                                return true;
                            }
                            TokenKind::OpenBrace
                                if brace_depth == 0 && paren_depth == 0 && bracket_depth == 0 =>
                            {
                                return true;
                            }
                            TokenKind::Semicolon
                            | TokenKind::Comma
                            | TokenKind::CloseBrace
                            | TokenKind::CloseBracket
                                if brace_depth == 0 && paren_depth == 0 && bracket_depth == 0 =>
                            {
                                return !Self::paren_list_contains_fat_arrow(
                                    &self.tokens[params_start + 1..i - 1],
                                );
                            }
                            // CloseParen at depth 0 means we've reached the end
                            // of an enclosing group (e.g., ternary consequent in
                            // `(opt ? (x) : (y))`) without finding `=>`. This is
                            // NOT an arrow function.
                            TokenKind::CloseParen
                                if brace_depth == 0 && paren_depth == 0 && bracket_depth == 0 =>
                            {
                                return false;
                            }
                            TokenKind::OpenParen => paren_depth += 1,
                            TokenKind::CloseParen => {
                                if paren_depth == 0 {
                                    return false;
                                }
                                paren_depth -= 1;
                            }
                            TokenKind::OpenBrace => brace_depth += 1,
                            TokenKind::CloseBrace => {
                                if brace_depth == 0 {
                                    return false;
                                }
                                brace_depth -= 1;
                            }
                            TokenKind::OpenBracket => bracket_depth += 1,
                            TokenKind::CloseBracket => {
                                if bracket_depth == 0 {
                                    return false;
                                }
                                bracket_depth -= 1;
                            }
                            // If we hit a statement terminator at top level, it's not an arrow.
                            // Semicolons inside braces are allowed (object type literals).
                            TokenKind::Semicolon if brace_depth == 0 => return false,
                            _ => {}
                        }
                        i += 1;
                    }
                    return false;
                }
                TokenKind::Semicolon
                | TokenKind::Comma
                | TokenKind::CloseBrace
                | TokenKind::CloseParen
                | TokenKind::CloseBracket
                    if Self::paren_list_has_ts_only_syntax(
                        &self.tokens[params_start + 1..i - 1],
                    ) =>
                {
                    return true;
                }
                // Anything else (like a `.`, `[`, `(`) means this isn't arrow params
                _ => return false,
            }
        }
        false
    }

    fn params_have_ts_only_syntax(params: &[Param]) -> bool {
        params.iter().any(|param| {
            param.type_ann.is_some()
                || param.optional
                || param.modifiers != MOD_NONE
                || !param.decorators.is_empty()
                || matches!(&param.name.kind, PatKind::Ident(name) if name == "this")
        })
    }

    fn paren_list_has_ts_only_syntax(tokens: &[Token]) -> bool {
        let mut paren_depth = 0i32;
        let mut brace_depth = 0i32;
        let mut bracket_depth = 0i32;
        let mut conditional_depth = 0i32;
        for (index, token) in tokens.iter().enumerate() {
            match token.kind {
                TokenKind::OpenParen => paren_depth += 1,
                TokenKind::CloseParen => paren_depth -= 1,
                TokenKind::OpenBrace => brace_depth += 1,
                TokenKind::CloseBrace => brace_depth -= 1,
                TokenKind::OpenBracket => bracket_depth += 1,
                TokenKind::CloseBracket => bracket_depth -= 1,
                TokenKind::Question
                    if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 =>
                {
                    let next = tokens.get(index + 1).map(|token| token.kind);
                    if matches!(
                        next,
                        Some(TokenKind::Colon)
                            | Some(TokenKind::Comma)
                            | Some(TokenKind::CloseParen)
                            | Some(TokenKind::Equals)
                    ) {
                        return true;
                    }
                    conditional_depth += 1;
                }
                TokenKind::Colon if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 => {
                    if conditional_depth > 0 {
                        conditional_depth -= 1;
                        continue;
                    }
                    return true;
                }
                _ => {}
            }
        }
        false
    }

    fn paren_list_contains_fat_arrow(tokens: &[Token]) -> bool {
        tokens.iter().any(|token| token.kind == TokenKind::FatArrow)
    }

    fn parse_arrow_body(&mut self) -> ArrowBody {
        if self.at(TokenKind::OpenBrace) {
            return ArrowBody::Block(self.parse_block_body());
        }

        // Recovery: `() => var x = 1; return x; }` should be treated like
        // `() => { var x = 1; return x; }` so the trailing `}` does not
        // close an outer block.  Parse multiple statements until `}` or EOF.
        if self.is_arrow_statement_recovery_start() {
            // tsc: an arrow body starting with a statement keyword gets
            // "'{' expected" at that keyword.
            if self.speculation_depth == 0 {
                self.error_code(1005, "'{' expected.".into());
            }
            let mut stmts = Vec::new();
            while !self.at(TokenKind::CloseBrace) && !self.is_eof() {
                let before = self.pos;
                stmts.push(self.parse_statement(self.current_token_starts_statement()));
                if self.pos == before && !self.is_eof() {
                    self.bump(); // skip stuck token to avoid infinite loop
                }
            }
            if self.at(TokenKind::CloseBrace) {
                self.bump();
            }
            return ArrowBody::Block(stmts);
        }

        // Recovery: missing body (e.g. `() => }`). Leave the delimiter in
        // place so outer constructs can consume it. tsc reports TS1109
        // "Expression expected" at the delimiter.
        if self.is_arrow_body_terminator() {
            if self.speculation_depth == 0 {
                self.error_code(1109, "Expression expected.".into());
            }
            return ArrowBody::Expr(Box::new(Expr {
                kind: ExprKind::Omitted,
                span: self.cur_span(),
            }));
        }

        ArrowBody::Expr(Box::new(self.parse_assignment_expr()))
    }

    fn is_arrow_statement_recovery_start(&self) -> bool {
        matches!(
            self.cur(),
            TokenKind::Var | TokenKind::Let | TokenKind::Const
        )
    }

    fn is_arrow_body_terminator(&self) -> bool {
        self.is_eof()
            || matches!(
                self.cur(),
                TokenKind::CloseBrace
                    | TokenKind::Semicolon
                    | TokenKind::Comma
                    | TokenKind::CloseParen
                    | TokenKind::CloseBracket
            )
    }

    // -----------------------------------------------------------------------
    // Patterns
    // -----------------------------------------------------------------------

    fn parse_binding_pattern(&mut self) -> Pat {
        self.parse_binding_pattern_at(self.cur_span().start, false)
    }

    fn parse_binding_pattern_at(&mut self, full_start: u32, track_full_start: bool) -> Pat {
        let start = self.cur_span().start;
        match self.cur() {
            TokenKind::OpenBrace => self.parse_object_pattern(start, track_full_start),
            TokenKind::OpenBracket => self.parse_array_pattern(start, track_full_start),
            // Error recovery: `const #foo = 3;` — preserve `#` prefix in binding name
            TokenKind::Hash => {
                self.bump();
                let (name, _): (AstString, _) = if let Some((n, s)) = self.parse_identifier_name() {
                    (n, s)
                } else {
                    ("<error>".into(), self.cur_span())
                };
                let pat = Pat {
                    kind: PatKind::Ident(format!("#{name}").into()),
                    span: self.span_from(start),
                };
                self.record_binding_name_full_start(pat.span, full_start, track_full_start);
                pat
            }
            // `this` keyword as parameter name (TypeScript this-parameter)
            TokenKind::This => {
                let span = self.cur_span();
                self.bump();
                let pat = Pat {
                    kind: PatKind::Ident("this".into()),
                    span,
                };
                self.record_binding_name_full_start(span, full_start, track_full_start);
                pat
            }
            _ => {
                let (name, span) = self.parse_identifier();
                let pat = Pat {
                    kind: PatKind::Ident(name.into()),
                    span,
                };
                self.record_binding_name_full_start(span, full_start, track_full_start);
                pat
            }
        }
    }

    fn record_binding_name_full_start(
        &mut self,
        name_span: Span,
        full_start: u32,
        track_full_start: bool,
    ) {
        if track_full_start {
            self.scratch_binding_name_full_starts
                .push(BindingNameFullStart {
                    name_span,
                    full_start,
                });
        }
    }

    fn parse_object_pattern(&mut self, start: u32, track_full_start: bool) -> Pat {
        let open_brace = self.bump();
        let mut element_full_start = open_brace.end;
        // Inside braces, `in` is allowed as a binary operator even in
        // for-loop init context (e.g., `for (let { x = 'a' in {} } in ...)`).
        let saved_disallow_in = self.disallow_in;
        self.disallow_in = false;
        let mut props = Vec::with_capacity(8);
        while !self.at(TokenKind::CloseBrace) && !self.is_eof() {
            let binding_full_start = element_full_start;
            if self.at(TokenKind::DotDotDot) {
                self.bump();
                let rest_binding_mark = self.scratch_binding_name_full_starts.len();
                let mut pat = self.parse_binding_pattern_at(binding_full_start, track_full_start);
                // Keep an invalid rest suffix inside the object pattern so the
                // enclosing declaration and initializer remain one statement.
                // The JS recovery emitter preserves the original suffix text.
                if self.eat(TokenKind::Colon).is_some() {
                    self.scratch_binding_name_full_starts
                        .truncate(rest_binding_mark);
                    pat = self.parse_binding_pattern_at(binding_full_start, track_full_start);
                }
                if self.eat(TokenKind::Equals).is_some() {
                    self.parse_assignment_expr();
                }
                props.push(ObjPatProp::Rest(pat));
                // Continue parsing for error recovery (rest not last).
                // TypeScript keeps parsing additional properties after rest.
                let Some(comma_span) = self.eat(TokenKind::Comma) else {
                    break;
                };
                element_full_start = comma_span.end;
                continue;
            }
            let name = if let Some(n) = self.parse_property_name() {
                n
            } else {
                self.error_code(1003, "Identifier expected.".into());
                PropName::Ident("<error>".into(), self.cur_span())
            };
            if self.eat(TokenKind::Colon).is_some() {
                let pat = self.parse_binding_pattern_at(binding_full_start, track_full_start);
                // Check for default value: { a: b = 1 }
                let pat = if self.eat(TokenKind::Equals).is_some() {
                    let init = Box::new(self.parse_assignment_expr());
                    Pat {
                        span: pat.span,
                        kind: PatKind::Assign(Box::new(pat), init),
                    }
                } else {
                    pat
                };
                props.push(ObjPatProp::KeyValue(name, pat));
            } else if self.eat(TokenKind::Equals).is_some() {
                if let PropName::Ident(ref n, s) = name {
                    self.record_binding_name_full_start(s, binding_full_start, track_full_start);
                    let init = Box::new(self.parse_assignment_expr());
                    props.push(ObjPatProp::ShorthandAssign(n.clone(), init, s));
                }
            } else if let PropName::Ident(ref n, s) = name {
                self.record_binding_name_full_start(s, binding_full_start, track_full_start);
                props.push(ObjPatProp::Shorthand(n.clone(), s));
            } else {
                // Error recovery: string/numeric/computed key without `:`.
                // Treat as shorthand with the key text as the name (e.g.
                // `{ "while" }` → `Shorthand("\"while\"", ...)`).
                let (key_text, key_span) = match &name {
                    PropName::String(s, span) => (format!("\"{}\"", s), *span),
                    PropName::Number(n, span) => (n.to_string(), *span),
                    PropName::Computed(_, span) => ("<computed>".to_string(), *span),
                    PropName::Private(n, span) => (format!("#{}", n), *span),
                    PropName::Ident(n, span) => (n.to_string(), *span),
                };
                self.record_binding_name_full_start(key_span, binding_full_start, track_full_start);
                props.push(ObjPatProp::Shorthand(key_text.into(), key_span));
            }
            // Skip TypeScript's optional property `?` marker in bindings
            // (e.g. `var {h?} = ...`). The `?` is not valid in JS but
            // TypeScript error-recovers by stripping it.
            self.eat(TokenKind::Question);
            let Some(comma_span) = self.eat(TokenKind::Comma) else {
                break;
            };
            element_full_start = comma_span.end;
        }
        self.disallow_in = saved_disallow_in;
        self.expect(TokenKind::CloseBrace);
        Pat {
            kind: PatKind::Object(props),
            span: self.span_from(start),
        }
    }

    fn parse_array_pattern(&mut self, start: u32, track_full_start: bool) -> Pat {
        let open_bracket = self.bump();
        let mut element_full_start = open_bracket.end;
        // Inside brackets, `in` is allowed as a binary operator even in
        // for-loop init context (e.g., `for (let [x = 'a' in {}] in ...)`).
        let saved_disallow_in = self.disallow_in;
        self.disallow_in = false;
        let mut elements = Vec::with_capacity(4);
        while !self.at(TokenKind::CloseBracket) && !self.is_eof() {
            if self.at(TokenKind::Comma) {
                elements.push(None);
                element_full_start = self.bump().end;
                continue;
            }
            let binding_full_start = element_full_start;
            if self.at(TokenKind::DotDotDot) {
                self.bump();
                let pat = self.parse_binding_pattern_at(binding_full_start, track_full_start);
                // Check for initializer on rest element (error case, but preserve it)
                let pat = if self.eat(TokenKind::Equals).is_some() {
                    let init = Box::new(self.parse_assignment_expr());
                    Pat {
                        span: pat.span,
                        kind: PatKind::Assign(Box::new(pat), init),
                    }
                } else {
                    pat
                };
                elements.push(Some(ArrayPatElem::Rest(pat)));
                // Continue parsing for error recovery (rest not last).
                // TypeScript keeps parsing additional elements after rest.
                let Some(comma_span) = self.eat(TokenKind::Comma) else {
                    break;
                };
                element_full_start = comma_span.end;
                continue;
            }
            let pat = self.parse_binding_pattern_at(binding_full_start, track_full_start);
            // Check for default value
            let pat = if self.eat(TokenKind::Equals).is_some() {
                let init = Box::new(self.parse_assignment_expr());
                Pat {
                    span: pat.span,
                    kind: PatKind::Assign(Box::new(pat), init),
                }
            } else {
                pat
            };
            elements.push(Some(ArrayPatElem::Pat(pat)));
            let Some(comma_span) = self.eat(TokenKind::Comma) else {
                break;
            };
            element_full_start = comma_span.end;
        }
        self.disallow_in = saved_disallow_in;
        self.expect(TokenKind::CloseBracket);
        Pat {
            kind: PatKind::Array(elements),
            span: self.span_from(start),
        }
    }

    fn parse_property_name(&mut self) -> Option<PropName> {
        let start = self.cur_span().start;
        match self.cur() {
            TokenKind::StringLiteral => {
                let span = self.bump_string_literal();
                let raw = self.text(span);
                let content = if raw.len() >= 2 {
                    &raw[1..raw.len() - 1]
                } else {
                    ""
                };
                Some(PropName::String(content.into(), span))
            }
            TokenKind::NumericLiteral | TokenKind::BigIntLiteral => {
                let span = self.bump();
                Some(PropName::Number(self.text(span).into(), span))
            }
            TokenKind::OpenBracket => {
                self.bump();
                // Use full expression (including comma operator) to handle
                // error recovery for `[0, 1]: {}` in object/class members.
                let expr = self.parse_expression();
                self.expect(TokenKind::CloseBracket);
                Some(PropName::Computed(Box::new(expr), self.span_from(start)))
            }
            TokenKind::Hash => {
                let hash_span = self.bump();
                if self.private_name_can_consume_identifier(hash_span.end) {
                    if let Some((name, _span)) = self.parse_identifier_name() {
                        Some(PropName::Private(name.into(), self.span_from(start)))
                    } else {
                        self.error_code(1003, "Identifier expected.".into());
                        Some(PropName::Private("".into(), self.span_from(start)))
                    }
                } else {
                    self.error_code(1003, "Identifier expected.".into());
                    Some(PropName::Private("".into(), self.span_from(start)))
                }
            }
            _ => {
                if let Some((name, span)) = self.parse_identifier_name() {
                    Some(PropName::Ident(name.into(), span))
                } else {
                    None
                }
            }
        }
    }

    fn private_name_can_consume_identifier(&self, hash_end: u32) -> bool {
        let start = hash_end as usize;
        let end = self.cur_span().start as usize;
        if start > end || end > self.source.len() {
            return true;
        }
        let between = &self.source[start..end];
        !between.contains('\n') && !between.contains('\r')
    }

    // -----------------------------------------------------------------------
    // Decorators
    // -----------------------------------------------------------------------

    fn parse_decorators(&mut self) -> Vec<Expr> {
        let mut decorators = Vec::new();
        while self.at(TokenKind::At) {
            decorators.push(self.parse_decorator());
        }
        decorators
    }

    fn parse_decorator(&mut self) -> Expr {
        let start = self.cur_span().start;
        self.bump(); // @
        let starts_with_identifier = self.is_identifier();
        let mut expr = self.parse_left_hand_side_expr();
        if starts_with_identifier {
            if let Some(prefix) = Self::decorator_prefix_before_element_access(&expr) {
                let prefix_follows_at_sign = self
                    .source
                    .get(start.saturating_add(1) as usize..prefix.span.start as usize)
                    .is_some_and(|between| between.trim().is_empty());
                if prefix_follows_at_sign {
                    if let Some(position) = self.tokens[..self.pos].iter().rposition(|token| {
                        token.kind == TokenKind::OpenBracket && token.span.start >= prefix.span.end
                    }) {
                        self.pos = position;
                        expr = prefix;
                    }
                }
            }
        }
        Self::normalize_invalid_await_decorator(Expr {
            span: self.span_from(start),
            kind: expr.kind,
        })
    }

    fn decorator_prefix_before_element_access(expression: &Expr) -> Option<Expr> {
        match &expression.kind {
            ExprKind::ElemAccess(access) if !access.optional => {
                Self::decorator_prefix_before_element_access(&access.object)
                    .or_else(|| Some((*access.object).clone()))
            }
            ExprKind::Member(member) => {
                Self::decorator_prefix_before_element_access(&member.object)
            }
            ExprKind::Call(call) => Self::decorator_prefix_before_element_access(&call.callee),
            _ => None,
        }
    }

    /// Normalize `await` when it occurs where a decorator expression is
    /// required. In an external-module await context TypeScript recovers
    /// `@await(x)` as decorator `(x)`, bare `@await` as a missing decorator,
    /// and `@(await)` as a parenthesized missing-operand await expression.
    fn normalize_invalid_await_decorator(mut decorator: Expr) -> Expr {
        match decorator.kind {
            ExprKind::Ident(ref name) if name == "await" => {
                decorator.kind = ExprKind::Omitted;
                decorator
            }
            ExprKind::Await(ref argument) if !matches!(argument.kind, ExprKind::Omitted) => {
                decorator.span = argument.span;
                decorator.kind = ExprKind::Paren(argument.clone());
                decorator
            }
            ExprKind::Call(call)
                if matches!(&call.callee.kind, ExprKind::Ident(name) if name == "await")
                    || matches!(&call.callee.kind, ExprKind::Await(argument) if matches!(argument.kind, ExprKind::Omitted)) =>
            {
                let inner = if call.args.len() == 1 {
                    (*call.args[0]).clone()
                } else {
                    Expr {
                        kind: ExprKind::Comma(call.args.clone()),
                        span: decorator.span,
                    }
                };
                decorator.span = inner.span;
                decorator.kind = ExprKind::Paren(Box::new(inner));
                decorator
            }
            ExprKind::Paren(mut inner)
                if matches!(&inner.kind, ExprKind::Ident(name) if name == "await")
                    || matches!(&inner.kind, ExprKind::Await(arg) if matches!(arg.kind, ExprKind::Omitted)) =>
            {
                // The trailing blank is observable in TypeScript's malformed
                // decorator emit: `@(await)` becomes `(await )`.
                decorator.span = inner.span;
                inner.span = Span::new(0, 0);
                inner.kind = ExprKind::Ident("await ".into());
                decorator.kind = ExprKind::Paren(inner);
                decorator
            }
            _ => decorator,
        }
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    pub(crate) fn try_parse_js_checked_type_params(&mut self) -> Option<Vec<TypeParam>> {
        let first_param_span = if self.at(TokenKind::LessThan) {
            self.tokens.get(self.pos + 1).map(|token| token.span)
        } else {
            None
        };
        let type_params = self.try_parse_type_params();
        if self.is_js_file && type_params.is_some() {
            if let Some(span) = first_param_span {
                self.error_at_span(
                    8004,
                    "Type parameter declarations can only be used in TypeScript files.".to_string(),
                    span,
                );
            }
        }
        type_params
    }

    fn parse_js_checked_type_annotation(&mut self) -> TypeNode {
        let type_node = self.parse_type();
        if self.is_js_file {
            self.error_at_span(
                8010,
                "Type annotations can only be used in TypeScript files.".to_string(),
                type_node.span,
            );
        }
        type_node
    }

    pub(crate) fn parse_js_checked_return_type_annotation(&mut self) -> TypeNode {
        let type_node = self.parse_return_type();
        if self.is_js_file {
            self.error_at_span(
                8010,
                "Type annotations can only be used in TypeScript files.".to_string(),
                type_node.span,
            );
        }
        type_node
    }

    fn try_parse_js_checked_type_args(&mut self) -> Option<Vec<TypeNode>> {
        let type_args = self.try_parse_type_args();
        if self.is_js_file {
            if let Some(span) = type_args
                .as_ref()
                .and_then(|arguments| arguments.first())
                .map(|argument| argument.span)
            {
                self.error_at_span(
                    8011,
                    "Type arguments can only be used in TypeScript files.".to_string(),
                    span,
                );
            }
        }
        type_args
    }

    fn parse_modifiers(&mut self) -> ModifierFlags {
        let mut flags = MOD_NONE;
        loop {
            match self.cur() {
                // Export is handled by parse_statement's Export arm, not as a modifier
                TokenKind::Declare => {
                    // `declare` is only a modifier when followed by a
                    // declaration keyword.  When followed by something else
                    // (e.g. `declare instanceof C`) it is an identifier
                    // expression and must not be consumed here.
                    if self.peek_can_follow_declare() {
                        self.bump();
                        flags |= MOD_DECLARE;
                    } else {
                        break;
                    }
                }
                TokenKind::Abstract => {
                    if !self.peek_is_on_new_line()
                        && (self.peek_is(TokenKind::Class) || self.peek_is(TokenKind::Interface))
                    {
                        self.bump();
                        flags |= MOD_ABSTRACT;
                    } else {
                        break;
                    }
                }
                TokenKind::Async => {
                    if self.peek_is(TokenKind::Function) {
                        // Don't consume here; let parse_function_decl handle it
                        break;
                    }
                    // Consume `async` before type-only declarations so it doesn't
                    // leak as a standalone expression statement (e.g. `async interface`).
                    // tsc parses it as a modifier and reports TS1042.
                    if self.peek_is(TokenKind::Interface)
                        || self.peek_is(TokenKind::Namespace)
                        || self.peek_is(TokenKind::Module)
                    {
                        let span = self.bump();
                        self.grammar_error_at_span(
                            1042,
                            "'async' modifier cannot be used here.".into(),
                            span,
                        );
                        flags |= MOD_ASYNC;
                        continue;
                    }
                    if (self.peek_is(TokenKind::Class) || self.peek_is(TokenKind::Enum))
                        && !self.peek_is_on_new_line()
                    {
                        let span = self.bump();
                        self.grammar_error_at_span(
                            1042,
                            "'async' modifier cannot be used here.".into(),
                            span,
                        );
                        continue;
                    }
                    break;
                }
                _ => break,
            }
        }
        flags
    }

    fn parse_member_modifiers(&mut self) -> ModifierFlags {
        let mut flags = MOD_NONE;
        let mut has_seen_static = false;
        loop {
            match self.cur() {
                TokenKind::Public | TokenKind::Private | TokenKind::Protected => {
                    if !self.peek_same_line_can_follow_modifier() {
                        break;
                    }
                    let flag = match self.cur() {
                        TokenKind::Public => MOD_PUBLIC,
                        TokenKind::Private => MOD_PRIVATE,
                        _ => MOD_PROTECTED,
                    };
                    self.bump();
                    flags |= flag;
                }
                TokenKind::Static => {
                    // A second static is a member name, including error recovery.
                    if has_seen_static {
                        break;
                    }
                    if !self.peek_can_follow_modifier() {
                        break;
                    }
                    self.bump();
                    flags |= MOD_STATIC;
                    has_seen_static = true;
                }
                TokenKind::Readonly => {
                    if !self.peek_could_be_member() {
                        break;
                    }
                    self.bump();
                    flags |= MOD_READONLY;
                }
                TokenKind::Abstract => {
                    // In class bodies, `abstract` on its own line followed by a member
                    // on the next line is treated as a property name via ASI, not a modifier.
                    if !self.peek_same_line_can_follow_modifier() {
                        break;
                    }
                    self.bump();
                    flags |= MOD_ABSTRACT;
                }
                TokenKind::Override => {
                    if !self.peek_same_line_can_follow_modifier() {
                        break;
                    }
                    self.bump();
                    flags |= MOD_OVERRIDE;
                }
                TokenKind::Async => {
                    if !self.peek_same_line_can_follow_modifier() {
                        break;
                    }
                    self.bump();
                    flags |= MOD_ASYNC;
                }
                TokenKind::Accessor => {
                    if !self.peek_same_line_can_follow_modifier() {
                        break;
                    }
                    self.bump();
                    flags |= MOD_ACCESSOR;
                }
                TokenKind::Declare => {
                    if !self.peek_same_line_can_follow_modifier() {
                        break;
                    }
                    self.bump();
                    flags |= MOD_DECLARE;
                }
                // `const` and `export` are not valid class member modifiers, but TypeScript
                // error-recovers by skipping them (e.g. `static const H = 1;`,
                // `constructor(export a: number)`).
                TokenKind::Const | TokenKind::Export => {
                    if !self.peek_same_line_can_follow_modifier() {
                        break;
                    }
                    self.bump();
                }
                _ => break,
            }
        }
        flags
    }

    fn parse_assign_op(&self) -> AssignOp {
        match self.cur() {
            TokenKind::Equals => AssignOp::Assign,
            TokenKind::PlusEquals => AssignOp::AddAssign,
            TokenKind::MinusEquals => AssignOp::SubAssign,
            TokenKind::AsteriskEquals => AssignOp::MulAssign,
            TokenKind::SlashEquals => AssignOp::DivAssign,
            TokenKind::PercentEquals => AssignOp::ModAssign,
            TokenKind::AsteriskAsteriskEquals => AssignOp::ExpAssign,
            TokenKind::AmpersandEquals => AssignOp::BitAndAssign,
            TokenKind::BarEquals => AssignOp::BitOrAssign,
            TokenKind::CaretEquals => AssignOp::BitXorAssign,
            TokenKind::LessLessEquals => AssignOp::ShlAssign,
            TokenKind::GreaterGreaterEquals => AssignOp::ShrAssign,
            TokenKind::GreaterGreaterGreaterEquals => AssignOp::UShrAssign,
            TokenKind::AmpersandAmpersandEquals => AssignOp::LogAndAssign,
            TokenKind::BarBarEquals => AssignOp::LogOrAssign,
            TokenKind::QuestionQuestionEquals => AssignOp::NullCoalAssign,
            _ => AssignOp::Assign,
        }
    }

    fn parse_string_literal(&mut self) -> String {
        if self.at(TokenKind::StringLiteral) {
            let span = self.bump_string_literal();
            let raw = self.text(span);
            Self::string_literal_content(raw).to_string()
        } else {
            self.error_code(1141, "String literal expected.".into());
            String::new()
        }
    }

    /// Parse a string literal, returning both the content and the span of the token.
    fn parse_string_literal_with_span(&mut self) -> (String, Span) {
        if self.at(TokenKind::StringLiteral) {
            let span = self.bump_string_literal();
            let raw = self.text(span);
            (Self::string_literal_content(raw).to_string(), span)
        } else {
            self.error_code(1141, "String literal expected.".into());
            (String::new(), self.cur_span())
        }
    }

    fn parse_optional_label(&mut self) -> Option<String> {
        if self.is_identifier() && !self.is_on_new_line() {
            let (name, _) = self.parse_identifier();
            Some(name.into())
        } else {
            None
        }
    }

    fn eat_semicolon(&mut self) {
        if self.at(TokenKind::Semicolon) {
            self.bump();
        }
        // ASI: allow missing semicolons at end of line, before }, or at EOF
    }

    /// Skip `with { ... }` or `assert { ... }` import attributes/assertions.
    fn skip_import_attributes(&mut self) -> bool {
        let mut has_resolution_mode = false;
        if self.at(TokenKind::With) || self.at(TokenKind::Assert) {
            self.bump(); // consume `with` / `assert`
            if self.at(TokenKind::OpenBrace) {
                self.bump(); // consume `{`
                let mut depth = 1u32;
                let mut template_tags = Vec::new();
                while depth > 0 && !self.is_eof() {
                    match self.cur() {
                        TokenKind::OpenBrace => {
                            depth += 1;
                            self.bump();
                        }
                        TokenKind::CloseBrace => {
                            depth -= 1;
                            self.bump();
                        }
                        _ => {
                            if matches!(
                                self.cur(),
                                TokenKind::StringLiteral | TokenKind::Identifier
                            ) && Self::string_literal_content(self.text(self.cur_span()))
                                == "resolution-mode"
                                && self.tokens.get(self.pos + 1).map(|token| token.kind)
                                    == Some(TokenKind::Colon)
                            {
                                has_resolution_mode = true;
                            }
                            self.bump_skipped_import_token(&mut template_tags);
                        }
                    }
                }
            }
        }
        has_resolution_mode
    }

    /// Skip optional second argument in import type: `import("mod", { with: { ... } })`.
    /// Consumes `, { ... }` (with balanced braces) if present.
    fn skip_import_type_options(&mut self) -> bool {
        let mut has_resolution_mode = false;
        if self.eat(TokenKind::Comma).is_some() && self.at(TokenKind::OpenBrace) {
            self.bump(); // consume `{`
            let mut depth = 1u32;
            let mut template_tags = Vec::new();
            while depth > 0 && !self.is_eof() {
                match self.cur() {
                    TokenKind::OpenBrace => {
                        depth += 1;
                        self.bump();
                    }
                    TokenKind::CloseBrace => {
                        depth -= 1;
                        self.bump();
                    }
                    _ => {
                        if matches!(self.cur(), TokenKind::StringLiteral | TokenKind::Identifier)
                            && Self::string_literal_content(self.text(self.cur_span()))
                                == "resolution-mode"
                            && self.tokens.get(self.pos + 1).map(|token| token.kind)
                                == Some(TokenKind::Colon)
                        {
                            has_resolution_mode = true;
                        }
                        self.bump_skipped_import_token(&mut template_tags);
                    }
                }
            }
        }
        has_resolution_mode
    }

    fn span_from(&self, start: u32) -> Span {
        let mut end = if self.pos > 0 && self.pos - 1 < self.tokens.len() {
            self.tokens[self.pos - 1].span.end
        } else {
            start
        };
        // A `>` split off the current token belongs to the node being closed.
        // The remainder must still start right after it: a speculative parse
        // that rolled the split back restores the compound token, whose start
        // is one byte earlier, and the stale entry is then ignored.
        if let Some((pos, split_end)) = self.split_gt_end {
            if pos == self.pos
                && split_end > end
                && self
                    .tokens
                    .get(pos)
                    .is_some_and(|t| t.span.start == split_end)
            {
                end = split_end;
            }
        }
        Span::new(start, end)
    }

    /// Whether the current token is on a different line from the previous one.
    ///
    /// The scanner already recorded this while skipping trivia, so this is a field
    /// read. It used to re-scan the raw source between the two tokens and run
    /// `contains('\n')` on every call — which made this helper one of the hottest
    /// functions in the parser.
    fn is_on_new_line(&self) -> bool {
        if self.pos == 0 || self.pos >= self.tokens.len() {
            return false;
        }
        self.tokens[self.pos].preceded_by_line_break
    }

    /// Whether a token reached by a statement-list recovery loop can begin a
    /// new statement. A stray `@` embedded in an unfinished expression must
    /// not be reinterpreted as a declaration decorator merely because
    /// recovery returned control to the outer list.
    fn current_token_starts_statement(&self) -> bool {
        self.pos == 0
            || self.is_on_new_line()
            || self
                .tokens
                .get(self.pos.wrapping_sub(1))
                .is_some_and(|token| {
                    matches!(
                        token.kind,
                        TokenKind::Semicolon
                            | TokenKind::OpenBrace
                            | TokenKind::CloseBrace
                            | TokenKind::Colon
                    )
                })
    }

    /// Check whether the next (peek) token is on a different line from the
    /// current token.
    fn peek_is_on_new_line(&self) -> bool {
        let next_idx = self.pos + 1;
        if next_idx >= self.tokens.len() {
            return false;
        }
        self.tokens[next_idx].preceded_by_line_break
    }

    fn peek_is(&self, kind: TokenKind) -> bool {
        self.tokens
            .get(self.pos + 1)
            .is_some_and(|t| t.kind == kind)
    }

    fn peek2_is(&self, kind: TokenKind) -> bool {
        self.tokens
            .get(self.pos + 2)
            .is_some_and(|t| t.kind == kind)
    }

    fn peek_is_numeric(&self) -> bool {
        self.tokens.get(self.pos + 1).is_some_and(|t| {
            t.kind == TokenKind::NumericLiteral || t.kind == TokenKind::BigIntLiteral
        })
    }

    fn peek_is_ident_or_keyword(&self) -> bool {
        self.tokens.get(self.pos + 1).is_some_and(|t| {
            t.kind == TokenKind::Identifier || t.kind.is_keyword() || t.kind.is_contextual_keyword()
        })
    }

    /// Returns true when the parser is at `type` and peek is `as` followed by
    /// a token indicating `type` is the local/imported name being aliased,
    /// NOT a type-only modifier.
    ///
    /// `export { type as foo }` → true  (type is local, foo is alias)
    /// `export { type as }` → false  (type is modifier, as is local)
    /// `export { type as as bar }` → false  (type is modifier, as is local, bar is alias)
    /// `import { type as as }` → true  (type is imported, as is local)
    fn peek_is_type_as_alias_start(&self) -> bool {
        if !self.peek_is(TokenKind::As) {
            return false;
        }
        // Look at the token after `as` (pos+2)
        match self.tokens.get(self.pos + 2) {
            Some(t) if t.kind == TokenKind::As => {
                // `type as as ...` — need 3-token lookahead:
                // If pos+3 is a name/string: `type` IS modifier (as=name, as=keyword, pos3=alias)
                // If pos+3 is `,`/`}`/EOF: `type` is NOT modifier (type=imported, as=keyword, as=local)
                !self.tokens.get(self.pos + 3).is_some_and(|t3| {
                    t3.kind == TokenKind::Identifier
                        || t3.kind.is_keyword()
                        || t3.kind.is_contextual_keyword()
                        || t3.kind == TokenKind::StringLiteral
                })
            }
            Some(t) => {
                // `type as <X>` where X is not `as` → type is local name if X is name/string
                t.kind == TokenKind::Identifier
                    || t.kind.is_keyword()
                    || t.kind.is_contextual_keyword()
                    || t.kind == TokenKind::StringLiteral
            }
            // `type as` at end → type is modifier
            None => false,
        }
    }

    fn peek_could_start_prop_name(&self) -> bool {
        self.tokens.get(self.pos + 1).is_some_and(|t| {
            matches!(
                t.kind,
                TokenKind::Identifier
                    | TokenKind::StringLiteral
                    | TokenKind::NumericLiteral
                    | TokenKind::OpenBracket
                    | TokenKind::Hash
            ) || t.kind.is_keyword()
                || t.kind.is_contextual_keyword()
        })
    }

    /// Returns `true` if the current token could start a new object literal
    /// property (used for error recovery when a comma is missing between
    /// properties).
    /// Check if the current token could start an expression (for error recovery
    /// when a comma is missing in array/object literals).
    pub(crate) fn cur_could_start_expr(&self) -> bool {
        let k = self.cur();
        matches!(
            k,
            TokenKind::Identifier
                | TokenKind::StringLiteral
                | TokenKind::NumericLiteral
                | TokenKind::BigIntLiteral
                | TokenKind::OpenBracket
                | TokenKind::OpenBrace
                | TokenKind::OpenParen
                | TokenKind::DotDotDot
                | TokenKind::Plus
                | TokenKind::Minus
                | TokenKind::Excl
                | TokenKind::Tilde
                | TokenKind::PlusPlus
                | TokenKind::MinusMinus
                | TokenKind::New
                | TokenKind::TypeOf
                | TokenKind::Void
                | TokenKind::Delete
                | TokenKind::This
                | TokenKind::Super
                | TokenKind::True
                | TokenKind::False
                | TokenKind::Null
                | TokenKind::Function
                | TokenKind::Class
                | TokenKind::Async
                | TokenKind::Await
                | TokenKind::Yield
                | TokenKind::Import
                | TokenKind::NoSubstitutionTemplate
                | TokenKind::TemplateHead
        )
    }

    pub(crate) fn cur_could_start_obj_prop(&self) -> bool {
        let k = self.cur();
        matches!(
            k,
            TokenKind::Identifier
                | TokenKind::StringLiteral
                | TokenKind::NumericLiteral
                | TokenKind::OpenBracket
                | TokenKind::Hash
                | TokenKind::DotDotDot
                | TokenKind::Asterisk
                | TokenKind::Get
                | TokenKind::Set
                | TokenKind::Async
        ) || k.is_keyword()
            || k.is_contextual_keyword()
    }

    fn peek_could_be_member(&self) -> bool {
        self.tokens.get(self.pos + 1).is_some_and(|t| {
            matches!(
                t.kind,
                TokenKind::Identifier
                    | TokenKind::StringLiteral
                    | TokenKind::NumericLiteral
                    | TokenKind::OpenBracket
                    | TokenKind::Hash
                    | TokenKind::Asterisk
            ) || t.kind.is_keyword()
                || t.kind.is_contextual_keyword()
        })
    }

    /// Check if the next token can follow a modifier keyword.
    /// Matches TypeScript's `canFollowModifier()` — the token after a modifier
    /// must be an identifier, literal property name, `[`, `{`, `*`, `...`, or `#`.
    fn peek_can_follow_modifier(&self) -> bool {
        self.tokens.get(self.pos + 1).is_some_and(|t| {
            matches!(
                t.kind,
                TokenKind::Identifier
                    | TokenKind::StringLiteral
                    | TokenKind::NumericLiteral
                    | TokenKind::OpenBracket
                    | TokenKind::OpenBrace
                    | TokenKind::Asterisk
                    | TokenKind::DotDotDot
                    | TokenKind::Hash
            ) || t.kind.is_keyword()
                // Contextual keywords (static, abstract, declare, etc.) are valid property names
                || t.kind.is_contextual_keyword()
        })
    }

    /// Check if next token is on a new line AND can follow a modifier.
    /// TypeScript's `nextTokenIsOnSameLineAndCanFollowModifier()`.
    fn peek_same_line_can_follow_modifier(&self) -> bool {
        !self.peek_is_on_new_line() && self.peek_can_follow_modifier()
    }

    /// Check if the current token is a modifier keyword that `parse_member_modifiers` would consume.
    fn is_modifier_keyword(&self) -> bool {
        matches!(
            self.cur(),
            TokenKind::Public
                | TokenKind::Private
                | TokenKind::Protected
                | TokenKind::Static
                | TokenKind::Readonly
                | TokenKind::Abstract
                | TokenKind::Override
                | TokenKind::Async
                | TokenKind::Accessor
                | TokenKind::Declare
                | TokenKind::Const
                | TokenKind::Export
        )
    }

    /// Check if the next token is something that follows a parameter name directly,
    /// indicating the current token is a parameter name, not a modifier.
    fn peek_is_param_name_follower(&self) -> bool {
        self.tokens.get(self.pos + 1).is_some_and(|t| {
            matches!(
                t.kind,
                TokenKind::CloseParen
                    | TokenKind::Comma
                    | TokenKind::Question
                    | TokenKind::Colon
                    | TokenKind::Equals
                    // `@` after a modifier keyword means the keyword is a parameter
                    // name, not a modifier: `constructor(public @dec p: number)`
                    // → two params: `public` and `p` (with decorator).
                    | TokenKind::At
            )
        })
    }

    /// Returns `true` when the next token (peek) is a keyword that can begin a
    /// declaration after `declare`, so that `declare` should be consumed as a
    /// modifier.  When this returns `false`, `declare` is an identifier and
    /// should NOT be consumed.
    fn peek_can_follow_declare(&self) -> bool {
        // ASI: if the next token is on a new line, `declare` is an identifier expression.
        if self.peek_is_on_new_line() {
            return false;
        }
        let Some(t) = self.tokens.get(self.pos + 1) else {
            return false;
        };
        if matches!(
            t.kind,
            TokenKind::Var
                | TokenKind::Let
                | TokenKind::Const
                | TokenKind::Function
                | TokenKind::Class
                | TokenKind::Enum
                | TokenKind::Interface
                | TokenKind::Type
                | TokenKind::Global
                | TokenKind::Abstract
                | TokenKind::Async
                | TokenKind::Await
                | TokenKind::Using
                | TokenKind::Import
                | TokenKind::Export
                | TokenKind::Declare
        ) {
            return true;
        }
        // `declare module` / `declare namespace` only when followed by a valid
        // module name (identifier, string literal, contextual keyword), NOT a
        // template literal — `declare module \`M\`` should emit as
        // `declare; module \`M\`;`, not as a module declaration.
        if matches!(t.kind, TokenKind::Module | TokenKind::Namespace) {
            return self.tokens.get(self.pos + 2).is_some_and(|t2| {
                matches!(t2.kind, TokenKind::Identifier | TokenKind::StringLiteral
                    // `declare module { }` — nameless ambient module
                    | TokenKind::OpenBrace)
                    || t2.kind.is_contextual_keyword()
                    // Strict keywords can also be namespace names
                    // (e.g. `declare namespace debugger {}`)
                    || t2.kind.is_keyword()
            });
        }
        false
    }

    fn is_let_declaration(&self) -> bool {
        self.tokens.get(self.pos + 1).is_some_and(|t| {
            matches!(
                t.kind,
                TokenKind::Identifier | TokenKind::OpenBrace | TokenKind::OpenBracket
            ) || t.kind.is_keyword()
                || t.kind.is_contextual_keyword()
        })
    }

    fn is_using_declaration(&self) -> bool {
        self.peek_is(TokenKind::Identifier)
            || self.peek_is(TokenKind::Await)
            || self.peek_is(TokenKind::OpenBrace)
            || self.is_js_file && self.peek_is(TokenKind::OpenBracket)
    }

    fn is_interface_declaration(&self) -> bool {
        // ASI: newline after `interface` means it's an identifier expression.
        // Only identifiers and contextual keywords can be interface names.
        // Strict reserved keywords (void, return, class, etc.) are NOT valid.
        !self.peek_is_on_new_line()
            && self
                .tokens
                .get(self.pos + 1)
                .is_some_and(|t| t.kind == TokenKind::Identifier || t.kind.is_contextual_keyword())
    }

    fn is_type_alias_declaration(&self) -> bool {
        // ASI: newline after `type` means it's an identifier expression.
        // Only identifiers and contextual keywords can be type alias names.
        // Strict reserved keywords (void, return, class, etc.) are NOT valid.
        !self.peek_is_on_new_line()
            && self
                .tokens
                .get(self.pos + 1)
                .is_some_and(|t| t.kind == TokenKind::Identifier || t.kind.is_contextual_keyword())
    }

    fn is_module_declaration(&self) -> bool {
        // ASI: newline after `module`/`namespace` means it's an identifier expression.
        // Only identifiers, contextual keywords, and string literals can be module names.
        // Strict reserved keywords (void, return, class, etc.) are NOT valid.
        if self.peek_is_on_new_line() {
            return false;
        }
        self.tokens.get(self.pos + 1).is_some_and(|t| {
            matches!(t.kind, TokenKind::Identifier | TokenKind::StringLiteral)
                || t.kind.is_contextual_keyword()
        })
    }

    fn is_mapped_type_start(&self) -> bool {
        // { [K in ...] or { readonly [K in ...] or { +readonly [K in ...] or { -readonly [K in ...]
        let mut i = self.pos;
        // Skip optional readonly/+/-
        if let Some(t) = self.tokens.get(i) {
            if t.kind == TokenKind::Readonly
                || t.kind == TokenKind::Plus
                || t.kind == TokenKind::Minus
            {
                i += 1;
                if let Some(t2) = self.tokens.get(i) {
                    if t2.kind == TokenKind::Readonly {
                        i += 1;
                    }
                }
            }
        }
        // Expect [
        if let Some(t) = self.tokens.get(i) {
            if t.kind != TokenKind::OpenBracket {
                return false;
            }
            i += 1;
        } else {
            return false;
        }
        // Skip identifier
        if let Some(t) = self.tokens.get(i) {
            if t.kind == TokenKind::Identifier
                || t.kind.is_keyword()
                || t.kind.is_contextual_keyword()
            {
                i += 1;
            } else {
                return false;
            }
        } else {
            return false;
        }
        // Expect 'in'
        self.tokens.get(i).is_some_and(|t| t.kind == TokenKind::In)
    }

    fn is_asserts_keyword(&self) -> bool {
        self.peek_is_ident_or_keyword() || self.peek_is(TokenKind::This)
    }

    fn try_parse_type_assertion(&self) -> bool {
        // Heuristic: <Type>expr is a type assertion if next token after < looks like a type.
        // Types can start with: identifiers, keywords (typeof, keyof, infer, etc.),
        // `[` (tuple), `{` (object), `(` (grouped/function type), or < (generic type param),
        // literal types (numeric, string, boolean literals, null), or `-` (negative numeric literal).
        self.tokens.get(self.pos + 1).is_some_and(|t| {
            t.kind == TokenKind::Identifier
                || t.kind == TokenKind::Any
                || t.kind == TokenKind::Number
                || t.kind == TokenKind::String
                || t.kind == TokenKind::Boolean
                || t.kind == TokenKind::Void
                || t.kind == TokenKind::Never
                || t.kind == TokenKind::UnknownKeyword
                || t.kind == TokenKind::Object
                || t.kind == TokenKind::Symbol
                || t.kind == TokenKind::BigInt
                || t.kind == TokenKind::Undefined
                || t.kind == TokenKind::Intrinsic
                || t.kind == TokenKind::OpenBracket
                || t.kind == TokenKind::OpenBrace
                || t.kind == TokenKind::OpenParen
                || t.kind == TokenKind::TypeOf
                || t.kind == TokenKind::LessThan  // <T> can start a generic type
                || t.kind == TokenKind::NoSubstitutionTemplate  // template literal type
                || t.kind == TokenKind::TemplateHead  // template literal type with substitutions
                || t.kind == TokenKind::NumericLiteral  // <1 | 2> literal type assertion
                || t.kind == TokenKind::StringLiteral   // <"foo"> literal type assertion
                || t.kind == TokenKind::BigIntLiteral    // <1n> literal type assertion
                || t.kind == TokenKind::True             // <true> literal type assertion
                || t.kind == TokenKind::False            // <false> literal type assertion
                || t.kind == TokenKind::Null             // <null> literal type assertion
                || t.kind == TokenKind::Minus            // <-1> negative literal type assertion
                || t.kind.is_keyword()
        })
    }

    fn try_parse_type_for_assertion(&mut self) -> Option<TypeNode> {
        Some(self.parse_type())
    }

    fn can_recover_missing_type_assertion_gt(&self) -> bool {
        !matches!(
            self.cur(),
            TokenKind::GreaterThan
                | TokenKind::LessThan
                | TokenKind::EqualsEquals
                | TokenKind::EqualsEqualsEquals
                | TokenKind::ExclEquals
                | TokenKind::ExclEqualsEquals
                | TokenKind::Semicolon
                | TokenKind::Comma
                | TokenKind::CloseParen
                | TokenKind::CloseBracket
                | TokenKind::CloseBrace
        )
    }

    fn is_type_assertion_expr_terminator(&self) -> bool {
        self.is_eof()
            || matches!(
                self.cur(),
                TokenKind::Semicolon
                    | TokenKind::Comma
                    | TokenKind::CloseParen
                    | TokenKind::CloseBracket
                    | TokenKind::CloseBrace
            )
    }

    /// Check if the current token can follow type arguments in an expression.
    fn can_follow_type_arguments_in_expression(&self) -> bool {
        match self.cur() {
            TokenKind::OpenParen
            | TokenKind::NoSubstitutionTemplate
            | TokenKind::TemplateHead
            | TokenKind::Dot
            | TokenKind::QuestionDot
            | TokenKind::CloseParen
            | TokenKind::CloseBracket
            | TokenKind::Colon
            | TokenKind::Semicolon
            | TokenKind::Question
            | TokenKind::EqualsEquals
            | TokenKind::EqualsEqualsEquals
            | TokenKind::ExclEquals
            | TokenKind::ExclEqualsEquals
            | TokenKind::Ampersand
            | TokenKind::AmpersandAmpersand
            | TokenKind::Bar
            | TokenKind::BarBar
            | TokenKind::QuestionQuestion
            | TokenKind::Caret
            | TokenKind::Comma
            | TokenKind::CloseBrace
            | TokenKind::Equals
            | TokenKind::PlusEquals
            | TokenKind::MinusEquals
            | TokenKind::AsteriskEquals
            | TokenKind::SlashEquals
            | TokenKind::PercentEquals
            | TokenKind::LessLessEquals
            | TokenKind::GreaterGreaterEquals
            | TokenKind::GreaterGreaterGreaterEquals
            | TokenKind::AmpersandEquals
            | TokenKind::BarEquals
            | TokenKind::CaretEquals
            | TokenKind::AsteriskAsteriskEquals
            | TokenKind::QuestionQuestionEquals
            | TokenKind::AmpersandAmpersandEquals
            | TokenKind::BarBarEquals
            | TokenKind::InstanceOf
            | TokenKind::In => true,
            _ if self.is_eof() => true,
            // ASI: when `>` is followed by a newline and the next token
            // can start a new statement, TypeScript treats `expr<T>` as an
            // instantiation expression (not comparison).
            // Exclude `{` which could be a class/function body after extends.
            _ if self.is_on_new_line() && !self.at(TokenKind::OpenBrace) => true,
            _ => false,
        }
    }

    fn try_parse_as_function_type(&mut self, _start: u32) -> bool {
        // Simplified check - will be handled by parse_primary_type caller
        false
    }

    fn try_parse_param_list_for_fn_type(&mut self) -> Option<Vec<Param>> {
        let saved = self.pos;
        // SPECULATIVE parse: caller will rollback `self.pos` if we return
        // `None`, but we also need to discard any diagnostics emitted
        // during a failed attempt — otherwise `parse_binding_pattern`'s
        // `expect(CloseBrace)` (run when `(` is followed by `{` of an
        // OBJECT TYPE LITERAL rather than a destructured param) leaks a
        // spurious TS1002 even after rollback. Mirrors the pattern already
        // used around line 3843 for the speculative arrow-fn parse.
        let saved_diag = self.diagnostics.len();
        let bail = |this: &mut Self| -> Option<Vec<Param>> {
            this.rewind_to(saved);
            this.diagnostics.truncate(saved_diag);
            None
        };
        // Lookahead disambiguator: if we see `(` followed by `{`, peek
        // past the matching brace and decide whether this looks like a
        // destructured param or a parenthesized object type. The cheap
        // signal: object type literals separate members with `;` (and
        // optionally `,`); destructuring patterns only use `,`. If a
        // top-level `;` appears inside the braces, treat as a TYPE and
        // bail to the parenthesized-type fallback. Without this, rollup's
        // `ObjectHook<T, O = {}> = T | ({ handler: T; order?: ... } & O)`
        // crashed the param-list parser; downstream `Plugin` types using
        // ObjectHook then had broken shapes and every plugin object
        // literal in `vite.config.ts` tripped TS2322.
        if self.at(TokenKind::OpenBrace) {
            let mut depth: i32 = 0;
            let mut i = self.pos;
            while i < self.tokens.len() {
                match self.tokens[i].kind {
                    TokenKind::OpenBrace => depth += 1,
                    TokenKind::CloseBrace => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    TokenKind::Semicolon if depth == 1 => {
                        return bail(self);
                    }
                    _ => {}
                }
                i += 1;
            }
        }
        let mut params = Vec::new();
        while !self.at(TokenKind::CloseParen) && !self.is_eof() {
            let start = self.cur_span().start;
            // Keep parameter modifiers during speculative function-type
            // parsing. Invalid constructs such as `(public value) => T`
            // are still syntactically a function type; the checker owns the
            // specific parameter-property diagnostic. Rejecting `public`
            // here made us fall back to a parenthesized type and cascade
            // generic TS1005/TS1109 parser errors instead.
            let modifiers = self.parse_member_modifiers();
            let dotdotdot = self.eat(TokenKind::DotDotDot).is_some();
            // Accept identifiers, `this`, and destructuring patterns ({, [)
            if !self.is_identifier()
                && !self.at(TokenKind::This)
                && !self.at(TokenKind::OpenBrace)
                && !self.at(TokenKind::OpenBracket)
            {
                return bail(self);
            }
            let pat = if self.at(TokenKind::This) {
                let s = self.bump();
                Pat {
                    kind: PatKind::Ident("this".into()),
                    span: s,
                }
            } else if self.at(TokenKind::OpenBrace) || self.at(TokenKind::OpenBracket) {
                self.parse_binding_pattern()
            } else {
                let (name, span) = self.parse_identifier();
                Pat {
                    kind: PatKind::Ident(name.into()),
                    span,
                }
            };
            let optional = self.eat(TokenKind::Question).is_some();
            let type_ann = if self.eat(TokenKind::Colon).is_some() {
                Some(self.parse_type())
            } else {
                None
            };
            let initializer = if self.eat(TokenKind::Equals).is_some() {
                Some(Box::new(self.parse_assignment_expr()))
            } else {
                None
            };
            params.push(Param {
                name: pat,
                type_ann,
                initializer,
                dotdotdot,
                optional,
                modifiers,
                decorators: Vec::new(),
                span: self.span_from(start),
            });
            if self.eat(TokenKind::Comma).is_none() {
                break;
            }
        }
        if !self.at(TokenKind::CloseParen) {
            return bail(self);
        }
        self.bump(); // )
                     // Speculative parse succeeded — keep `self.diagnostics` as-is
                     // (any genuine errors from a successful parse path stay).
        Some(params)
    }
}

// ---------------------------------------------------------------------------
// Utility functions
// ---------------------------------------------------------------------------

fn expr_to_for_in_of_left(expr: Expr) -> ForInOfLeft {
    fn is_property_target(expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Member(_) | ExprKind::ElemAccess(_) => true,
            ExprKind::Paren(inner) | ExprKind::NonNull(inner) => is_property_target(inner),
            ExprKind::As(as_expr) => is_property_target(&as_expr.expr),
            ExprKind::Satisfies(satisfies) => is_property_target(&satisfies.expr),
            ExprKind::TypeAssertion(assertion) => is_property_target(&assertion.expr),
            _ => false,
        }
    }

    if is_property_target(&expr) {
        ForInOfLeft::Expr(Box::new(expr))
    } else {
        ForInOfLeft::Pat(expr_to_pat(&expr))
    }
}

fn expr_to_pat(expr: &Expr) -> Pat {
    match &expr.kind {
        ExprKind::Ident(name) => Pat {
            kind: PatKind::Ident(name.clone()),
            span: expr.span,
        },
        // Unwrap type-level wrappers (as, satisfies, type assertion, non-null)
        // so that `for ((g satisfies number) of ...)` resolves to the
        // underlying identifier pattern.
        ExprKind::As(as_expr) => expr_to_pat(&as_expr.expr),
        ExprKind::Satisfies(sat_expr) => expr_to_pat(&sat_expr.expr),
        ExprKind::TypeAssertion(ta_expr) => expr_to_pat(&ta_expr.expr),
        ExprKind::NonNull(inner) => expr_to_pat(inner),
        ExprKind::Assign(assign) if assign.op == AssignOp::Assign => Pat {
            kind: PatKind::Assign(expr_to_pat(&assign.left).into(), assign.right.clone()),
            span: expr.span,
        },
        // Unwrap parentheses only when the inner expression ultimately resolves
        // to an identifier (possibly through type-level wrappers). This handles
        // `(g satisfies number)` → `g` while preserving `<error>` spans for
        // arbitrary parenthesized expressions like `(n[idx++])`.
        ExprKind::Paren(inner) => {
            let result = expr_to_pat(inner);
            if matches!(&result.kind, PatKind::Ident(name) if name == "<error>") {
                // Inner isn't a simple identifier; keep the full paren span
                Pat {
                    kind: PatKind::Ident("<error>".into()),
                    span: expr.span,
                }
            } else {
                result
            }
        }
        ExprKind::ArrayLit(elements) => Pat {
            kind: PatKind::Array(
                elements
                    .iter()
                    .map(|elem| {
                        elem.as_ref().map(|elem| match &elem.kind {
                            ExprKind::Spread(inner) => ArrayPatElem::Rest(expr_to_pat(inner)),
                            _ => ArrayPatElem::Pat(expr_to_pat(elem)),
                        })
                    })
                    .collect(),
            ),
            span: expr.span,
        },
        ExprKind::ObjectLit(props) => Pat {
            kind: PatKind::Object(
                props
                    .iter()
                    .map(|prop| match prop {
                        ObjLitProp::Property(prop) => {
                            ObjPatProp::KeyValue(prop.key.clone(), expr_to_pat(&prop.value))
                        }
                        ObjLitProp::Shorthand(name, span) => {
                            ObjPatProp::Shorthand(name.clone(), *span)
                        }
                        ObjLitProp::ShorthandDefault(name, init, span) => {
                            ObjPatProp::ShorthandAssign(name.clone(), init.clone(), *span)
                        }
                        ObjLitProp::Spread(expr, _) => ObjPatProp::Rest(expr_to_pat(expr)),
                        ObjLitProp::Method(_) | ObjLitProp::Get(_) | ObjLitProp::Set(_) => {
                            ObjPatProp::Shorthand("<error>".into(), expr.span)
                        }
                    })
                    .collect(),
            ),
            span: expr.span,
        },
        _ => Pat {
            kind: PatKind::Ident("<error>".into()),
            span: expr.span,
        },
    }
}
