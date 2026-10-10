//! Streaming TypeScript scanner.
//!
//! Mirrors the scanning model of TypeScript's `scanner.ts`:
//! - Call `scan()` to advance to the next non-trivia token.
//! - Query the current token via `token()`, `token_pos()`, `text_pos()`,
//!   `token_value()`, and `has_preceding_line_break()`.
//! - Use re-scan methods when the parser needs to reinterpret a token.

use crate::char_utils::*;
use crate::token_kind::TokenKind;

/// A comment recorded during scanning. Scanner tokens already use 32-bit
/// source spans, so reuse the AST representation and transfer the vector
/// directly into the parsed source file.
pub use tsc_rs_ast::Comment as RawComment;

pub struct TsScanner<'a> {
    source: &'a [u8],
    text: &'a str,
    pos: usize,
    end: usize,
    token_kind: TokenKind,
    token_start: usize,
    /// Cooked (escape-decoded) value, populated only when it differs from the
    /// raw source slice — i.e., string/template literals containing escapes,
    /// or identifiers containing `\u` escapes. `None` means callers should
    /// derive the value from `self.text[token_start..pos]` (with delimiter
    /// stripping for string/template kinds; see [`token_value`]).
    token_value_cooked: Option<String>,
    /// Token start associated with `token_value_cooked`. Keeping the previous
    /// rare cooked value avoids clearing/dropping an `Option<String>` on every
    /// ordinary token; `token_value` only observes it when this tag matches.
    token_value_cooked_start: usize,
    preceding_line_break: bool,
    /// Comments collected during scanning.
    pub comments: Vec<RawComment>,
    /// Start offsets of merge conflict markers skipped as trivia (TS1185).
    pub conflict_markers: Vec<u32>,
}

impl<'a> TsScanner<'a> {
    pub fn new(source: &'a str) -> Self {
        Self {
            source: source.as_bytes(),
            text: source,
            pos: 0,
            end: source.len(),
            token_kind: TokenKind::Unknown,
            token_start: 0,
            token_value_cooked: None,
            token_value_cooked_start: usize::MAX,
            preceding_line_break: false,
            comments: Vec::new(),
            conflict_markers: Vec::new(),
        }
    }

    // ----- Public query API -----

    /// Current token kind.
    pub fn token(&self) -> TokenKind {
        self.token_kind
    }

    /// Start position of the current token (after skipped trivia).
    pub fn token_pos(&self) -> usize {
        self.token_start
    }

    /// Current scan position (end of current token).
    pub fn text_pos(&self) -> usize {
        self.pos
    }

    /// Total length of the source text.
    pub fn source_len(&self) -> usize {
        self.end
    }

    /// Get the byte at a given position in the source.
    pub fn source_byte_at(&self, pos: usize) -> u8 {
        self.source[pos]
    }

    /// Byte at `i`, without a bounds check.
    ///
    /// The scanner reads source bytes tens of times per token, almost always
    /// immediately after testing `pos < end`. The redundant bounds check on each
    /// read was a large share of the lexer's per-token cost, so the hot paths use
    /// this instead. `self.end == self.source.len()` is established in `new()` and
    /// never changes, so `i < self.end` is exactly the in-bounds condition; the
    /// debug_assert catches any caller that gets it wrong under test.
    ///
    /// # Safety-critical invariant
    /// Every caller must have already established `i < self.end`.
    #[inline(always)]
    fn at(&self, i: usize) -> u8 {
        debug_assert!(
            i < self.end,
            "scanner read out of bounds: {i} >= {}",
            self.end
        );
        // SAFETY: caller guarantees i < end, and end == source.len().
        unsafe { *self.source.get_unchecked(i) }
    }

    /// Semantic value of the current token (identifier name, string content,
    /// numeric text, etc.).
    ///
    /// Most tokens have no cooked form distinct from the source slice, so the
    /// scanner avoids allocating a separate value during scanning. When a
    /// cooked form was produced (string/template with escapes, identifier
    /// with `\u` escapes), it is returned directly; otherwise the value is
    /// derived from the source slice with delimiter stripping for string and
    /// template kinds.
    pub fn token_value(&self) -> &str {
        if self.token_value_cooked_start == self.token_start {
            if let Some(s) = &self.token_value_cooked {
                return s.as_str();
            }
        }
        let raw = &self.text[self.token_start..self.pos];
        match self.token_kind {
            // `"foo"` or `'foo'` → strip both quotes when both delimiters are
            // present. Unterminated strings (only opening quote) lose just the
            // leading quote.
            TokenKind::StringLiteral => {
                if raw.len() >= 2 {
                    let last = raw.as_bytes()[raw.len() - 1];
                    if last == b'"' || last == b'\'' {
                        &raw[1..raw.len() - 1]
                    } else {
                        &raw[1..]
                    }
                } else {
                    ""
                }
            }
            // `` `foo` `` → strip both backticks.
            TokenKind::NoSubstitutionTemplate => {
                if raw.len() >= 2 && raw.as_bytes()[raw.len() - 1] == b'`' {
                    &raw[1..raw.len() - 1]
                } else if raw.len() >= 1 {
                    &raw[1..]
                } else {
                    ""
                }
            }
            // `` `foo${ `` → strip leading backtick and trailing `${`.
            TokenKind::TemplateHead => {
                if raw.len() >= 3 && raw.as_bytes()[..1] == [b'`'] && raw.ends_with("${") {
                    &raw[1..raw.len() - 2]
                } else {
                    raw
                }
            }
            // `}foo${` → strip leading `}` and trailing `${`.
            TokenKind::TemplateMiddle => {
                if raw.len() >= 3 && raw.as_bytes()[..1] == [b'}'] && raw.ends_with("${") {
                    &raw[1..raw.len() - 2]
                } else {
                    raw
                }
            }
            // `}foo`` → strip leading `}` and trailing backtick.
            TokenKind::TemplateTail => {
                if raw.len() >= 2 && raw.as_bytes()[..1] == [b'}'] && raw.ends_with('`') {
                    &raw[1..raw.len() - 1]
                } else {
                    raw
                }
            }
            _ => raw,
        }
    }

    /// Raw source text slice of the current token.
    pub fn token_text(&self) -> &str {
        &self.text[self.token_start..self.pos]
    }

    /// Whether there was a line break before the current token.
    pub fn has_preceding_line_break(&self) -> bool {
        self.preceding_line_break
    }

    /// Extend the current identifier/keyword using JSX identifier rules. Hyphens
    /// and identifier continuation characters are part of the same token, even
    /// when a hyphen is trailing or followed by a digit. Trivia ends the name.
    pub fn scan_jsx_identifier(&mut self) -> TokenKind {
        if !self.token_kind.is_identifier_name() {
            return self.token_kind;
        }
        let mut cooked = if self.token_value_cooked_start == self.token_start {
            self.token_value_cooked.clone()
        } else {
            None
        };
        while self.pos < self.end {
            let byte = self.source[self.pos];
            if byte == b'-' || is_identifier_continue(byte) {
                self.pos += 1;
                if let Some(value) = &mut cooked {
                    value.push(byte as char);
                }
            } else if byte == b'\\' && self.source.get(self.pos + 1) == Some(&b'u') {
                let escape_start = self.pos;
                self.pos += 2;
                let character = self.scan_unicode_escape();
                if let Some(character) = character.filter(|ch| is_unicode_identifier_continue(*ch))
                {
                    cooked
                        .get_or_insert_with(|| self.text[self.token_start..escape_start].to_owned())
                        .push(character);
                } else {
                    self.pos = escape_start;
                    break;
                }
            } else if byte >= 0x80 {
                let before = self.pos;
                let character = self.decode_utf8_char();
                if !is_unicode_identifier_continue(character) {
                    self.pos = before;
                    break;
                }
                if let Some(value) = &mut cooked {
                    value.push(character);
                }
            } else {
                break;
            }
        }
        if let Some(value) = cooked {
            self.set_token_value_cooked(value);
        }
        self.token_kind = TokenKind::keyword_from_str(self.token_value());
        self.token_kind
    }

    // ----- Main scan entry point -----

    /// Advance to the next token, skipping trivia. Returns the token kind.
    pub fn scan(&mut self) -> TokenKind {
        // SAFETY: the pointer and limit come from this scanner's immutable source.
        let result = unsafe { self.scan_batch_from(self.pos, self.source.as_ptr(), self.end) };
        self.pos = result.pos;
        self.token_kind = result.kind;
        result.kind
    }

    /// Batch-scanner entry point. Unlike the public streaming API, the final
    /// cursor and kind are returned without requiring them to be published into
    /// scanner state; the fused producer can carry the common path directly
    /// into the next token. Re-scan paths explicitly publish the rare token that
    /// needs it.
    ///
    /// # Safety
    /// `source` must be the base pointer of this scanner's source allocation and
    /// `end` must be its byte length.
    pub(crate) unsafe fn scan_batch_from(
        &mut self,
        mut pos: usize,
        source: *const u8,
        end: usize,
    ) -> ScanResult {
        self.preceding_line_break = false;

        debug_assert_eq!(end, self.source.len());
        debug_assert_eq!(source, self.source.as_ptr());

        if pos == 0
            && end >= 3
            && unsafe { source.read() } == 0xEF
            && unsafe { source.add(1).read() } == 0xBB
            && unsafe { source.add(2).read() } == 0xBF
        {
            pos = 3;
        }

        loop {
            if pos >= end {
                self.token_start = pos;
                return ScanResult {
                    pos,
                    kind: TokenKind::EndOfFile,
                };
            }

            let token_start = pos;
            // SAFETY: `pos < end == self.source.len()` and `source` is its base.
            let ch = unsafe { source.add(pos).read() };
            let result = SCAN_HANDLERS[ch as usize](self, ch, pos);
            pos = result.pos;
            let kind = result.kind;
            if kind.is_trivia() {
                continue;
            }

            self.token_start = token_start;
            return ScanResult { pos, kind };
        }
    }

    #[inline(always)]
    pub(crate) fn source_ptr(&self) -> *const u8 {
        self.source.as_ptr()
    }

    #[inline(always)]
    pub(crate) fn publish_batch_token(&mut self, pos: usize, kind: TokenKind) {
        self.pos = pos;
        self.token_kind = kind;
    }

    #[inline]
    fn set_token_value_cooked(&mut self, value: String) {
        self.token_value_cooked = Some(value);
        self.token_value_cooked_start = self.token_start;
    }

    // ----- Re-scan methods -----

    /// Re-scan `>` as `>>`, `>>>`, `>=`, `>>=`, or `>>>=`.
    /// Called by the parser when it knows the `>` is not closing a type argument.
    pub fn re_scan_greater_token(&mut self) -> TokenKind {
        if self.token_kind == TokenKind::GreaterThan {
            // Reset position to just after the `>`
            self.pos = self.token_start + 1;
            self.scan_greater_than_rest()
        } else {
            self.token_kind
        }
    }

    /// Re-scan `/` or `/=` as a regex literal.
    /// Called by the parser when it determines the `/` starts a regex.
    pub fn re_scan_slash_token(&mut self) -> TokenKind {
        if self.token_kind == TokenKind::Slash || self.token_kind == TokenKind::SlashEquals {
            // Preserve the original slash-ish token kind so an unterminated
            // rescan can revert to what we started with (Slash vs SlashEquals).
            let original_kind = self.token_kind;
            let original_pos = self.pos;
            // Reset to just after the initial `/`
            self.pos = self.token_start + 1;
            let mut in_char_class = false;
            let mut escaped = false;

            while self.pos < self.end {
                let ch = self.source[self.pos];
                if is_line_break_at(self.source, self.pos) {
                    break; // unterminated regex
                }
                if escaped {
                    escaped = false;
                    self.pos += 1;
                    continue;
                }
                if ch == b'\\' {
                    escaped = true;
                    self.pos += 1;
                    continue;
                }
                if ch == b'[' {
                    in_char_class = true;
                    self.pos += 1;
                    continue;
                }
                if ch == b']' {
                    in_char_class = false;
                    self.pos += 1;
                    continue;
                }
                // `</` outside character class indicates a JSX/HTML closing tag
                // misidentified as regex body (e.g., `&lt;/head&gt;</code>`).
                // Bare `<` without `/` is valid in regex (e.g., /<link/).
                if ch == b'<' && !in_char_class {
                    // Only a real closing tag (`</name>`, `</>`): in
                    // `/a"></g` the `/` after `<` ends the regex.
                    let closing_tag = self.pos + 1 < self.end && self.at(self.pos + 1) == b'/' && {
                        let mut at = self.pos + 2;
                        while at < self.end
                            && (self.at(at).is_ascii_alphanumeric()
                                || matches!(self.at(at), b'-' | b'.' | b':' | b'_' | b'$'))
                        {
                            at += 1;
                        }
                        while at < self.end && matches!(self.at(at), b' ' | b'\t') {
                            at += 1;
                        }
                        at < self.end && self.at(at) == b'>'
                    };
                    if closing_tag {
                        // `</` — closing tag, not regex; revert to the
                        // slash-ish token we started with.
                        self.pos = original_pos;
                        self.token_kind = original_kind;
                        return self.token_kind;
                    }
                }
                if ch == b'/' && !in_char_class {
                    self.pos += 1; // consume closing /
                                   // Consume identifier parts, keeping Unicode whitespace
                                   // and punctuation outside the regular expression.
                    for character in self.text[self.pos..self.end].chars() {
                        if !is_unicode_identifier_continue(character) {
                            break;
                        }
                        self.pos += character.len_utf8();
                    }
                    // Raw token text matches the value verbatim — no cook needed.
                    self.token_kind = TokenKind::RegExpLiteral;
                    return self.token_kind;
                }
                self.pos += 1;
            }

            // Unterminated regex -- revert to the slash-ish token we started
            // with (preserves `/=` as SlashEquals when there's no closing `/`).
            self.pos = original_pos;
            self.token_kind = original_kind;
            self.token_kind
        } else {
            self.token_kind
        }
    }

    /// Re-scan `}` as a template middle or template tail.
    /// Called by the parser when it knows `}` ends a template expression.
    pub fn re_scan_template_token(&mut self) -> TokenKind {
        debug_assert!(
            self.token_kind == TokenKind::CloseBrace,
            "re_scan_template_token called on non-CloseBrace"
        );
        // pos is currently past the `}`, start scanning template content
        self.token_start = self.pos - 1; // include the `}`
        self.scan_template_tail()
    }

    // ----- Internal scanning methods -----

    #[inline]
    fn peek(&self) -> u8 {
        if self.pos < self.end {
            self.at(self.pos)
        } else {
            0
        }
    }

    #[inline]
    fn peek_at(&self, offset: usize) -> u8 {
        let idx = self.pos + offset;
        if idx < self.end {
            self.at(idx)
        } else {
            0
        }
    }

    #[inline]
    fn is_at_line_start(&self) -> bool {
        if self.pos == 0 {
            return true;
        }
        let prev = self.source[self.pos - 1];
        if is_line_break(prev) {
            return true;
        }
        self.pos >= 3
            && self.source[self.pos - 3] == 0xE2
            && self.source[self.pos - 2] == 0x80
            && (self.source[self.pos - 1] == 0xA8 || self.source[self.pos - 1] == 0xA9)
    }

    fn is_conflict_marker_start(&self, first: u8) -> bool {
        if !self.is_at_line_start() {
            return false;
        }
        let marker: &[u8] = match first {
            b'<' => b"<<<<<<<",
            b'=' => b"=======",
            b'>' => b">>>>>>>",
            b'|' => b"|||||||",
            _ => return false,
        };
        if self.pos + marker.len() > self.end {
            return false;
        }
        if &self.source[self.pos..self.pos + marker.len()] != marker {
            return false;
        }
        if self.pos + marker.len() == self.end {
            return true;
        }
        let next = self.source[self.pos + marker.len()];
        next == b' ' || next == b'\t' || is_line_break(next)
    }

    #[cold]
    #[inline(never)]
    fn scan_conflict_marker_trivia(&mut self) {
        self.conflict_markers.push(self.pos as u32);
        let marker = self.source[self.pos];
        if marker == b'<' || marker == b'>' {
            // `<<<<<<<` and `>>>>>>>` — only skip to end of line.
            while self.pos < self.end && !is_line_break(self.source[self.pos]) {
                self.pos += 1;
            }
        } else {
            // `=======` — skip everything until `>>>>>>>` at line start
            // `|||||||` — skip everything until `=======` at line start
            // First skip the marker line itself.
            while self.pos < self.end && !is_line_break(self.source[self.pos]) {
                self.pos += 1;
            }
            // Now skip all content lines until we find the terminating marker.
            let end_marker: &[u8] = if marker == b'=' {
                b">>>>>>>"
            } else {
                // `|||||||` terminates at `=======`
                b"======="
            };
            while self.pos < self.end {
                // Skip line break(s) to get to next line
                if is_line_break(self.source[self.pos]) {
                    if self.source[self.pos] == b'\r'
                        && self.pos + 1 < self.end
                        && self.source[self.pos + 1] == b'\n'
                    {
                        self.pos += 2;
                    } else {
                        self.pos += 1;
                    }
                }
                // Check if this line starts with the end marker
                if self.pos + end_marker.len() <= self.end
                    && &self.source[self.pos..self.pos + end_marker.len()] == end_marker
                {
                    // Found it — stop here. The main scan loop will pick up
                    // this marker and process it on its next iteration.
                    break;
                }
                // Skip this content line
                while self.pos < self.end && !is_line_break(self.source[self.pos]) {
                    self.pos += 1;
                }
            }
        }
        self.token_kind = TokenKind::ConflictMarkerTrivia;
    }

    fn scan_dot(&mut self) {
        self.pos += 1;
        if self.peek() == b'.' && self.peek_at(1) == b'.' {
            self.pos += 2;
            self.token_kind = TokenKind::DotDotDot;
        } else if self.pos < self.end && self.source[self.pos].is_ascii_digit() {
            // Number starting with `.`
            self.scan_number_fragment_after_dot();
            self.finish_number();
        } else {
            self.token_kind = TokenKind::Dot;
        }
    }

    fn scan_question(&mut self) {
        self.pos += 1;
        if self.peek() == b'.' && !self.peek_at(1).is_ascii_digit() {
            self.pos += 1;
            self.token_kind = TokenKind::QuestionDot;
        } else if self.peek() == b'?' {
            self.pos += 1;
            if self.peek() == b'=' {
                self.pos += 1;
                self.token_kind = TokenKind::QuestionQuestionEquals;
            } else {
                self.token_kind = TokenKind::QuestionQuestion;
            }
        } else {
            self.token_kind = TokenKind::Question;
        }
    }

    fn scan_less_than(&mut self) {
        self.pos += 1;
        if self.peek() == b'<' {
            self.pos += 1;
            if self.peek() == b'=' {
                self.pos += 1;
                self.token_kind = TokenKind::LessLessEquals;
            } else {
                self.token_kind = TokenKind::LessLess;
            }
        } else if self.peek() == b'=' {
            self.pos += 1;
            self.token_kind = TokenKind::LessEqual;
        } else {
            self.token_kind = TokenKind::LessThan;
        }
    }

    fn scan_greater_than(&mut self) {
        self.pos += 1;
        self.scan_greater_than_rest();
    }

    fn scan_greater_than_rest(&mut self) -> TokenKind {
        // `>=>` / `>>=>` / `>>>=>` is never an operator (`a >= > b` is not
        // JS), only a type closing right before an arrow:
        // `(): Promise<void>=> x`. Leave the `=` for the next token so it
        // scans as `=>` instead of fusing into `>=` and stranding a `>`.
        let takes_equals = |s: &Self| s.peek() == b'=' && s.peek_at(1) != b'>';
        if self.peek() == b'>' {
            self.pos += 1;
            if self.peek() == b'>' {
                self.pos += 1;
                if takes_equals(self) {
                    self.pos += 1;
                    self.token_kind = TokenKind::GreaterGreaterGreaterEquals;
                } else {
                    self.token_kind = TokenKind::GreaterGreaterGreater;
                }
            } else if takes_equals(self) {
                self.pos += 1;
                self.token_kind = TokenKind::GreaterGreaterEquals;
            } else {
                self.token_kind = TokenKind::GreaterGreater;
            }
        } else if takes_equals(self) {
            self.pos += 1;
            self.token_kind = TokenKind::GreaterEqual;
        } else {
            self.token_kind = TokenKind::GreaterThan;
        }
        self.token_kind
    }

    fn scan_equals(&mut self) {
        self.pos += 1;
        if self.peek() == b'=' {
            self.pos += 1;
            if self.peek() == b'=' {
                self.pos += 1;
                self.token_kind = TokenKind::EqualsEqualsEquals;
            } else {
                self.token_kind = TokenKind::EqualsEquals;
            }
        } else if self.peek() == b'>' {
            self.pos += 1;
            self.token_kind = TokenKind::FatArrow;
        } else {
            self.token_kind = TokenKind::Equals;
        }
    }

    fn scan_exclamation(&mut self) {
        self.pos += 1;
        if self.peek() == b'=' {
            self.pos += 1;
            if self.peek() == b'=' {
                self.pos += 1;
                self.token_kind = TokenKind::ExclEqualsEquals;
            } else {
                self.token_kind = TokenKind::ExclEquals;
            }
        } else {
            self.token_kind = TokenKind::Excl;
        }
    }

    fn scan_plus(&mut self) {
        self.pos += 1;
        if self.peek() == b'+' {
            self.pos += 1;
            self.token_kind = TokenKind::PlusPlus;
        } else if self.peek() == b'=' {
            self.pos += 1;
            self.token_kind = TokenKind::PlusEquals;
        } else {
            self.token_kind = TokenKind::Plus;
        }
    }

    fn scan_minus(&mut self) {
        self.pos += 1;
        if self.peek() == b'-' {
            self.pos += 1;
            self.token_kind = TokenKind::MinusMinus;
        } else if self.peek() == b'=' {
            self.pos += 1;
            self.token_kind = TokenKind::MinusEquals;
        } else {
            self.token_kind = TokenKind::Minus;
        }
    }

    fn scan_asterisk(&mut self) {
        self.pos += 1;
        if self.peek() == b'*' {
            self.pos += 1;
            if self.peek() == b'=' {
                self.pos += 1;
                self.token_kind = TokenKind::AsteriskAsteriskEquals;
            } else {
                self.token_kind = TokenKind::AsteriskAsterisk;
            }
        } else if self.peek() == b'=' {
            self.pos += 1;
            self.token_kind = TokenKind::AsteriskEquals;
        } else {
            self.token_kind = TokenKind::Asterisk;
        }
    }

    /// Scan `/`, `/=`, or a comment. Returns `true` if a comment was scanned
    /// (caller should continue to skip trivia).
    /// Check if the current position might be inside a regex character class `[...]`.
    /// Scans backward to find potential regex-starting `/` chars, then forward-scans
    /// from each with proper character class tracking to determine if the current
    /// position falls inside a `[...]` in that regex.
    #[inline(never)]
    fn maybe_in_regex_char_class(&self) -> bool {
        // Find the start of the current line
        let mut line_start = self.pos;
        while line_start > 0 && !is_line_break(self.source[line_start - 1]) {
            line_start -= 1;
        }
        // Scan backward from current position looking for `/` that could start a regex
        let mut pos = self.pos;
        while pos > line_start {
            pos -= 1;
            if self.source[pos] != b'/' {
                continue;
            }
            // Check if this `/` could be a regex start by looking at what precedes it
            let mut p = pos;
            while p > line_start && self.source[p - 1] == b' ' {
                p -= 1;
            }
            if p > line_start {
                let prev = self.source[p - 1];
                // After ), ], }, alphanumeric, _, $ — this `/` is division, skip it
                if prev == b')'
                    || prev == b']'
                    || prev == b'}'
                    || prev.is_ascii_alphanumeric()
                    || prev == b'_'
                    || prev == b'$'
                {
                    continue;
                }
            }
            // This `/` could start a regex. Forward-scan with character class tracking.
            let mut fwd = pos + 1;
            let mut in_class = false;
            let mut escaped = false;
            while fwd < self.pos {
                let ch = self.source[fwd];
                if is_line_break_at(self.source, fwd) {
                    break;
                }
                if escaped {
                    escaped = false;
                    fwd += 1;
                    continue;
                }
                if ch == b'\\' {
                    escaped = true;
                    fwd += 1;
                    continue;
                }
                if ch == b'[' && !in_class {
                    in_class = true;
                } else if ch == b']' && in_class {
                    in_class = false;
                } else if ch == b'/' && !in_class {
                    // This `/` ends the regex before reaching our position
                    break;
                }
                fwd += 1;
            }
            if fwd >= self.pos && in_class {
                return true;
            }
        }
        false
    }

    #[cold]
    #[inline(never)]
    fn scan_slash_or_comment(&mut self) -> bool {
        let src = self.source;
        let end = self.end;
        let mut pos = self.pos;

        if pos + 1 < end {
            match src[pos + 1] {
                b'/' => {
                    // Before treating as line comment, check if we're inside
                    // a potential regex character class [...] where // is just
                    // two forward slash characters, not a comment.
                    if self.maybe_in_regex_char_class() {
                        self.pos = pos + 1;
                        self.token_kind = TokenKind::Slash;
                        return false;
                    }
                    // Single-line comment
                    let comment_start = pos;
                    pos += 2;
                    if pos < end {
                        if let Some(rel) = memchr::memchr2(b'\n', b'\r', &src[pos..end]) {
                            pos += rel;
                        } else {
                            pos = end;
                        }
                    }
                    self.pos = pos;
                    self.comments.push(RawComment {
                        pos: comment_start as u32,
                        end: pos as u32,
                        is_multiline: false,
                    });
                    return true;
                }
                b'*' => {
                    // Multi-line comment
                    let comment_start = pos;
                    pos += 2;
                    let mut found_end = false;
                    if end - pos >= 32 {
                        while pos + 1 < end {
                            let Some(rel) = memchr::memchr3(b'*', b'\n', b'\r', &src[pos..end])
                            else {
                                pos = end;
                                break;
                            };
                            pos += rel;
                            let ch = unsafe { *src.get_unchecked(pos) };
                            if ch == b'\n' || ch == b'\r' {
                                self.preceding_line_break = true;
                                pos += 1;
                                continue;
                            }
                            if unsafe { *src.get_unchecked(pos + 1) } == b'/' {
                                pos += 2;
                                found_end = true;
                                break;
                            }
                            pos += 1;
                        }
                    } else {
                        while pos + 1 < end {
                            let ch = unsafe { *src.get_unchecked(pos) };
                            if ch == b'\n' || ch == b'\r' {
                                self.preceding_line_break = true;
                            }
                            if ch == b'*' && unsafe { *src.get_unchecked(pos + 1) } == b'/' {
                                pos += 2;
                                found_end = true;
                                break;
                            }
                            pos += 1;
                        }
                    }
                    if !found_end {
                        pos = end;
                    }
                    self.pos = pos;
                    self.comments.push(RawComment {
                        pos: comment_start as u32,
                        end: pos as u32,
                        is_multiline: true,
                    });
                    return true;
                }
                b'=' => {
                    self.pos = pos + 2;
                    self.token_kind = TokenKind::SlashEquals;
                    return false;
                }
                _ => {}
            }
        }
        self.pos = pos + 1;
        self.token_kind = TokenKind::Slash;
        false
    }

    fn scan_percent(&mut self) {
        self.pos += 1;
        if self.peek() == b'=' {
            self.pos += 1;
            self.token_kind = TokenKind::PercentEquals;
        } else {
            self.token_kind = TokenKind::Percent;
        }
    }

    fn scan_ampersand(&mut self) {
        self.pos += 1;
        if self.peek() == b'&' {
            self.pos += 1;
            if self.peek() == b'=' {
                self.pos += 1;
                self.token_kind = TokenKind::AmpersandAmpersandEquals;
            } else {
                self.token_kind = TokenKind::AmpersandAmpersand;
            }
        } else if self.peek() == b'=' {
            self.pos += 1;
            self.token_kind = TokenKind::AmpersandEquals;
        } else {
            self.token_kind = TokenKind::Ampersand;
        }
    }

    fn scan_bar(&mut self) {
        self.pos += 1;
        if self.peek() == b'|' {
            self.pos += 1;
            if self.peek() == b'=' {
                self.pos += 1;
                self.token_kind = TokenKind::BarBarEquals;
            } else {
                self.token_kind = TokenKind::BarBar;
            }
        } else if self.peek() == b'=' {
            self.pos += 1;
            self.token_kind = TokenKind::BarEquals;
        } else {
            self.token_kind = TokenKind::Bar;
        }
    }

    fn scan_caret(&mut self) {
        self.pos += 1;
        if self.peek() == b'=' {
            self.pos += 1;
            self.token_kind = TokenKind::CaretEquals;
        } else {
            self.token_kind = TokenKind::Caret;
        }
    }

    // ----- String scanning -----

    #[cold]
    #[inline(never)]
    fn scan_string(&mut self, quote: u8) {
        self.pos += 1; // skip opening quote
        let body_start = self.pos;
        // Fast path: scan plain bytes until we hit the closing quote, an
        // escape, or a line break. No allocation when the string contains no
        // escapes (the overwhelmingly common case).
        while self.pos < self.end {
            let ch = self.at(self.pos);
            if ch == quote {
                self.pos += 1;
                self.token_kind = TokenKind::StringLiteral;
                return; // No cooked value for this token; raw[1..end-1] is the value.
            }
            if ch == b'\\' {
                // Slow path: cook escape sequences from here.
                return self.scan_string_with_escapes(quote, body_start);
            }
            if is_line_break(ch) {
                // Unterminated string; raw text (including opening quote) covers
                // it. token_value() will strip the leading quote.
                self.token_kind = TokenKind::StringLiteral;
                return;
            }
            if ch < 128 {
                self.pos += 1;
            } else {
                self.skip_utf8_char();
            }
        }
        // Unterminated at EOF.
        self.token_kind = TokenKind::StringLiteral;
    }

    /// Slow path for `scan_string` once an escape has been observed. The bytes
    /// in `self.text[body_start..self.pos]` have already been verified plain;
    /// `self.pos` points at the first `\`.
    #[cold]
    #[inline(never)]
    fn scan_string_with_escapes(&mut self, quote: u8, body_start: usize) {
        let mut value = String::with_capacity(self.pos - body_start + 16);
        value.push_str(&self.text[body_start..self.pos]);
        while self.pos < self.end {
            let ch = self.at(self.pos);
            if ch == quote {
                self.pos += 1;
                break;
            }
            if ch == b'\\' {
                self.pos += 1;
                if self.pos < self.end {
                    let esc = self.source[self.pos];
                    match esc {
                        b'n' => {
                            value.push('\n');
                            self.pos += 1;
                        }
                        b'r' => {
                            value.push('\r');
                            self.pos += 1;
                        }
                        b't' => {
                            value.push('\t');
                            self.pos += 1;
                        }
                        b'b' => {
                            value.push('\u{0008}');
                            self.pos += 1;
                        }
                        b'f' => {
                            value.push('\u{000C}');
                            self.pos += 1;
                        }
                        b'v' => {
                            value.push('\u{000B}');
                            self.pos += 1;
                        }
                        b'0' => {
                            value.push('\0');
                            self.pos += 1;
                        }
                        b'\'' => {
                            value.push('\'');
                            self.pos += 1;
                        }
                        b'"' => {
                            value.push('"');
                            self.pos += 1;
                        }
                        b'\\' => {
                            value.push('\\');
                            self.pos += 1;
                        }
                        b'`' => {
                            value.push('`');
                            self.pos += 1;
                        }
                        b'x' => {
                            self.pos += 1;
                            if let Some(ch) = self.scan_hex_digits(2) {
                                value.push(ch);
                            }
                        }
                        b'u' => {
                            self.pos += 1;
                            if let Some(ch) = self.scan_unicode_escape() {
                                value.push(ch);
                            }
                        }
                        b'\r' => {
                            self.pos += 1;
                            if self.pos < self.end && self.at(self.pos) == b'\n' {
                                self.pos += 1;
                            }
                        }
                        b'\n' => {
                            self.pos += 1;
                        }
                        _ => {
                            if esc < 128 {
                                value.push(esc as char);
                                self.pos += 1;
                            } else {
                                // Multi-byte UTF-8 char after backslash
                                let start = self.pos;
                                self.skip_utf8_char();
                                if let Ok(s) = std::str::from_utf8(&self.source[start..self.pos]) {
                                    value.push_str(s);
                                }
                            }
                        }
                    }
                }
            } else if is_line_break(ch) {
                break; // unterminated string
            } else {
                // Regular character (could be multi-byte UTF-8)
                if ch < 128 {
                    value.push(ch as char);
                    self.pos += 1;
                } else {
                    let start = self.pos;
                    self.skip_utf8_char();
                    value.push_str(&self.text[start..self.pos]);
                }
            }
        }
        self.set_token_value_cooked(value);
        self.token_kind = TokenKind::StringLiteral;
    }

    fn scan_hex_digits(&mut self, count: usize) -> Option<char> {
        let mut value: u32 = 0;
        for _ in 0..count {
            if self.pos >= self.end {
                return None;
            }
            let ch = self.at(self.pos);
            if let Some(digit) = hex_digit_value(ch) {
                value = value * 16 + digit as u32;
                self.pos += 1;
            } else {
                return None;
            }
        }
        char::from_u32(value)
    }

    fn scan_unicode_escape(&mut self) -> Option<char> {
        if self.pos < self.end && self.at(self.pos) == b'{' {
            // \u{XXXXX}
            self.pos += 1;
            let mut value: u32 = 0;
            let mut has_digit = false;
            while self.pos < self.end && self.source[self.pos] != b'}' {
                if let Some(digit) = hex_digit_value(self.source[self.pos]) {
                    value = value * 16 + digit as u32;
                    has_digit = true;
                    self.pos += 1;
                } else {
                    return None;
                }
            }
            if self.pos < self.end {
                self.pos += 1; // consume `}`
            }
            if has_digit {
                char::from_u32(value)
            } else {
                None
            }
        } else {
            // \uXXXX
            self.scan_hex_digits(4)
        }
    }

    // ----- Template scanning -----

    #[cold]
    #[inline(never)]
    fn scan_template(&mut self) {
        self.pos += 1; // skip opening backtick
        let result = self.scan_template_body();
        self.token_kind = result;
    }

    #[cold]
    #[inline(never)]
    fn scan_template_body(&mut self) -> TokenKind {
        let body_start = self.pos;
        // Fast path: scan plain bytes until `\``, `${`, an escape, or `\r`
        // (which requires CRLF → LF cooking). When the template body has no
        // escapes and no CR, the cooked value equals the raw slice.
        while self.pos < self.end {
            let ch = self.at(self.pos);
            if ch == b'`' {
                self.pos += 1;
                return TokenKind::NoSubstitutionTemplate;
            }
            if ch == b'$' && self.pos + 1 < self.end && self.at(self.pos + 1) == b'{' {
                self.pos += 2;
                return TokenKind::TemplateHead;
            }
            if ch == b'\\' || ch == b'\r' {
                return self.scan_template_body_with_escapes(body_start);
            }
            if ch < 128 {
                self.pos += 1;
            } else {
                self.skip_utf8_char();
            }
        }
        // Unterminated; raw text is the value.
        TokenKind::NoSubstitutionTemplate
    }

    #[cold]
    #[inline(never)]
    fn scan_template_body_with_escapes(&mut self, body_start: usize) -> TokenKind {
        let mut value = String::with_capacity(self.pos - body_start + 16);
        value.push_str(&self.text[body_start..self.pos]);
        while self.pos < self.end {
            let ch = self.at(self.pos);
            if ch == b'`' {
                self.pos += 1;
                self.set_token_value_cooked(value);
                return TokenKind::NoSubstitutionTemplate;
            }
            if ch == b'\\' {
                self.pos += 1;
                if self.pos < self.end {
                    let esc = self.source[self.pos];
                    match esc {
                        b'n' => {
                            value.push('\n');
                            self.pos += 1;
                        }
                        b'r' => {
                            value.push('\r');
                            self.pos += 1;
                        }
                        b't' => {
                            value.push('\t');
                            self.pos += 1;
                        }
                        b'b' => {
                            value.push('\u{0008}');
                            self.pos += 1;
                        }
                        b'f' => {
                            value.push('\u{000C}');
                            self.pos += 1;
                        }
                        b'v' => {
                            value.push('\u{000B}');
                            self.pos += 1;
                        }
                        b'0' => {
                            value.push('\0');
                            self.pos += 1;
                        }
                        b'\\' => {
                            value.push('\\');
                            self.pos += 1;
                        }
                        b'`' => {
                            value.push('`');
                            self.pos += 1;
                        }
                        b'$' => {
                            value.push('$');
                            self.pos += 1;
                        }
                        b'\r' => {
                            self.pos += 1;
                            if self.pos < self.end && self.at(self.pos) == b'\n' {
                                self.pos += 1;
                            }
                            value.push('\n');
                        }
                        b'\n' => {
                            self.pos += 1;
                            value.push('\n');
                        }
                        b'x' => {
                            self.pos += 1;
                            if let Some(ch) = self.scan_hex_digits(2) {
                                value.push(ch);
                            }
                        }
                        b'u' => {
                            self.pos += 1;
                            if let Some(ch) = self.scan_unicode_escape() {
                                value.push(ch);
                            }
                        }
                        _ => {
                            if esc < 128 {
                                value.push(esc as char);
                                self.pos += 1;
                            } else {
                                // Multi-byte UTF-8 char after backslash
                                let start = self.pos;
                                self.skip_utf8_char();
                                if let Ok(s) = std::str::from_utf8(&self.source[start..self.pos]) {
                                    value.push_str(s);
                                }
                            }
                        }
                    }
                }
            } else if ch == b'$' && self.pos + 1 < self.end && self.at(self.pos + 1) == b'{' {
                self.pos += 2; // skip `${`
                self.set_token_value_cooked(value);
                return TokenKind::TemplateHead;
            } else if ch == b'\r' {
                self.pos += 1;
                if self.pos < self.end && self.at(self.pos) == b'\n' {
                    self.pos += 1;
                }
                value.push('\n');
            } else if ch == b'\n' {
                self.pos += 1;
                value.push('\n');
            } else if ch < 128 {
                value.push(ch as char);
                self.pos += 1;
            } else {
                let start = self.pos;
                self.skip_utf8_char();
                value.push_str(&self.text[start..self.pos]);
            }
        }
        // Unterminated template
        self.set_token_value_cooked(value);
        TokenKind::NoSubstitutionTemplate
    }

    fn scan_template_tail(&mut self) -> TokenKind {
        // Fast path: walk plain bytes; bail to slow path on `\` or `\r`
        // (CRLF→LF normalization) since those need cooking. `${` ends as
        // TemplateMiddle, `\`` ends as TemplateTail.
        let body_start = self.pos;
        while self.pos < self.end {
            let ch = self.at(self.pos);
            if ch == b'`' {
                self.pos += 1;
                self.token_kind = TokenKind::TemplateTail;
                return self.token_kind;
            }
            if ch == b'$' && self.pos + 1 < self.end && self.at(self.pos + 1) == b'{' {
                self.pos += 2;
                self.token_kind = TokenKind::TemplateMiddle;
                return self.token_kind;
            }
            if ch == b'\\' || ch == b'\r' {
                return self.scan_template_tail_with_escapes(body_start);
            }
            if ch < 128 {
                self.pos += 1;
            } else {
                self.skip_utf8_char();
            }
        }
        // Unterminated; raw text covers it.
        self.token_kind = TokenKind::TemplateTail;
        self.token_kind
    }

    fn scan_template_tail_with_escapes(&mut self, body_start: usize) -> TokenKind {
        let mut value = String::with_capacity(self.pos - body_start + 16);
        value.push_str(&self.text[body_start..self.pos]);
        while self.pos < self.end {
            let ch = self.at(self.pos);
            if ch == b'`' {
                self.pos += 1;
                self.set_token_value_cooked(value);
                self.token_kind = TokenKind::TemplateTail;
                return self.token_kind;
            }
            if ch == b'\\' {
                self.pos += 1;
                if self.pos < self.end {
                    let esc = self.source[self.pos];
                    match esc {
                        b'n' => {
                            value.push('\n');
                            self.pos += 1;
                        }
                        b'r' => {
                            value.push('\r');
                            self.pos += 1;
                        }
                        b't' => {
                            value.push('\t');
                            self.pos += 1;
                        }
                        b'\\' => {
                            value.push('\\');
                            self.pos += 1;
                        }
                        b'`' => {
                            value.push('`');
                            self.pos += 1;
                        }
                        b'$' => {
                            value.push('$');
                            self.pos += 1;
                        }
                        b'\r' => {
                            self.pos += 1;
                            if self.pos < self.end && self.at(self.pos) == b'\n' {
                                self.pos += 1;
                            }
                            value.push('\n');
                        }
                        b'\n' => {
                            self.pos += 1;
                            value.push('\n');
                        }
                        b'x' => {
                            self.pos += 1;
                            if let Some(ch) = self.scan_hex_digits(2) {
                                value.push(ch);
                            }
                        }
                        b'u' => {
                            self.pos += 1;
                            if let Some(ch) = self.scan_unicode_escape() {
                                value.push(ch);
                            }
                        }
                        _ => {
                            value.push(esc as char);
                            self.pos += 1;
                        }
                    }
                }
            } else if ch == b'$' && self.pos + 1 < self.end && self.at(self.pos + 1) == b'{' {
                self.pos += 2;
                self.set_token_value_cooked(value);
                self.token_kind = TokenKind::TemplateMiddle;
                return self.token_kind;
            } else if ch == b'\r' {
                self.pos += 1;
                if self.pos < self.end && self.at(self.pos) == b'\n' {
                    self.pos += 1;
                }
                value.push('\n');
            } else if ch == b'\n' {
                self.pos += 1;
                value.push('\n');
            } else if ch < 128 {
                value.push(ch as char);
                self.pos += 1;
            } else {
                let start = self.pos;
                self.skip_utf8_char();
                value.push_str(&self.text[start..self.pos]);
            }
        }
        self.set_token_value_cooked(value);
        self.token_kind = TokenKind::TemplateTail;
        self.token_kind
    }

    // ----- Number scanning -----

    fn scan_number(&mut self) {
        let start_byte = self.at(self.pos);
        if start_byte == b'0' && self.pos + 1 < self.end {
            match self.at(self.pos + 1) {
                b'x' | b'X' => {
                    self.pos += 2;
                    self.scan_hex_integer();
                    return;
                }
                b'o' | b'O' => {
                    self.pos += 2;
                    self.scan_octal_integer();
                    return;
                }
                b'b' | b'B' => {
                    self.pos += 2;
                    self.scan_binary_integer();
                    return;
                }
                _ => {}
            }
        }

        // Decimal
        let int_start = self.pos;
        self.scan_digits();
        let int_end = self.pos;

        // TypeScript terminates a legacy-octal token before a numeric
        // separator (`01_2` => `01`, `_2`). `scan_digits` intentionally
        // consumes separators for ordinary decimal literals, so find the
        // first one and limit legacy-octal classification to the digit prefix.
        let legacy_end = self.source[int_start..int_end]
            .iter()
            .position(|byte| *byte == b'_')
            .map_or(int_end, |offset| int_start + offset);

        // Check for legacy octal: starts with 0, followed by digit(s),
        // all digits 0-7, and no separators. TypeScript's scanner stops
        // before `.` and `e` for valid legacy octals (e.g. `01.5` is
        // tokenized as `01` then `.5`).
        let is_legacy_octal = start_byte == b'0'
            && legacy_end > int_start + 1
            && self.at(int_start + 1).is_ascii_digit()
            && self.source[int_start..legacy_end]
                .iter()
                .all(|&b| (b'0'..=b'7').contains(&b));

        if is_legacy_octal {
            self.pos = legacy_end;
            // A legacy-octal spelling can never form a bigint token. Keep a
            // following `n` available as an identifier so the parser reports
            // TS1121 for the number and then performs ordinary recovery on
            // the suffix (`0123n` => `0123`, `n`).
            self.token_kind = TokenKind::NumericLiteral;
            return;
        }

        if self.pos < self.end && self.at(self.pos) == b'.' {
            self.pos += 1;
            self.scan_digits();
        }
        self.scan_exponent();
        self.finish_number();
    }

    fn scan_hex_integer(&mut self) {
        while self.pos < self.end {
            let ch = self.at(self.pos);
            if ch.is_ascii_hexdigit() || ch == b'_' {
                self.pos += 1;
            } else {
                break;
            }
        }
        self.finish_number();
    }

    fn scan_octal_integer(&mut self) {
        while self.pos < self.end {
            let ch = self.at(self.pos);
            if (b'0'..=b'7').contains(&ch) || ch == b'_' {
                self.pos += 1;
            } else {
                break;
            }
        }
        self.finish_number();
    }

    fn scan_binary_integer(&mut self) {
        while self.pos < self.end {
            let ch = self.at(self.pos);
            if ch == b'0' || ch == b'1' || ch == b'_' {
                self.pos += 1;
            } else {
                break;
            }
        }
        self.finish_number();
    }

    fn scan_digits(&mut self) {
        while self.pos < self.end {
            let ch = self.at(self.pos);
            if ch.is_ascii_digit() || ch == b'_' {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn scan_exponent(&mut self) {
        if self.pos < self.end && (self.at(self.pos) == b'e' || self.at(self.pos) == b'E') {
            self.pos += 1;
            if self.pos < self.end && (self.at(self.pos) == b'+' || self.at(self.pos) == b'-') {
                self.pos += 1;
            }
            self.scan_digits();
        }
    }

    fn scan_number_fragment_after_dot(&mut self) {
        self.scan_digits();
        self.scan_exponent();
    }

    fn finish_number(&mut self) {
        if self.pos < self.end && self.at(self.pos) == b'n' {
            self.pos += 1;
            self.token_kind = TokenKind::BigIntLiteral;
        } else {
            self.token_kind = TokenKind::NumericLiteral;
        }
        // Raw token text matches the value verbatim — no cook needed.
    }

    // ----- Identifier scanning -----

    /// Scan past an already-verified ASCII identifier start. Returns the end
    /// offset when the source spelling can be classified directly, or
    /// `usize::MAX` when an escape path has already decoded and classified the
    /// token.
    #[inline(never)]
    fn scan_identifier_tail(&mut self, mut pos: usize) -> usize {
        let token_start = pos;
        pos += 1; // skip first char (already verified as ident start)

        // Fast path: consume plain ASCII identifier bytes at a time. Unicode and
        // escapes deopt to the cold scalar helper below.
        #[cfg(target_arch = "x86_64")]
        while pos + 16 <= self.end {
            // SAFETY: the loop condition guarantees 16 readable bytes at `pos`.
            let first_break =
                unsafe { first_non_identifier_continue_16(self.source.as_ptr().add(pos)) };
            match first_break {
                None => pos += 16,
                Some(n) => {
                    pos += n as usize;
                    let b = self.at(pos);
                    if b < 0x80 && b != b'\\' {
                        return pos;
                    }
                    self.token_start = token_start;
                    self.pos = pos;
                    return if self.scan_identifier_tail_slow() {
                        self.pos
                    } else {
                        usize::MAX
                    };
                }
            }
        }

        #[cfg(not(target_arch = "x86_64"))]
        while pos + 8 <= self.end {
            // SAFETY: the loop condition guarantees 8 readable bytes at `pos`.
            // This drops both the slice bounds check and try_into's length check.
            let chunk: [u8; 8] = unsafe { *(self.source.as_ptr().add(pos) as *const [u8; 8]) };
            match first_non_identifier_continue(chunk) {
                None => pos += 8,
                Some(n) => {
                    pos += n as usize;
                    let b = self.at(pos);
                    if b < 0x80 && b != b'\\' && !is_identifier_continue(b) {
                        return pos;
                    }
                    self.token_start = token_start;
                    self.pos = pos;
                    return if self.scan_identifier_tail_slow() {
                        self.pos
                    } else {
                        usize::MAX
                    };
                }
            }
        }

        while pos < self.end {
            let b = self.at(pos);
            if is_identifier_continue(b) {
                pos += 1;
            } else if b == b'\\' || b > 127 {
                self.token_start = token_start;
                self.pos = pos;
                return if self.scan_identifier_tail_slow() {
                    self.pos
                } else {
                    usize::MAX
                };
            } else {
                break;
            }
        }
        pos
    }

    #[cold]
    fn scan_identifier_tail_slow(&mut self) -> bool {
        while self.pos < self.end {
            let b = self.at(self.pos);
            if is_identifier_continue(b) {
                self.pos += 1;
            } else if b == b'\\' && self.pos + 1 < self.end && self.at(self.pos + 1) == b'u' {
                // Unicode escape in identifier – delegate to full escape-aware scanner
                // Restart: build decoded value from what we have so far, then continue
                let decoded_so_far = self.text[self.token_start..self.pos].to_string();
                self.scan_identifier_escape_tail(decoded_so_far);
                return false;
            } else if b > 127 {
                let before = self.pos;
                let ch = self.decode_utf8_char();
                if is_unicode_identifier_continue(ch) {
                    // consumed by decode
                } else {
                    self.pos = before;
                    break;
                }
            } else {
                break;
            }
        }
        true
    }

    /// Scan an identifier that starts with a `\u` escape sequence.
    #[cold]
    #[inline(never)]
    fn scan_identifier_with_escapes(&mut self) {
        // pos is at `\`, next is `u`
        // Peek-first: save position, tentatively consume `\u` and try to decode.
        // If the escape is invalid or doesn't produce a valid identifier start,
        // restore position and emit just `\` as Unknown so the remaining chars
        // (e.g. `u003`, `uxxxx`) become separate tokens.
        let save_pos = self.pos;
        self.pos += 2; // tentatively skip `\u`
        if let Some(ch) = self.scan_unicode_escape() {
            if is_unicode_identifier_start(ch) || ch == '$' || ch == '_' {
                let mut decoded = String::new();
                decoded.push(ch);
                self.scan_identifier_escape_tail(decoded);
            } else {
                // Valid escape but not a valid identifier start (e.g. \u0031 = '1')
                // Don't consume the `\uXXXX`; just emit `\` as Unknown
                self.pos = save_pos + 1;
                self.token_kind = TokenKind::Unknown;
            }
        } else {
            // Invalid escape sequence – don't consume `\u`, just emit `\` as Unknown
            self.pos = save_pos + 1;
            self.token_kind = TokenKind::Unknown;
        }
    }

    /// Continue scanning an identifier that may contain `\u` escapes.
    /// `decoded` contains the already-decoded characters up to this point.
    #[cold]
    #[inline(never)]
    fn scan_identifier_escape_tail(&mut self, mut decoded: String) {
        loop {
            // Scan ASCII identifier continuation characters
            let ascii_start = self.pos;
            while self.pos < self.end && is_identifier_continue(self.at(self.pos)) {
                self.pos += 1;
            }
            if self.pos > ascii_start {
                decoded.push_str(&self.text[ascii_start..self.pos]);
            }

            if self.pos >= self.end {
                break;
            }

            let b = self.source[self.pos];
            if b == b'\\' && self.pos + 1 < self.end && self.at(self.pos + 1) == b'u' {
                // Peek-first: save position before consuming `\u` so we can
                // backtrack if the escape is invalid. This ensures the `\`
                // doesn't get swallowed into the current identifier token.
                let save_pos = self.pos;
                self.pos += 2; // tentatively skip `\u`
                if let Some(ch) = self.scan_unicode_escape() {
                    if is_unicode_identifier_continue(ch) || ch == '$' || ch == '_' {
                        decoded.push(ch);
                    } else {
                        // Valid escape but not valid identifier continue – restore
                        self.pos = save_pos;
                        break;
                    }
                } else {
                    // Invalid escape – restore position before `\u`
                    self.pos = save_pos;
                    break;
                }
            } else if b > 127 {
                let before = self.pos;
                let ch = self.decode_utf8_char();
                if is_unicode_identifier_continue(ch) {
                    decoded.push(ch);
                } else {
                    self.pos = before;
                    break;
                }
            } else {
                break;
            }
        }

        self.token_kind = TokenKind::keyword_from_str(&decoded);
        self.set_token_value_cooked(decoded);
    }

    #[cold]
    #[inline(never)]
    fn scan_unicode_identifier_or_unknown(&mut self) {
        let before = self.pos;
        let ch = self.decode_utf8_char();
        if is_unicode_identifier_start(ch) {
            // Scan rest of identifier
            while self.pos < self.end {
                if is_identifier_continue(self.source[self.pos]) {
                    self.pos += 1;
                } else if self.source[self.pos] > 127 {
                    let save = self.pos;
                    let c = self.decode_utf8_char();
                    if !is_unicode_identifier_continue(c) {
                        self.pos = save;
                        break;
                    }
                } else {
                    break;
                }
            }
            let ident_text = &self.text[self.token_start..self.pos];
            self.token_kind = TokenKind::keyword_from_str(ident_text);
            // Raw token text equals the identifier name — no cook needed.
        } else if ch == '\u{2028}' || ch == '\u{2029}' {
            self.preceding_line_break = true;
            self.token_kind = TokenKind::NewLineTrivia;
        } else if ch.is_whitespace() || ch == '\u{feff}' {
            self.token_kind = TokenKind::WhitespaceTrivia;
        } else {
            // Not an identifier start and not a line separator -- unknown token
            self.pos = before;
            self.skip_utf8_char();
            self.token_kind = TokenKind::Unknown;
        }
    }

    // ----- UTF-8 helpers -----

    fn skip_utf8_char(&mut self) {
        if self.pos >= self.end {
            return;
        }
        let b = self.source[self.pos];
        let len = if b < 0x80 {
            1
        } else if b < 0xE0 {
            2
        } else if b < 0xF0 {
            3
        } else {
            4
        };
        self.pos = (self.pos + len).min(self.end);
    }

    fn decode_utf8_char(&mut self) -> char {
        if self.pos >= self.end {
            return '\0';
        }
        // Use str slicing for correct UTF-8 decode
        let remaining = &self.text[self.pos..];
        if let Some(ch) = remaining.chars().next() {
            self.pos += ch.len_utf8();
            ch
        } else {
            self.pos += 1;
            '\u{FFFD}'
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ScanResult {
    pub(crate) pos: usize,
    pub(crate) kind: TokenKind,
}

#[inline(always)]
fn scan_result(scanner: &TsScanner<'_>, kind: TokenKind) -> ScanResult {
    ScanResult {
        pos: scanner.pos,
        kind,
    }
}

type ScanHandler = for<'a> fn(&mut TsScanner<'a>, u8, usize) -> ScanResult;

macro_rules! direct_scan_handler {
    ($name:ident, $kind:ident) => {
        fn $name(_scanner: &mut TsScanner<'_>, _ch: u8, pos: usize) -> ScanResult {
            ScanResult {
                pos: pos + 1,
                kind: TokenKind::$kind,
            }
        }
    };
}

direct_scan_handler!(scan_open_brace, OpenBrace);
direct_scan_handler!(scan_close_brace, CloseBrace);
direct_scan_handler!(scan_open_paren, OpenParen);
direct_scan_handler!(scan_close_paren, CloseParen);
direct_scan_handler!(scan_open_bracket, OpenBracket);
direct_scan_handler!(scan_close_bracket, CloseBracket);
direct_scan_handler!(scan_semicolon, Semicolon);
direct_scan_handler!(scan_comma, Comma);
direct_scan_handler!(scan_colon, Colon);
direct_scan_handler!(scan_tilde, Tilde);
direct_scan_handler!(scan_at, At);

fn scan_whitespace(scanner: &mut TsScanner<'_>, _ch: u8, pos: usize) -> ScanResult {
    let src = scanner.source;
    let end = scanner.end;
    let mut pos = pos + 1;
    // SAFETY: `pos < end == src.len()` is the loop guard.
    while pos < end && is_white_space(unsafe { *src.get_unchecked(pos) }) {
        pos += 1;
    }
    ScanResult {
        pos,
        kind: TokenKind::WhitespaceTrivia,
    }
}

fn scan_line_feed(scanner: &mut TsScanner<'_>, _ch: u8, pos: usize) -> ScanResult {
    let src = scanner.source;
    let end = scanner.end;
    let mut pos = pos + 1;
    // SAFETY: `pos < end == src.len()` is the loop guard.
    while pos < end && is_white_space(unsafe { *src.get_unchecked(pos) }) {
        pos += 1;
    }
    scanner.preceding_line_break = true;
    ScanResult {
        pos,
        kind: TokenKind::NewLineTrivia,
    }
}

fn scan_carriage_return(scanner: &mut TsScanner<'_>, _ch: u8, pos: usize) -> ScanResult {
    let src = scanner.source;
    let end = scanner.end;
    let mut pos = pos + 1;
    if pos < end && src[pos] == b'\n' {
        pos += 1;
    }
    // SAFETY: `pos < end == src.len()` is the loop guard.
    while pos < end && is_white_space(unsafe { *src.get_unchecked(pos) }) {
        pos += 1;
    }
    scanner.preceding_line_break = true;
    ScanResult {
        pos,
        kind: TokenKind::NewLineTrivia,
    }
}

fn scan_hash(scanner: &mut TsScanner<'_>, _ch: u8, pos: usize) -> ScanResult {
    scanner.pos = pos;
    if scanner.pos == 0 && scanner.pos + 1 < scanner.end && scanner.source[scanner.pos + 1] == b'!'
    {
        scanner.pos += 2;
        if scanner.pos < scanner.end {
            if let Some(rel) =
                memchr::memchr2(b'\n', b'\r', &scanner.source[scanner.pos..scanner.end])
            {
                scanner.pos += rel;
            } else {
                scanner.pos = scanner.end;
            }
        }
        return scan_result(scanner, TokenKind::WhitespaceTrivia);
    }
    if scanner.preceding_line_break
        && scanner.pos + 1 < scanner.end
        && scanner.source[scanner.pos + 1] == b'!'
    {
        scanner.pos += 1;
        return scan_result(scanner, TokenKind::WhitespaceTrivia);
    }
    scanner.pos += 1;
    scan_result(scanner, TokenKind::Hash)
}

fn scan_dot_handler(scanner: &mut TsScanner<'_>, _ch: u8, pos: usize) -> ScanResult {
    scanner.pos = pos;
    scanner.scan_dot();
    scan_result(scanner, scanner.token_kind)
}

fn scan_question_handler(scanner: &mut TsScanner<'_>, _ch: u8, pos: usize) -> ScanResult {
    scanner.pos = pos;
    scanner.scan_question();
    scan_result(scanner, scanner.token_kind)
}

fn scan_less_handler(scanner: &mut TsScanner<'_>, ch: u8, pos: usize) -> ScanResult {
    scanner.pos = pos;
    let kind = if scanner.is_conflict_marker_start(ch) {
        scanner.scan_conflict_marker_trivia();
        TokenKind::ConflictMarkerTrivia
    } else {
        scanner.scan_less_than();
        scanner.token_kind
    };
    scan_result(scanner, kind)
}

fn scan_greater_handler(scanner: &mut TsScanner<'_>, ch: u8, pos: usize) -> ScanResult {
    scanner.pos = pos;
    let kind = if scanner.is_conflict_marker_start(ch) {
        scanner.scan_conflict_marker_trivia();
        TokenKind::ConflictMarkerTrivia
    } else {
        scanner.scan_greater_than();
        scanner.token_kind
    };
    scan_result(scanner, kind)
}

fn scan_equals_handler(scanner: &mut TsScanner<'_>, ch: u8, pos: usize) -> ScanResult {
    scanner.pos = pos;
    let kind = if scanner.is_conflict_marker_start(ch) {
        scanner.scan_conflict_marker_trivia();
        TokenKind::ConflictMarkerTrivia
    } else {
        scanner.scan_equals();
        scanner.token_kind
    };
    scan_result(scanner, kind)
}

fn scan_exclamation_handler(scanner: &mut TsScanner<'_>, _ch: u8, pos: usize) -> ScanResult {
    scanner.pos = pos;
    scanner.scan_exclamation();
    scan_result(scanner, scanner.token_kind)
}

fn scan_plus_handler(scanner: &mut TsScanner<'_>, _ch: u8, pos: usize) -> ScanResult {
    scanner.pos = pos;
    scanner.scan_plus();
    scan_result(scanner, scanner.token_kind)
}

fn scan_minus_handler(scanner: &mut TsScanner<'_>, _ch: u8, pos: usize) -> ScanResult {
    scanner.pos = pos;
    scanner.scan_minus();
    scan_result(scanner, scanner.token_kind)
}

fn scan_asterisk_handler(scanner: &mut TsScanner<'_>, _ch: u8, pos: usize) -> ScanResult {
    scanner.pos = pos;
    scanner.scan_asterisk();
    scan_result(scanner, scanner.token_kind)
}

fn scan_slash_handler(scanner: &mut TsScanner<'_>, _ch: u8, pos: usize) -> ScanResult {
    scanner.pos = pos;
    let kind = if scanner.scan_slash_or_comment() {
        TokenKind::SingleLineCommentTrivia
    } else {
        scanner.token_kind
    };
    scan_result(scanner, kind)
}

fn scan_percent_handler(scanner: &mut TsScanner<'_>, _ch: u8, pos: usize) -> ScanResult {
    scanner.pos = pos;
    scanner.scan_percent();
    scan_result(scanner, scanner.token_kind)
}

fn scan_ampersand_handler(scanner: &mut TsScanner<'_>, _ch: u8, pos: usize) -> ScanResult {
    scanner.pos = pos;
    scanner.scan_ampersand();
    scan_result(scanner, scanner.token_kind)
}

fn scan_bar_handler(scanner: &mut TsScanner<'_>, ch: u8, pos: usize) -> ScanResult {
    scanner.pos = pos;
    let kind = if scanner.is_conflict_marker_start(ch) {
        scanner.scan_conflict_marker_trivia();
        TokenKind::ConflictMarkerTrivia
    } else {
        scanner.scan_bar();
        scanner.token_kind
    };
    scan_result(scanner, kind)
}

fn scan_caret_handler(scanner: &mut TsScanner<'_>, _ch: u8, pos: usize) -> ScanResult {
    scanner.pos = pos;
    scanner.scan_caret();
    scan_result(scanner, scanner.token_kind)
}

#[cold]
#[inline(never)]
fn scan_string_handler(scanner: &mut TsScanner<'_>, quote: u8, pos: usize) -> ScanResult {
    scanner.token_start = pos;
    scanner.pos = pos;
    scanner.scan_string(quote);
    scan_result(scanner, scanner.token_kind)
}

#[cold]
#[inline(never)]
fn scan_template_handler(scanner: &mut TsScanner<'_>, _ch: u8, pos: usize) -> ScanResult {
    scanner.token_start = pos;
    scanner.pos = pos;
    let kind = if scanner.pos > 0 && scanner.source[scanner.pos - 1] == b'\\' {
        scanner.pos += 1;
        TokenKind::Unknown
    } else {
        scanner.scan_template();
        scanner.token_kind
    };
    scan_result(scanner, kind)
}

fn scan_number_handler(scanner: &mut TsScanner<'_>, _ch: u8, pos: usize) -> ScanResult {
    scanner.pos = pos;
    scanner.scan_number();
    scan_result(scanner, scanner.token_kind)
}

fn scan_plain_identifier_handler(scanner: &mut TsScanner<'_>, _ch: u8, pos: usize) -> ScanResult {
    let pos = scanner.scan_identifier_tail(pos);
    if pos == usize::MAX {
        scan_result(scanner, scanner.token_kind)
    } else {
        ScanResult {
            pos,
            kind: TokenKind::Identifier,
        }
    }
}

macro_rules! keyword_scan_handler {
    ($name:ident { $($tail:literal => $kind:ident),+ $(,)? }) => {
        fn $name(scanner: &mut TsScanner<'_>, _ch: u8, token_start: usize) -> ScanResult {
            let pos = scanner.scan_identifier_tail(token_start);
            if pos == usize::MAX {
                return scan_result(scanner, scanner.token_kind);
            }
            let tail_bytes = &scanner.source[token_start + 1..pos];
            // SAFETY: `source` came from a valid `str`; the start is just after
            // the handler's ASCII initial byte, and identifier scanning always
            // stops on a UTF-8 character boundary. Slicing bytes first avoids
            // repeating those boundary checks in every keyword handler.
            let tail = unsafe { std::str::from_utf8_unchecked(tail_bytes) };
            let kind = match tail {
                $($tail => TokenKind::$kind,)+
                _ => TokenKind::Identifier,
            };
            ScanResult { pos, kind }
        }
    };
}

keyword_scan_handler!(scan_identifier_a {
    "wait" => Await,
    "sync" => Async,
    "bstract" => Abstract,
    "ccessor" => Accessor,
    "ny" => Any,
    "s" => As,
    "ssert" => Assert,
    "sserts" => Asserts,
});
keyword_scan_handler!(scan_identifier_b {
    "reak" => Break,
    "oolean" => Boolean,
    "igint" => BigInt,
});
keyword_scan_handler!(scan_identifier_c {
    "onst" => Const,
    "lass" => Class,
    "ontinue" => Continue,
    "atch" => Catch,
    "ase" => Case,
    "onstructor" => Constructor,
});
keyword_scan_handler!(scan_identifier_d {
    "o" => Do,
    "elete" => Delete,
    "eclare" => Declare,
    "efault" => Default,
    "ebugger" => Debugger,
});
keyword_scan_handler!(scan_identifier_e {
    "lse" => Else,
    "num" => Enum,
    "xport" => Export,
    "xtends" => Extends,
});
keyword_scan_handler!(scan_identifier_f {
    "unction" => Function,
    "alse" => False,
    "or" => For,
    "inally" => Finally,
    "rom" => From,
});
keyword_scan_handler!(scan_identifier_g {
    "et" => Get,
    "lobal" => Global,
});
keyword_scan_handler!(scan_identifier_i {
    "f" => If,
    "nstanceof" => InstanceOf,
    "n" => In,
    "mplements" => Implements,
    "mport" => Import,
    "nfer" => Infer,
    "nterface" => Interface,
    "ntrinsic" => Intrinsic,
    "s" => Is,
});
keyword_scan_handler!(scan_identifier_k { "eyof" => Keyof });
keyword_scan_handler!(scan_identifier_l { "et" => Let });
keyword_scan_handler!(scan_identifier_m { "odule" => Module });
keyword_scan_handler!(scan_identifier_n {
    "ull" => Null,
    "ew" => New,
    "umber" => Number,
    "amespace" => Namespace,
    "ever" => Never,
});
keyword_scan_handler!(scan_identifier_o {
    "f" => Of,
    "bject" => Object,
    "ut" => Out,
    "verride" => Override,
});
keyword_scan_handler!(scan_identifier_p {
    "ackage" => Package,
    "rivate" => Private,
    "rotected" => Protected,
    "ublic" => Public,
});
keyword_scan_handler!(scan_identifier_r {
    "eturn" => Return,
    "equire" => Require,
    "eadonly" => Readonly,
});
keyword_scan_handler!(scan_identifier_s {
    "et" => Set,
    "uper" => Super,
    "witch" => Switch,
    "tatic" => Static,
    "ymbol" => Symbol,
    "tring" => String,
    "atisfies" => Satisfies,
});
keyword_scan_handler!(scan_identifier_t {
    "his" => This,
    "rue" => True,
    "hrow" => Throw,
    "ry" => Try,
    "ypeof" => TypeOf,
    "ype" => Type,
});
keyword_scan_handler!(scan_identifier_u {
    "ndefined" => Undefined,
    "sing" => Using,
    "nique" => Unique,
    "nknown" => UnknownKeyword,
});
keyword_scan_handler!(scan_identifier_v {
    "ar" => Var,
    "oid" => Void,
});
keyword_scan_handler!(scan_identifier_w {
    "hile" => While,
    "ith" => With,
});
keyword_scan_handler!(scan_identifier_y { "ield" => Yield });

#[cold]
#[inline(never)]
fn scan_backslash_handler(scanner: &mut TsScanner<'_>, _ch: u8, pos: usize) -> ScanResult {
    scanner.token_start = pos;
    scanner.pos = pos;
    let kind = if scanner.pos + 1 < scanner.end && scanner.at(scanner.pos + 1) == b'u' {
        scanner.scan_identifier_with_escapes();
        scanner.token_kind
    } else {
        scanner.pos += 1;
        TokenKind::Unknown
    };
    scan_result(scanner, kind)
}

#[cold]
#[inline(never)]
fn scan_unicode_handler(scanner: &mut TsScanner<'_>, _ch: u8, pos: usize) -> ScanResult {
    scanner.token_start = pos;
    scanner.pos = pos;
    scanner.scan_unicode_identifier_or_unknown();
    scan_result(scanner, scanner.token_kind)
}

#[cold]
#[inline(never)]
fn scan_e2_handler(scanner: &mut TsScanner<'_>, _ch: u8, pos: usize) -> ScanResult {
    scanner.token_start = pos;
    scanner.pos = pos;
    let kind = if scanner.pos + 2 < scanner.end
        && scanner.source[scanner.pos + 1] == 0x80
        && (scanner.source[scanner.pos + 2] == 0xA8 || scanner.source[scanner.pos + 2] == 0xA9)
    {
        scanner.pos += 3;
        scanner.preceding_line_break = true;
        TokenKind::NewLineTrivia
    } else {
        return scan_unicode_handler(scanner, 0xE2, pos);
    };
    scan_result(scanner, kind)
}

#[cold]
#[inline(never)]
fn scan_unknown_handler(scanner: &mut TsScanner<'_>, ch: u8, pos: usize) -> ScanResult {
    let pos = if ch >= 0x80 {
        scanner.pos = pos;
        scanner.skip_utf8_char();
        scanner.pos
    } else {
        pos + 1
    };
    ScanResult {
        pos,
        kind: TokenKind::Unknown,
    }
}

const fn make_scan_handlers() -> [ScanHandler; 256] {
    let mut handlers = [scan_unknown_handler as ScanHandler; 256];

    handlers[b' ' as usize] = scan_whitespace;
    handlers[b'\t' as usize] = scan_whitespace;
    handlers[0x0B] = scan_whitespace;
    handlers[0x0C] = scan_whitespace;
    handlers[b'\n' as usize] = scan_line_feed;
    handlers[b'\r' as usize] = scan_carriage_return;
    handlers[b'#' as usize] = scan_hash;

    handlers[b'{' as usize] = scan_open_brace;
    handlers[b'}' as usize] = scan_close_brace;
    handlers[b'(' as usize] = scan_open_paren;
    handlers[b')' as usize] = scan_close_paren;
    handlers[b'[' as usize] = scan_open_bracket;
    handlers[b']' as usize] = scan_close_bracket;
    handlers[b';' as usize] = scan_semicolon;
    handlers[b',' as usize] = scan_comma;
    handlers[b':' as usize] = scan_colon;
    handlers[b'~' as usize] = scan_tilde;
    handlers[b'@' as usize] = scan_at;

    handlers[b'.' as usize] = scan_dot_handler;
    handlers[b'?' as usize] = scan_question_handler;
    handlers[b'<' as usize] = scan_less_handler;
    handlers[b'>' as usize] = scan_greater_handler;
    handlers[b'=' as usize] = scan_equals_handler;
    handlers[b'!' as usize] = scan_exclamation_handler;
    handlers[b'+' as usize] = scan_plus_handler;
    handlers[b'-' as usize] = scan_minus_handler;
    handlers[b'*' as usize] = scan_asterisk_handler;
    handlers[b'/' as usize] = scan_slash_handler;
    handlers[b'%' as usize] = scan_percent_handler;
    handlers[b'&' as usize] = scan_ampersand_handler;
    handlers[b'|' as usize] = scan_bar_handler;
    handlers[b'^' as usize] = scan_caret_handler;
    handlers[b'\'' as usize] = scan_string_handler;
    handlers[b'"' as usize] = scan_string_handler;
    handlers[b'`' as usize] = scan_template_handler;
    handlers[b'\\' as usize] = scan_backslash_handler;

    let mut byte = b'0';
    while byte <= b'9' {
        handlers[byte as usize] = scan_number_handler;
        byte += 1;
    }
    byte = b'A';
    while byte <= b'Z' {
        handlers[byte as usize] = scan_plain_identifier_handler;
        byte += 1;
    }
    byte = b'a';
    while byte <= b'z' {
        handlers[byte as usize] = scan_plain_identifier_handler;
        byte += 1;
    }
    handlers[b'$' as usize] = scan_plain_identifier_handler;
    handlers[b'_' as usize] = scan_plain_identifier_handler;

    handlers[b'a' as usize] = scan_identifier_a;
    handlers[b'b' as usize] = scan_identifier_b;
    handlers[b'c' as usize] = scan_identifier_c;
    handlers[b'd' as usize] = scan_identifier_d;
    handlers[b'e' as usize] = scan_identifier_e;
    handlers[b'f' as usize] = scan_identifier_f;
    handlers[b'g' as usize] = scan_identifier_g;
    handlers[b'i' as usize] = scan_identifier_i;
    handlers[b'k' as usize] = scan_identifier_k;
    handlers[b'l' as usize] = scan_identifier_l;
    handlers[b'm' as usize] = scan_identifier_m;
    handlers[b'n' as usize] = scan_identifier_n;
    handlers[b'o' as usize] = scan_identifier_o;
    handlers[b'p' as usize] = scan_identifier_p;
    handlers[b'r' as usize] = scan_identifier_r;
    handlers[b's' as usize] = scan_identifier_s;
    handlers[b't' as usize] = scan_identifier_t;
    handlers[b'u' as usize] = scan_identifier_u;
    handlers[b'v' as usize] = scan_identifier_v;
    handlers[b'w' as usize] = scan_identifier_w;
    handlers[b'y' as usize] = scan_identifier_y;

    let mut non_ascii = 0x80;
    while non_ascii <= 0xFF {
        handlers[non_ascii] = scan_unicode_handler;
        non_ascii += 1;
    }
    handlers[0xE2] = scan_e2_handler;
    handlers
}

static SCAN_HANDLERS: [ScanHandler; 256] = make_scan_handlers();

fn hex_digit_value(ch: u8) -> Option<u8> {
    match ch {
        b'0'..=b'9' => Some(ch - b'0'),
        b'a'..=b'f' => Some(ch - b'a' + 10),
        b'A'..=b'F' => Some(ch - b'A' + 10),
        _ => None,
    }
}
