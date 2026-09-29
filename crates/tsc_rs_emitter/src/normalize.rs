//! Source text post-processing normalization functions.

use std::borrow::Cow;

/// Run the standard six-stage normalizer chain used by source-copy emit
/// paths in `emit_source_line`. Consolidating these into one helper makes
/// future structural refactors of the chain (combined state machine,
/// shared scratch buffers) localized to a single function instead of
/// touching ~5 call sites.
///
/// Stages (in order): `brace_spacing` → `keyword_paren` →
/// `import_call_spacing` → `unary_spacing` → `close_paren` →
/// `comment_word_space`.
///
/// Returns `Cow::Borrowed(text)` when every stage produced output equal to
/// its input (saves one heap alloc on the all-no-op path); otherwise
/// `Cow::Owned` of the final result.
pub(crate) fn normalize_emit_chain(text: &str) -> Cow<'_, str> {
    let s1 = normalize_brace_spacing(text);
    let s2 = normalize_keyword_paren(&s1);
    let s3 = normalize_import_call_spacing(&s2);
    let s4 = normalize_unary_spacing(&s3);
    let s5 = normalize_close_paren(&s4);
    let s6 = normalize_comment_word_space(&s5);
    if s6.as_ref() == text {
        Cow::Borrowed(text)
    } else {
        Cow::Owned(s6.into_owned())
    }
}

/// Single-pass equivalent of [`normalize_emit_chain`]. Brace-spacing
/// remains its own first stage (1600-line state machine, deferred); the
/// other five chain stages (keyword_paren, import_call_spacing,
/// unary_spacing, close_paren, comment_word_space) collapse into one
/// `normalize_unified_pass` walk. Six passes → two.
pub(crate) fn normalize_emit_chain_v2(text: &str) -> Cow<'_, str> {
    let s1 = normalize_brace_spacing(text);
    let s2 = normalize_unified_pass(&s1);
    if s2.as_ref() == text {
        Cow::Borrowed(text)
    } else {
        Cow::Owned(s2.into_owned())
    }
}

/// One pass over `text`. Applies the rules of (in original chain order):
///   - `normalize_keyword_paren`     (insert space before `(` after keywords)
///   - `normalize_import_call_spacing` (collapse `import +(` → `import(`)
///   - `normalize_unary_spacing`     (drop space after unary `~`/`!`/`-`/`+`)
///   - `normalize_close_paren`       (strip space before `)`/`]`/`;`/`,`,
///                                    strip trailing comma before `)`,
///                                    strip space after `[`/`(`,
///                                    JSX `/>` spacing)
///   - `normalize_comment_word_space` (insert space after `*/` before words)
///
/// Order within a single byte mirrors the original chain: rules that edit
/// the output tail (`)`/`]`/`;`/`,` strip) run *before* the byte is
/// pushed; the `[`/`(` "skip-following-spaces" rule runs *after*.
pub(crate) fn normalize_unified_pass(text: &str) -> Cow<'_, str> {
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut out: Vec<u8> = Vec::with_capacity(len + 16);
    let mut state = LineState::new();
    let mut i = 0;

    static KEYWORD_PAREN_KEYWORDS: &[&[u8]] = &[
        b"function*",
        b"function",
        b"if",
        b"for",
        b"while",
        b"switch",
        b"catch",
        b"with",
        b"void",
        b"typeof",
        b"new",
    ];
    let import_pat = b"import ";

    while i < len {
        let ch = bytes[i];
        let was_in_block = state.in_block_comment;

        // Single in-code dispatch: only one of these branches fires per byte.
        // Hoists `state.in_code()` out of the per-rule predicates.
        if state.in_code() {
            match ch {
                // close_paren regex-start + JSX `/>` (must run first since it
                // manually flips `state.in_regex`).
                b'/' if i + 1 < len && bytes[i + 1] != b'/' && bytes[i + 1] != b'*' => {
                    let prev_non_ws = out.iter().rposition(|&b| b != b' ' && b != b'\t');
                    let (prev_byte, prev_word): (Option<u8>, &[u8]) = match prev_non_ws {
                        None => (None, b""),
                        Some(p) => {
                            let pc = out[p];
                            if pc.is_ascii_alphanumeric() || pc == b'_' || pc == b'$' {
                                let word_start = out[..=p]
                                    .iter()
                                    .rposition(|b| {
                                        !b.is_ascii_alphanumeric() && *b != b'_' && *b != b'$'
                                    })
                                    .map(|pos| pos + 1)
                                    .unwrap_or(0);
                                (Some(pc), &out[word_start..=p])
                            } else {
                                (Some(pc), b"")
                            }
                        }
                    };
                    if LineState::regex_start_after(prev_byte, prev_word) {
                        out.push(b'/');
                        state.in_regex = true;
                        state.in_regex_class = false;
                        i += 1;
                        continue;
                    }
                    // JSX self-closing `/>`: spaces handling.
                    if bytes[i + 1] == b'>' && !out.is_empty() {
                        let mut count = 0;
                        for j in (0..out.len()).rev() {
                            if out[j] == b' ' {
                                count += 1;
                            } else {
                                break;
                            }
                        }
                        let before = out.len() - count;
                        if before > 0 && out[before - 1] != b'\n' && out[before - 1] != b'\r' {
                            let prev = out[before - 1];
                            if prev.is_ascii_alphanumeric()
                                || prev == b'-'
                                || prev == b':'
                                || prev == b'.'
                            {
                                let mut scan = before - 1;
                                while scan > 0
                                    && (out[scan - 1].is_ascii_alphanumeric()
                                        || out[scan - 1] == b'-'
                                        || out[scan - 1] == b':'
                                        || out[scan - 1] == b'.'
                                        || out[scan - 1] == b'_')
                                {
                                    scan -= 1;
                                }
                                let is_tag_name = scan > 0 && out[scan - 1] == b'<';
                                if is_tag_name && count == 0 {
                                    out.push(b' ');
                                }
                            } else if count > 0 {
                                out.truncate(before);
                            }
                        }
                    }
                }
                // import_call_spacing: collapse `import +(` → `import(`.
                b'i' if i + import_pat.len() < len
                    && &bytes[i..i + import_pat.len()] == import_pat
                    && (i == 0
                        || !(bytes[i - 1].is_ascii_alphanumeric()
                            || bytes[i - 1] == b'_'
                            || bytes[i - 1] == b'.')) =>
                {
                    let mut j = i + import_pat.len();
                    while j < len && bytes[j] == b' ' {
                        j += 1;
                    }
                    if j < len && bytes[j] == b'(' && j > i + import_pat.len() - 1 {
                        out.extend_from_slice(b"import");
                        let mut k = i;
                        while k < j {
                            state.advance(bytes, k);
                            k += 1;
                        }
                        i = j;
                        continue;
                    }
                }
                // unary_spacing: drop space after unary `~`/`!`/`-`/`+`.
                b'~' | b'!' | b'-' | b'+' if i + 1 < len && bytes[i + 1] == b' ' => {
                    let is_ambiguous_unary = ch == b'-' || ch == b'+';
                    let prev = {
                        let mut p = out.len();
                        while p > 0 && out[p - 1] == b' ' {
                            p -= 1;
                        }
                        if p == 0 {
                            0
                        } else {
                            out[p - 1]
                        }
                    };
                    if !(is_ambiguous_unary && (prev == b'+' || prev == b'-')) {
                        let is_unary_context = matches!(
                            prev,
                            0 | b'('
                                | b','
                                | b'='
                                | b'['
                                | b':'
                                | b';'
                                | b'{'
                                | b'!'
                                | b'~'
                                | b'?'
                                | b'|'
                                | b'&'
                                | b'^'
                                | b'*'
                                | b'/'
                                | b'%'
                                | b'<'
                                | b'>'
                        );
                        if is_unary_context {
                            let collapse = if is_ambiguous_unary {
                                let mut j = i + 2;
                                while j < len && bytes[j] == b' ' {
                                    j += 1;
                                }
                                !(j < len && (bytes[j] == b'+' || bytes[j] == b'-'))
                            } else {
                                true
                            };
                            if collapse {
                                out.push(ch);
                                state.advance(bytes, i);
                                state.advance(bytes, i + 1);
                                i += 2;
                                continue;
                            }
                        }
                    }
                }
                // keyword_paren: insert space before `(` after keyword.
                b'(' if !out.is_empty() && out[out.len() - 1] != b' ' => {
                    for kw in KEYWORD_PAREN_KEYWORDS {
                        let klen = kw.len();
                        if out.len() >= klen && &out[out.len() - klen..] == *kw {
                            let at_boundary = out.len() == klen || {
                                let prev = out[out.len() - klen - 1];
                                !prev.is_ascii_alphanumeric() && prev != b'_' && prev != b'.'
                            };
                            if at_boundary {
                                out.push(b' ');
                                break;
                            }
                        }
                    }
                }
                // close_paren strip-trailing-space-before `)` / `]`.
                b')' | b']' if !out.is_empty() => {
                    let mut count = 0;
                    for j in (0..out.len()).rev() {
                        if out[j] == b' ' {
                            count += 1;
                        } else {
                            break;
                        }
                    }
                    if count > 0 {
                        let before = out.len() - count;
                        let is_comma_expr_space =
                            ch == b')' && before > 0 && out[before - 1] == b',';
                        // A block comment that OWNS its line keeps the space
                        // before a closing bracket (`/*3*/ ]`) — it is emitted
                        // as a leading comment. A comment trailing real
                        // expression text on the same line collapses
                        // (`"toString" /*3*/]`).
                        let after_block_comment = ch == b']'
                            && before >= 2
                            && out[before - 1] == b'/'
                            && out[before - 2] == b'*'
                            && {
                                let line_start = out[..before]
                                    .iter()
                                    .rposition(|&b| b == b'\n')
                                    .map(|i| i + 1)
                                    .unwrap_or(0);
                                let line = &out[line_start..before];
                                let trimmed: Vec<u8> = line
                                    .iter()
                                    .copied()
                                    .skip_while(|b| *b == b' ' || *b == b'\t')
                                    .collect();
                                trimmed.starts_with(b"/*")
                            };
                        if before > 0
                            && out[before - 1] != b'\n'
                            && out[before - 1] != b'\r'
                            && !is_comma_expr_space
                            && !after_block_comment
                        {
                            out.truncate(before);
                        }
                    }
                    if ch == b')' && !out.is_empty() && out[out.len() - 1] == b',' {
                        let orig_had_space = i > 0 && bytes[i - 1] == b' ';
                        if !orig_had_space {
                            out.pop();
                        }
                    }
                }
                // close_paren strip-trailing-space-before `;`.
                b';' if !out.is_empty() => {
                    let mut count = 0;
                    for j in (0..out.len()).rev() {
                        if out[j] == b' ' {
                            count += 1;
                        } else {
                            break;
                        }
                    }
                    if count > 0 {
                        let before = out.len() - count;
                        let preserve = if before > 0 {
                            let prev_byte = out[before - 1];
                            if prev_byte == b'>' || prev_byte == b'\n' || prev_byte == b'\r' {
                                true
                            } else if prev_byte.is_ascii_alphabetic() {
                                let word_start = out[..before]
                                    .iter()
                                    .rposition(|b| !b.is_ascii_alphabetic())
                                    .map(|p| p + 1)
                                    .unwrap_or(0);
                                let word = &out[word_start..before];
                                // Keyword preserve list mirrors
                                // `normalize_trailing_semi::should_collapse`
                                // (the chain pass that previously ran
                                // before close_paren). `yield` is *not*
                                // here despite being a unary keyword:
                                // TypeScript collapses `yield ;` → `yield;`
                                // in source-copy emit, so the unified
                                // pass must too. Was the cause of
                                // generatorTypeCheck62 regressing during
                                // the unified-pass adoption — see commit
                                // history.
                                word == b"void"
                                    || word == b"delete"
                                    || word == b"typeof"
                                    || word == b"return"
                                    || word == b"throw"
                                    || word == b"await"
                                    || word == b"using"
                            } else if matches!(
                                prev_byte,
                                b'+' | b'-'
                                    | b'*'
                                    | b'/'
                                    | b'%'
                                    | b'^'
                                    | b'&'
                                    | b'|'
                                    | b'~'
                                    | b'='
                                    | b'!'
                                    | b'?'
                                    | b','
                            ) {
                                if (prev_byte == b'+' || prev_byte == b'-')
                                    && before >= 2
                                    && out[before - 2] == prev_byte
                                {
                                    false
                                } else if (prev_byte == b'/' || prev_byte == b'*')
                                    && before >= 2
                                    && out[before - 2] == b'*'
                                {
                                    false
                                } else {
                                    true
                                }
                            } else {
                                false
                            }
                        } else {
                            false
                        };
                        if !preserve {
                            out.truncate(before);
                        }
                    }
                }
                // close_paren strip-trailing-space-before `,`.
                b',' if !out.is_empty() => {
                    let mut count = 0;
                    for j in (0..out.len()).rev() {
                        if out[j] == b' ' {
                            count += 1;
                        } else {
                            break;
                        }
                    }
                    if count > 0 {
                        let before = out.len() - count;
                        if before > 0 {
                            let prev = out[before - 1];
                            if prev.is_ascii_alphanumeric()
                                || prev == b'_'
                                || prev == b'$'
                                || prev == b')'
                                || prev == b']'
                                || prev == b'"'
                                || prev == b'\''
                                || prev == b'`'
                                || prev == b'}'
                            {
                                out.truncate(before);
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        let was_open_bracket_in_code = state.in_code() && (ch == b'[' || ch == b'(');
        let advance = state.advance(bytes, i);
        out.extend_from_slice(&bytes[i..i + advance]);
        i += advance;

        // -- comment_word_space: on `*/` exit, maybe add space, manipulate
        //    following whitespace. -------------------------------------
        if was_in_block && !state.in_block_comment && i < len {
            let rlen = out.len();
            let mut cs = rlen.saturating_sub(2);
            while cs > 0 {
                if out[cs] == b'/' && cs + 1 < rlen && out[cs + 1] == b'*' {
                    break;
                }
                cs -= 1;
            }
            let mut before_idx = if cs > 0 { cs - 1 } else { 0 };
            while before_idx > 0 && out[before_idx] == b' ' {
                before_idx -= 1;
            }
            let dot_before = cs > 0 && out[before_idx] == b'.';
            let paren_before = cs > 0 && out[before_idx] == b'(' && {
                if before_idx == 0 {
                    true
                } else {
                    let pre_paren = out[before_idx - 1];
                    !pre_paren.is_ascii_alphanumeric()
                        && pre_paren != b'_'
                        && pre_paren != b'$'
                        && pre_paren != b')'
                        && pre_paren != b']'
                }
            };

            let after = bytes[i];
            if after.is_ascii_alphanumeric() || after == b'_' || after == b'$' {
                if !dot_before && !paren_before {
                    out.push(b' ');
                }
            } else if after == b'(' && !dot_before {
                if !paren_before {
                    out.push(b' ');
                }
                if !paren_before && i + 2 < len && bytes[i + 1] == b'/' && bytes[i + 2] == b'*' {
                    out.push(b'(');
                    out.push(b' ');
                    state.advance(bytes, i);
                    i += 1;
                }
            } else if after == b' ' {
                let bracket_before = cs > 0 && out[before_idx] == b'[';
                let start = i;
                while i < len && bytes[i] == b' ' {
                    state.advance(bytes, i);
                    i += 1;
                }
                if i < len {
                    let next = bytes[i];
                    if (next == b'"' || next == b'\'') && (bracket_before || paren_before) {
                        // strip
                    } else if paren_before
                        && (next.is_ascii_alphanumeric()
                            || next == b'_'
                            || next == b'$'
                            || next == b'(')
                    {
                        // strip
                    } else {
                        out.extend_from_slice(&bytes[start..i]);
                    }
                } else {
                    out.extend_from_slice(&bytes[start..i]);
                }
            }
        }

        // -- close_paren post-rule: skip spaces after `[`/`(`. ------------
        if was_open_bracket_in_code {
            let start = i;
            while i < len && bytes[i] == b' ' {
                state.advance(bytes, i);
                i += 1;
            }
            let should_keep = i >= len
                || bytes[i] == b'\n'
                || bytes[i] == b'\r'
                || (bytes[i] == b'/' && i + 1 < len && bytes[i + 1] == b'*');
            if should_keep {
                out.extend_from_slice(&bytes[start..i]);
            }
        }
    }

    if out.len() == text.len() && out == text.as_bytes() {
        return Cow::Borrowed(text);
    }
    Cow::Owned(unsafe { String::from_utf8_unchecked(out) })
}

/// Lexical-context state shared by the source-copy normalizers in this
/// module. Tracks string/comment/regex/template/paren state so a unified
/// single-pass state machine can be built without each normalizer
/// duplicating its own bookkeeping.
///
/// Spacing-specific ternary and for-header stacks belong to
/// `normalize_brace_spacing`; shared lexical state does not maintain them.
///
/// Field semantics mirror the inline state in the existing per-normalizer
/// loops (`normalize_brace_spacing`, `normalize_close_paren`, etc.) — the
/// extraction is purely mechanical so that porting one normalizer at a
/// time keeps the corpus parity test green.
#[derive(Debug, Clone, Default)]
pub(crate) struct LineState {
    /// `0` = not in a string; `b'\''` / `b'"'` / `b'`'` while inside one.
    pub in_string: u8,
    pub in_line_comment: bool,
    pub in_block_comment: bool,
    /// `true` while inside a `/.../flags` regex literal.
    pub in_regex: bool,
    /// `true` while inside a `[...]` character class within a regex.
    pub in_regex_class: bool,
    /// Depth of `${ ... }` interpolations inside template literals. When
    /// nonzero, the parser is inside a template-expression and ordinary
    /// JS rules (brace spacing, etc.) apply again.
    pub template_depth: u32,
    /// Per `{` brace, whether it was a `${` template-expression open.
    /// Used by brace_spacing to know which `}` closes a template
    /// expression vs a normal block.
    pub template_brace_stack: Vec<bool>,
    /// Plain JS paren depth (inside code, not inside string/regex/etc.).
    pub paren_depth: u32,
}

impl LineState {
    pub fn new() -> Self {
        Self::default()
    }

    /// `true` when the byte at the cursor is part of normal JS code (not
    /// inside a string, comment, or regex). Ternary/template *content*
    /// counts as code; string interiors and comment bodies do not.
    #[inline]
    pub fn in_code(&self) -> bool {
        self.in_string == 0 && !self.in_line_comment && !self.in_block_comment && !self.in_regex
    }

    /// Advance state given the byte at `bytes[i]`. The caller is
    /// responsible for incrementing `i` (some transitions consume
    /// multiple input bytes — e.g., entering a `//` line comment skips
    /// the second `/`); this method returns the number of input bytes
    /// the transition consumed (always `>= 1`).
    ///
    /// `bytes` is the full input slice so the method can peek ahead for
    /// 2-byte sequences (`/*`, `*/`, `${`, `\\<x>`).
    ///
    /// Regex-start detection requires looking at the bytes *preceding*
    /// the `/` to disambiguate division from a regex literal. Callers
    /// that emit transformed output cannot just reuse `bytes[..i]`
    /// because their output may differ from input. They pass an explicit
    /// `prev_non_ws_byte` (or `None` if at start) plus a function for
    /// reading the lookback word, via [`LineState::regex_start_after`].
    pub fn advance(&mut self, bytes: &[u8], i: usize) -> usize {
        debug_assert!(i < bytes.len());
        let ch = bytes[i];

        if self.in_line_comment {
            if ch == b'\n' {
                self.in_line_comment = false;
            }
            return 1;
        }
        if self.in_block_comment {
            if ch == b'*' && bytes.get(i + 1) == Some(&b'/') {
                self.in_block_comment = false;
                return 2;
            }
            return 1;
        }
        if self.in_regex {
            if ch == b'\\' && i + 1 < bytes.len() {
                return 2;
            }
            if ch == b'[' && !self.in_regex_class {
                self.in_regex_class = true;
                return 1;
            }
            if ch == b']' && self.in_regex_class {
                self.in_regex_class = false;
                return 1;
            }
            if ch == b'/' && !self.in_regex_class {
                self.in_regex = false;
                self.in_regex_class = false;
                return 1;
            }
            // JS regex literals can't contain unescaped newlines — exit
            // regex mode on a line break (matches per-normalizer
            // recovery behaviour in `normalize_brace_spacing`).
            if ch == b'\n' || ch == b'\r' {
                self.in_regex = false;
                self.in_regex_class = false;
                return 1;
            }
            return 1;
        }
        if self.in_string != 0 {
            // `${` inside a backtick template enters a JS expression
            // context — leave string mode until matching `}`.
            if self.in_string == b'`' && ch == b'$' && bytes.get(i + 1) == Some(&b'{') {
                self.template_depth = self.template_depth.saturating_add(1);
                self.in_string = 0;
                self.template_brace_stack.push(true);
                return 2;
            }
            if closes_string(bytes, i, self.in_string) {
                self.in_string = 0;
            }
            return 1;
        }
        // Code context.
        if ch == b'\'' || ch == b'"' || ch == b'`' {
            self.in_string = ch;
            return 1;
        }
        if ch == b'/' && i + 1 < bytes.len() {
            let next = bytes[i + 1];
            if next == b'/' {
                self.in_line_comment = true;
                return 2;
            }
            if next == b'*' {
                self.in_block_comment = true;
                return 2;
            }
            // Regex-start detection is left to the caller via
            // `regex_start_after`; this method advances state assuming
            // the `/` is division. Callers that detect regex-start set
            // `in_regex` themselves before consuming the byte.
        }
        if ch == b'(' {
            self.paren_depth = self.paren_depth.saturating_add(1);
        }
        if ch == b')' {
            self.paren_depth = self.paren_depth.saturating_sub(1);
        }
        if ch == b'{' {
            self.template_brace_stack.push(false);
        }
        if ch == b'}' {
            if let Some(was_template_expr) = self.template_brace_stack.pop() {
                if was_template_expr {
                    self.template_depth = self.template_depth.saturating_sub(1);
                    self.in_string = b'`';
                }
            }
        }
        1
    }

    /// Heuristic reproduction of the regex-vs-division disambiguation used
    /// by `normalize_close_paren` and `normalize_brace_spacing`. The `/` is
    /// treated as a regex opener when the previous non-whitespace byte is
    /// missing (start of input), or is a punctuation/operator that can be
    /// followed by a regex, or is a keyword like `return`/`typeof`/etc.
    /// `prev_word` should be the word ending at the previous non-whitespace
    /// position (empty if not an alphanumeric word).
    pub fn regex_start_after(prev_non_ws: Option<u8>, prev_word: &[u8]) -> bool {
        match prev_non_ws {
            None => true,
            Some(pc) => {
                if pc.is_ascii_alphanumeric() || pc == b'_' || pc == b'$' {
                    matches!(
                        prev_word,
                        b"return"
                            | b"typeof"
                            | b"void"
                            | b"delete"
                            | b"throw"
                            | b"case"
                            | b"new"
                            | b"in"
                            | b"instanceof"
                            | b"yield"
                            | b"await"
                    )
                } else {
                    pc != b')'
                        && pc != b']'
                        && pc != b'}'
                        && pc != b'>'
                        && pc != b'"'
                        && pc != b'\''
                        && pc != b'`'
                }
            }
        }
    }
}

/// Decide whether the byte at `i` (a candidate closing quote that matches
/// the open quote `q`) actually closes the string literal. The naive
/// `bytes[i-1] != b'\\'` check is wrong for `"\\\\"` (4 backslashes
/// represent two literal backslashes followed by an unescaped `"`): the
/// preceding `\` is itself the second half of an escape pair. Counting
/// consecutive backslashes and checking parity is correct.
#[inline]
fn closes_string(bytes: &[u8], i: usize, q: u8) -> bool {
    if bytes[i] != q {
        return false;
    }
    let mut bs = 0usize;
    let mut j = i;
    while j > 0 && bytes[j - 1] == b'\\' {
        bs += 1;
        j -= 1;
    }
    bs % 2 == 0
}

/// Check if the identifier ending at position `end` (inclusive) in `bytes`
/// is a keyword after which `+`/`-` should be treated as unary, not binary.
/// For example: `return -1` (unary), `throw +x` (unary), `case -1:` (unary).
fn preceding_word_is_unary_keyword(bytes: &[u8], end: usize) -> bool {
    // Walk back from `end` to find the start of the word.
    let mut start = end;
    while start > 0 && bytes[start - 1].is_ascii_alphanumeric() {
        start -= 1;
    }
    // The character before the word (if any) must not be alphanumeric or `_`
    // (otherwise it's part of a larger identifier like `myreturn`).
    if start > 0
        && (bytes[start - 1].is_ascii_alphanumeric()
            || bytes[start - 1] == b'_'
            || bytes[start - 1] == b'$')
    {
        return false;
    }
    let word = &bytes[start..=end];
    matches!(
        word,
        b"return"
            | b"throw"
            | b"case"
            | b"typeof"
            | b"void"
            | b"yield"
            | b"await"
            | b"delete"
            | b"in"
            | b"new"
            | b"export"
            | b"default"
    )
}

/// Normalize spacing inside single-line object literal braces.
/// Converts `{a: v}` to `{ a: v }` while avoiding changes to:
/// - Template literal interpolations `${expr}`
/// - Empty braces `{}`
/// - Multi-line blocks (where `{` is followed by newline)
pub(crate) fn normalize_brace_spacing(text: &str) -> Cow<'_, str> {
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut result = Vec::with_capacity(len + 32);
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut in_string: u8 = 0; // 0 = none, b'\'' or b'"' or b'`'
                               // Track pending ternary `?` count at each nesting level of `(`, `[`, `{`.
                               // Used to distinguish property/label colons (remove space before) from
                               // ternary colons (keep space before).
    let mut ternary_stack: Vec<u32> = vec![0];
    // Track whether each `{` is a template expression `${` (true) or regular brace (false).
    let mut template_brace_stack: Vec<bool> = Vec::new();
    // Track template literal nesting depth for `${...}` handling.
    // When > 0, we're inside a template expression and should normalize code.
    let mut template_depth: u32 = 0;
    // Track regex literal context — skip normalization inside regex.
    let mut in_regex = false;
    let mut in_regex_class = false; // inside `[...]` character class
                                    // Track for-loop parentheses: stack of paren depths where a `for (` was found.
                                    // When `;` is encountered at the current paren depth matching a for-loop, add space after.
    let mut paren_depth: u32 = 0;
    let mut for_paren_depths: Vec<u32> = Vec::new();

    let mut i = 0;
    while i < len {
        let ch = bytes[i];

        // Track string literals (skip normalization inside strings).
        // Crucially, also skip the string-start check when we're inside a
        // regex literal — `/"/g` has a `"` mid-regex that is NOT a string
        // delimiter. Without this guard the scanner enters string mode at
        // the first `"`, then consumes the closing `/` and the rest of the
        // line as "string contents", producing mangled output like
        // `/" / g, '\\"')` — a real regression observed on
        // `@neondatabase/serverless`'s `index.mjs`.
        //
        // The string-close check below also needs even/odd backslash
        // counting: in a literal like `"\\\\"` (four backslashes inside a
        // string), the `\` immediately before the closing `"` is itself
        // the second half of an escape pair (not an escape of the `"`).
        // The naive `bytes[i-1] != b'\\'` check misses this and leaves
        // the parser in `in_string` mode, swallowing all subsequent
        // characters until the NEXT quote — at which point the parser is
        // off-by-one on string state and mis-parses the next regex as
        // division.
        if !in_line_comment && !in_block_comment && !in_regex {
            if in_string == 0 && (ch == b'\'' || ch == b'"' || ch == b'`') {
                in_string = ch;
                result.push(ch);
                i += 1;
                continue;
            }
            if in_string != 0 {
                // Handle template literal `${...}` expressions
                if in_string == b'`' && ch == b'$' && i + 1 < len && bytes[i + 1] == b'{' {
                    // Enter template expression — exit string mode temporarily
                    template_depth += 1;
                    in_string = 0;
                    result.push(b'$');
                    i += 1;
                    // The `{` will be processed by the main brace handler below
                    continue;
                }
                if closes_string(bytes, i, in_string) {
                    in_string = 0;
                }
                result.push(ch);
                i += 1;
                continue;
            }
        }

        // Handle regex literal content — push verbatim, no normalization.
        if !in_line_comment && !in_block_comment && in_string == 0 && in_regex {
            // JS regex literals cannot contain unescaped newlines — exit regex mode.
            // This prevents false regex detection (e.g. `</tag>` in JSX) from
            // swallowing subsequent lines.
            if ch == b'\n' || ch == b'\r' {
                in_regex = false;
                result.push(ch);
                i += 1;
                continue;
            }
            if ch == b'\\' && i + 1 < len {
                // Escaped character — push both
                result.push(ch);
                result.push(bytes[i + 1]);
                i += 2;
                continue;
            }
            if ch == b'[' && !in_regex_class {
                in_regex_class = true;
            }
            if ch == b']' && in_regex_class {
                in_regex_class = false;
            }
            if ch == b'/' && !in_regex_class {
                // End of regex
                in_regex = false;
                result.push(ch);
                i += 1;
                // Consume regex flags (g, i, m, s, u, y, etc.)
                // Also handle non-ASCII characters (e.g. non-BMP Unicode
                // math-italic letters used as intentionally-invalid flags).
                while i < len && (bytes[i].is_ascii_alphabetic() || bytes[i] >= 0x80) {
                    result.push(bytes[i]);
                    i += 1;
                }
                // Strip space(s) between regex closing `/` (+ flags) and `.` (member access).
                // e.g. `/ pattern / .test(x)` → `/ pattern /.test(x)`
                if i < len && bytes[i] == b' ' {
                    let mut j = i;
                    while j < len && bytes[j] == b' ' {
                        j += 1;
                    }
                    if j < len && bytes[j] == b'.' {
                        i = j; // skip the space(s)
                    }
                }
                continue;
            }
            result.push(ch);
            i += 1;
            continue;
        }

        // Track comments (skip normalization inside comments)
        if !in_line_comment && !in_block_comment && ch == b'/' && i + 1 < len {
            if bytes[i + 1] == b'/' {
                in_line_comment = true;
                result.push(ch);
                i += 1;
                continue;
            }
            if bytes[i + 1] == b'*' {
                in_block_comment = true;
                // Ensure space before `/*` when preceded by non-whitespace
                if !result.is_empty() {
                    let last = *result.last().unwrap();
                    if last != b' '
                        && last != b'\n'
                        && last != b'\r'
                        && last != b'\t'
                        && last != b'('
                        && last != b'['
                        && last != b'{'
                        && last != b'='
                    {
                        result.push(b' ');
                    }
                }
                result.push(ch);
                i += 1;
                continue;
            }
            // Not a comment start — check if this `/` begins a regex literal.
            {
                let prev_non_ws = result.iter().rposition(|&b| b != b' ' && b != b'\t');
                let is_regex_start = match prev_non_ws {
                    None => true, // start of text
                    Some(p) => {
                        let pc = result[p];
                        if pc.is_ascii_alphanumeric() || pc == b'_' || pc == b'$' {
                            // Could be a keyword that allows regex after it
                            let word_start = result[..=p]
                                .iter()
                                .rposition(|b| {
                                    !b.is_ascii_alphanumeric() && *b != b'_' && *b != b'$'
                                })
                                .map(|pos| pos + 1)
                                .unwrap_or(0);
                            let word = &result[word_start..=p];
                            word == b"return"
                                || word == b"typeof"
                                || word == b"void"
                                || word == b"delete"
                                || word == b"throw"
                                || word == b"case"
                                || word == b"new"
                                || word == b"in"
                                || word == b"instanceof"
                                || word == b"yield"
                                || word == b"await"
                        } else {
                            // After operators, punctuation — regex is valid
                            // After string quotes, `/` is division, not regex
                            pc != b')'
                                && pc != b']'
                                && pc != b'}'
                                && pc != b'"'
                                && pc != b'\''
                                && pc != b'`'
                        }
                    }
                };
                if is_regex_start {
                    in_regex = true;
                    in_regex_class = false;
                    result.push(ch);
                    i += 1;
                    continue;
                }
            }
            // Not a regex, not a comment — this `/` is a division operator.
            // TypeScript adds spaces around division operators in structured emit.
            // Skip `/=` (compound assignment) — only handle bare `/`.
            if i + 1 >= len || bytes[i + 1] != b'=' {
                // Add space before `/` if not already present
                if !result.is_empty() {
                    let last = *result.last().unwrap();
                    if last != b' ' && last != b'\t' && last != b'\n' && last != b'\r' {
                        result.push(b' ');
                    }
                }
                result.push(b'/');
                // Add space after `/` if next char is not whitespace
                if i + 1 < len
                    && bytes[i + 1] != b' '
                    && bytes[i + 1] != b'\t'
                    && bytes[i + 1] != b'\n'
                {
                    result.push(b' ');
                }
                i += 1;
                continue;
            }
        }
        if in_line_comment && (ch == b'\n' || ch == b'\r') {
            in_line_comment = false;
            result.push(ch);
            i += 1;
            continue;
        }
        if in_block_comment {
            if ch == b'/' && i > 0 && bytes[i - 1] == b'*' {
                in_block_comment = false;
                result.push(ch);
                i += 1;
                // Ensure space after `*/` when followed by non-whitespace (not punctuation like `;`, `)`, etc.)
                if i < len {
                    let after = bytes[i];
                    if after == b',' {
                        // Add space between `*/` and `,` only when the comment
                        // is the sole non-whitespace content on this line
                        // (e.g. array element that is just a comment before comma).
                        let line_start = result
                            .iter()
                            .rposition(|&b| b == b'\n')
                            .map(|p| p + 1)
                            .unwrap_or(0);
                        let pre_comment = &result[line_start..];
                        // Find the `/*` that started this comment in the result
                        let comment_only = pre_comment
                            .iter()
                            .take_while(|&&b| b == b' ' || b == b'\t')
                            .count()
                            == pre_comment
                                .iter()
                                .position(|&b| b == b'/')
                                .unwrap_or(pre_comment.len());
                        if comment_only {
                            result.push(b' ');
                        }
                    } else if after == b'[' {
                        // `*/[` — element access after comment: `Array /*c*/[`.
                        // Only add space if the character before `/*` is NOT an
                        // identifier char, `)`, or `]` (i.e., not element access).
                        let rlen = result.len();
                        let mut j = rlen.saturating_sub(2); // skip `*/`
                                                            // Walk back past the block comment to find `/*`
                        while j > 0 {
                            if result[j] == b'/' && j + 1 < rlen && result[j + 1] == b'*' {
                                break;
                            }
                            j -= 1;
                        }
                        // j points to `/` of `/*`, look before it
                        let mut bj = if j > 0 { j - 1 } else { 0 };
                        while bj > 0 && result[bj] == b' ' {
                            bj -= 1;
                        }
                        let before_char = result[bj];
                        let is_elem_access = before_char.is_ascii_alphanumeric()
                            || before_char == b'_'
                            || before_char == b'$'
                            || before_char == b')'
                            || before_char == b']';
                        if !is_elem_access {
                            result.push(b' ');
                        }
                    } else if after != b' '
                        && after != b'\n'
                        && after != b'\r'
                        && after != b'\t'
                        && after != b';'
                        && after != b'('
                        && after != b')'
                        && after != b']'
                        && after != b'}'
                        && after != b','
                        && after != b':'
                        && after != b'.'
                        && after != b'"'
                        && after != b'\''
                        && !after.is_ascii_alphanumeric()
                        && after != b'_'
                        && after != b'$'
                    {
                        result.push(b' ');
                    }
                }
                continue;
            }
            result.push(ch);
            i += 1;
            continue;
        }
        if in_line_comment {
            result.push(ch);
            i += 1;
            continue;
        }

        // Track bracket nesting for ternary depth.
        if ch == b'(' || ch == b'[' {
            ternary_stack.push(0);
        }
        // Track paren depth and for-loop paren groups.
        if ch == b'(' {
            paren_depth += 1;
            // Check if preceding word is `for`
            let pns = result.iter().rposition(|&b| b != b' ' && b != b'\t');
            if let Some(p) = pns {
                let word_start = result[..=p]
                    .iter()
                    .rposition(|b| !b.is_ascii_alphanumeric() && *b != b'_')
                    .map(|pos| pos + 1)
                    .unwrap_or(0);
                if &result[word_start..=p] == b"for" {
                    for_paren_depths.push(paren_depth);
                }
            }
        }
        if ch == b')' {
            // Remove for-paren tracking at this depth
            for_paren_depths.retain(|&d| d != paren_depth);
            if paren_depth > 0 {
                paren_depth -= 1;
            }
        }
        if ch == b')' || ch == b']' {
            if ternary_stack.len() > 1 {
                ternary_stack.pop();
            }
        }
        // Collapse empty brackets: `[ ]` (with any whitespace) → `[]`
        if ch == b']' && !result.is_empty() {
            let mut k = result.len();
            while k > 0 && result[k - 1] == b' ' {
                k -= 1;
            }
            if k > 0 && result[k - 1] == b'[' {
                result.truncate(k); // remove all whitespace after `[`
            }
        }
        // Normalize `] [` → `][` (chained element access).
        if ch == b'['
            && result.len() >= 2
            && result[result.len() - 1] == b' '
            && result[result.len() - 2] == b']'
        {
            result.pop(); // remove the space
        }
        // Ensure space after `[` when followed by `/*...*/]` (empty array with only a comment):
        // `[/* comment */]` → `[ /* comment */]`
        if ch == b'[' && i + 1 < len && bytes[i + 1] == b'/' && i + 2 < len && bytes[i + 2] == b'*'
        {
            // Look ahead past the block comment to see if `]` follows
            let mut j = i + 3;
            let mut found_end = false;
            while j + 1 < len {
                if bytes[j] == b'*' && bytes[j + 1] == b'/' {
                    j += 2; // skip past `*/`
                    found_end = true;
                    break;
                }
                j += 1;
            }
            if found_end && j < len && bytes[j] == b']' {
                result.push(b'[');
                result.push(b' ');
                i += 1;
                continue;
            }
        }
        // Normalize `name [` → `name[` (index/array suffix), but keep space
        // after keywords that require it (e.g. `return [ ... ]`).
        if ch == b'[' && !result.is_empty() && *result.last().unwrap() == b' ' {
            let space_pos = result.len() - 1;
            if space_pos > 0 {
                let before_space = result[space_pos - 1];
                if before_space == b']' || before_space == b')' {
                    result.pop(); // `] [` / `) [` -> `][` / `)[`
                } else if before_space.is_ascii_alphanumeric()
                    || before_space == b'_'
                    || before_space == b'$'
                {
                    let word_start = result[..space_pos]
                        .iter()
                        .rposition(|b| !b.is_ascii_alphanumeric() && *b != b'_' && *b != b'$')
                        .map(|p| p + 1)
                        .unwrap_or(0);
                    let word = &result[word_start..space_pos];
                    let is_keyword = word == b"if"
                        || word == b"while"
                        || word == b"for"
                        || word == b"switch"
                        || word == b"catch"
                        || word == b"with"
                        || word == b"var"
                        || word == b"let"
                        || word == b"const"
                        || word == b"return"
                        || word == b"typeof"
                        || word == b"void"
                        || word == b"delete"
                        || word == b"new"
                        || word == b"in"
                        || word == b"of"
                        || word == b"instanceof"
                        || word == b"throw"
                        || word == b"yield"
                        || word == b"await"
                        || word == b"case"
                        || word == b"do"
                        || word == b"else"
                        || word == b"async"
                        || word == b"from"
                        || word == b"import"
                        || word == b"export"
                        || word == b"get"
                        || word == b"set"
                        || word == b"function";
                    if !is_keyword {
                        result.pop();
                    }
                }
            }
        }
        // Remove trailing space before `]` when after `,` (elision/trailing comma).
        if ch == b']'
            && result.len() >= 2
            && result[result.len() - 1] == b' '
            && result[result.len() - 2] == b','
        {
            result.pop(); // remove the space
        }
        // Track ternary `?` (not `?.` or `??`) and normalize spacing.
        if ch == b'?' && i + 1 < len && bytes[i + 1] != b'.' && bytes[i + 1] != b'?' {
            if i > 0 && bytes[i - 1] != b'?' {
                // Normalize spacing: `true?` → `true ?`
                if i > 0 && bytes[i - 1] != b' ' && bytes[i - 1] != b'\n' {
                    result.push(b' ');
                }
                result.push(b'?');
                if bytes[i + 1] != b' ' && bytes[i + 1] != b'\n' {
                    result.push(b' ');
                }
                if let Some(top) = ternary_stack.last_mut() {
                    *top += 1;
                }
                i += 1;
                continue;
            }
            if let Some(top) = ternary_stack.last_mut() {
                *top += 1;
            }
        }
        // Reset ternary count on `,` (new expression context).
        if ch == b',' {
            if let Some(top) = ternary_stack.last_mut() {
                *top = 0;
            }
        }

        if ch == b'{' {
            ternary_stack.push(0);
            let is_template_expr = i > 0 && bytes[i - 1] == b'$';
            // Unicode escape: `\u{XXXX}` — push verbatim, no space padding.
            let is_unicode_escape = i >= 2 && bytes[i - 1] == b'u' && bytes[i - 2] == b'\\';
            if is_unicode_escape {
                // Copy `{...}` verbatim until closing `}`
                result.push(b'{');
                i += 1;
                while i < len && bytes[i] != b'}' {
                    result.push(bytes[i]);
                    i += 1;
                }
                if i < len {
                    result.push(b'}');
                    i += 1;
                }
                continue;
            }
            template_brace_stack.push(is_template_expr);

            if is_template_expr {
                // Template expression `${...}`: no space padding,
                // unless the space precedes a comment (`// ...` or `/* ...`).
                result.push(b'{');
                i += 1;
                // Peek ahead to check if spaces are followed by a comment
                let mut j = i;
                while j < len && bytes[j] == b' ' {
                    j += 1;
                }
                let followed_by_comment = j + 1 < len
                    && bytes[j] == b'/'
                    && (bytes[j + 1] == b'/' || bytes[j + 1] == b'*');
                if !followed_by_comment {
                    // Skip spaces (TypeScript strips them for non-comment content)
                    i = j;
                }
                // When followed by a comment, preserve the space
                continue;
            }

            // Ensure space before `{` when preceded by `)` (function/block body)
            // or by keywords like `const`, `let`, `var` (destructuring).
            if i > 0 && !result.is_empty() {
                let last = *result.last().unwrap();
                if last == b')' {
                    result.push(b' ');
                } else if last.is_ascii_alphabetic() || last == b'_' {
                    // Check if the preceding word is a keyword that needs space before `{`
                    let rlen = result.len();
                    let word_start = result[..rlen]
                        .iter()
                        .rposition(|b| !b.is_ascii_alphanumeric() && *b != b'_')
                        .map(|p| p + 1)
                        .unwrap_or(0);
                    let word = &result[word_start..rlen];
                    if word == b"const"
                        || word == b"let"
                        || word == b"var"
                        || word == b"return"
                        || word == b"throw"
                    {
                        result.push(b' ');
                    }
                }
            }
            // When `{` is at the end of the text, push it and continue.
            if i + 1 >= len {
                result.push(b'{');
                i += 1;
                continue;
            }
            let next = bytes[i + 1];
            // Special case: `{}` after `)` or `=>` is an empty function body → emit `{ }`
            if next == b'}' && i > 0 {
                let prev_non_space = result.iter().rposition(|&b| b != b' ');
                if let Some(p) = prev_non_space {
                    let is_fn_body =
                        result[p] == b')' || (result[p] == b'>' && p > 0 && result[p - 1] == b'=');
                    if is_fn_body {
                        // Ensure space before {
                        if !result.last().map(|&b| b == b' ').unwrap_or(false) {
                            result.push(b' ');
                        }
                        result.push(b'{');
                        result.push(b' ');
                        result.push(b'}');
                        template_brace_stack.pop();
                        i += 2;
                        continue;
                    }
                }
            }
            if next != b' ' && next != b'\n' && next != b'\r' && next != b'}' && next != b'\t' {
                result.push(b'{');
                result.push(b' ');
                i += 1;
                continue;
            }
            // `{` followed by spaces/tabs: collapse to `{ ` (single space)
            if next == b' ' || next == b'\t' {
                result.push(b'{');
                result.push(b' ');
                i += 1;
                // Skip all consecutive spaces/tabs after `{`
                while i < len && (bytes[i] == b' ' || bytes[i] == b'\t') {
                    i += 1;
                }
                continue;
            }
        }

        if ch == b'}' && i > 0 {
            if ternary_stack.len() > 1 {
                ternary_stack.pop();
            }
            let is_closing_template = template_brace_stack.pop().unwrap_or(false);

            if is_closing_template {
                // Template expression `}`: strip trailing spaces before `}`
                while result.last() == Some(&b' ') {
                    result.pop();
                }
                result.push(b'}');
                // Re-enter template string mode
                if template_depth > 0 {
                    template_depth -= 1;
                    in_string = b'`';
                }
                i += 1;
                continue;
            }

            // Check if we need to insert `;` before `}` in inline statement blocks.
            // Only for safe cases: content starts with `return` or `throw`.
            let last_non_space = result.iter().rposition(|&b| b != b' ');
            if let Some(lns) = last_non_space {
                let lnsc = result[lns];
                if lnsc != b';' && lnsc != b'{' && lnsc != b'}' && lnsc != b'\n' && lnsc != b'\r' {
                    let mut depth = 0;
                    let mut brace_pos = None;
                    for j in (0..result.len()).rev() {
                        if result[j] == b'}' {
                            depth += 1;
                        }
                        if result[j] == b'{' {
                            if depth == 0 {
                                brace_pos = Some(j);
                                break;
                            }
                            depth -= 1;
                        }
                    }
                    if let Some(bp) = brace_pos {
                        let block_content = &result[bp + 1..];
                        if !block_content.contains(&b'\n') {
                            let trimmed_start: Vec<u8> = block_content
                                .iter()
                                .copied()
                                .skip_while(|&b| b == b' ')
                                .collect();
                            let starts_with_keyword = trimmed_start.starts_with(b"return ")
                                || trimmed_start.starts_with(b"return;")
                                || trimmed_start.starts_with(b"throw ")
                                || trimmed_start.starts_with(b"var ")
                                || trimmed_start.starts_with(b"let ")
                                || trimmed_start.starts_with(b"const ");
                            // Also add `;` in block bodies preceded by `)` or `=>`
                            // (function bodies, if/for/while bodies, arrow bodies)
                            let is_block_body = if bp > 0 {
                                let prev_non_space = result[..bp].iter().rposition(|&b| b != b' ');
                                prev_non_space
                                    .map(|p| {
                                        result[p] == b')'
                                            || (result[p] == b'>' && p > 0 && result[p - 1] == b'=')
                                    })
                                    .unwrap_or(false)
                            } else {
                                false
                            };
                            if starts_with_keyword || is_block_body {
                                result.insert(lns + 1, b';');
                            }
                        }
                    }
                }
            }

            // Check for empty braces `{ }` → collapse to `{}` unless function body.
            // Find the matching `{` and check if only spaces between.
            if let Some(open_pos) = result.iter().rposition(|&b| b == b'{') {
                let between = &result[open_pos + 1..];
                if between.iter().all(|&b| b == b' ') {
                    // Check if this is a function/arrow body (preceded by `)` or `=>`)
                    let is_fn_body = if open_pos > 0 {
                        let before_open = result[..open_pos].iter().rposition(|&b| b != b' ');
                        before_open
                            .map(|p| {
                                result[p] == b')'
                                    || (result[p] == b'>' && p > 0 && result[p - 1] == b'=')
                            })
                            .unwrap_or(false)
                    } else {
                        false
                    };
                    if !is_fn_body {
                        // Collapse to `{}`
                        result.truncate(open_pos + 1);
                        result.push(b'}');
                        i += 1;
                        continue;
                    }
                }
            }

            let prev = bytes[i - 1];
            if prev != b' ' && prev != b'\t' && prev != b'\n' && prev != b'\r' && prev != b'{' {
                result.push(b' ');
            }
        }

        // Normalize `name (` → `name(` — remove space between function/method name
        // and opening `(`, unless the preceding word is a JS keyword that requires a space.
        // Also normalize `} (` → `}(` for IIFE patterns (function call after body close).
        if ch == b'(' && !result.is_empty() && *result.last().unwrap() == b' ' {
            // Find the space position and check what word precedes it.
            let space_pos = result.len() - 1;
            if space_pos > 0 {
                let before_space = result[space_pos - 1];
                // `} (` → `}(` for IIFE calls (function body close + call paren)
                if before_space == b'}' {
                    result.pop(); // remove the space
                } else if before_space.is_ascii_alphanumeric()
                    || before_space == b'_'
                    || before_space == b'$'
                {
                    // Extract the preceding word.
                    let word_start = result[..space_pos]
                        .iter()
                        .rposition(|b| !b.is_ascii_alphanumeric() && *b != b'_' && *b != b'$')
                        .map(|p| p + 1)
                        .unwrap_or(0);
                    let preceding_word = &result[word_start..space_pos];
                    let is_keyword = preceding_word == b"if"
                        || preceding_word == b"while"
                        || preceding_word == b"for"
                        || preceding_word == b"switch"
                        || preceding_word == b"catch"
                        || preceding_word == b"with"
                        || preceding_word == b"return"
                        || preceding_word == b"typeof"
                        || preceding_word == b"void"
                        || preceding_word == b"delete"
                        || preceding_word == b"new"
                        || preceding_word == b"in"
                        || preceding_word == b"of"
                        || preceding_word == b"instanceof"
                        || preceding_word == b"throw"
                        || preceding_word == b"yield"
                        || preceding_word == b"await"
                        || preceding_word == b"case"
                        || preceding_word == b"do"
                        || preceding_word == b"else"
                        || preceding_word == b"async"
                        || preceding_word == b"from"
                        || preceding_word == b"import"
                        || preceding_word == b"export"
                        || preceding_word == b"get"
                        || preceding_word == b"set"
                        || preceding_word == b"with"
                        || preceding_word == b"function";
                    // If the word is preceded by `.`, it's a method call (e.g. Symbol.for()),
                    // not a keyword — allow the space to be removed.
                    let is_method_call = word_start > 0 && result[word_start - 1] == b'.';
                    if !is_keyword || is_method_call {
                        result.pop(); // remove the space
                    }
                }
            }
        }

        // Normalize `; )` → `;)` (for-loop empty update clause).
        if ch == b')'
            && result.len() >= 2
            && result[result.len() - 1] == b' '
            && result[result.len() - 2] == b';'
        {
            result.pop(); // remove the space
        }

        // Normalize `; ;` → `;;` (for-loop empty test/update).
        if ch == b';'
            && result.len() >= 2
            && result[result.len() - 1] == b' '
            && result[result.len() - 2] == b';'
        {
            result.pop(); // remove the space
        }

        // Normalize for-loop `;` spacing: `for(x;y;z)` → `for(x; y; z)`.
        // Add space after `;` when inside a for-loop paren group.
        if ch == b';' && i + 1 < len && for_paren_depths.contains(&paren_depth) {
            let next = bytes[i + 1];
            if next != b' ' && next != b'\n' && next != b'\r' && next != b';' && next != b')' {
                result.push(b';');
                result.push(b' ');
                i += 1;
                continue;
            }
        }

        // Normalize `} )` → `})` (closing function/object passed as argument).
        if ch == b')'
            && result.len() >= 2
            && result[result.len() - 1] == b' '
            && result[result.len() - 2] == b'}'
        {
            result.pop(); // remove the space
        }

        // Normalize postfix `++`/`--` spacing: `x ++` → `x++`, `x --` → `x--`.
        // Remove space before `++`/`--` when preceded by alphanumeric, `)`, or `]`.
        // But NOT when followed by alphanumeric (prefix `--x`): `void --x` stays.
        if (ch == b'+' || ch == b'-') && i + 1 < len && bytes[i + 1] == ch {
            let is_prefix = i + 2 < len
                && (bytes[i + 2].is_ascii_alphanumeric()
                    || bytes[i + 2] == b'_'
                    || bytes[i + 2] == b'$');
            if !is_prefix && result.len() >= 2 && result[result.len() - 1] == b' ' {
                let before_space = result[result.len() - 2];
                if before_space.is_ascii_alphanumeric()
                    || before_space == b'_'
                    || before_space == b'$'
                    || before_space == b')'
                    || before_space == b']'
                {
                    result.pop(); // remove the space
                }
            }
        }

        // Normalize prefix `++`/`--` spacing: `++ new Foo()` → `++new Foo()`.
        // When a space follows `++`/`--` and the `++`/`--` is NOT postfix
        // (i.e., not preceded by alphanumeric/`)`/`]`), collapse the space.
        if ch == b' ' && result.len() >= 2 {
            let r_last = result[result.len() - 1];
            let r_prev = result[result.len() - 2];
            if (r_last == b'+' && r_prev == b'+') || (r_last == b'-' && r_prev == b'-') {
                let before_op = if result.len() >= 3 {
                    result[result.len() - 3]
                } else {
                    b'\0'
                };
                let is_postfix = before_op.is_ascii_alphanumeric()
                    || before_op == b'_'
                    || before_op == b'$'
                    || before_op == b')'
                    || before_op == b']';
                if !is_postfix {
                    i += 1; // skip the space after prefix ++/--
                    continue;
                }
            }
        }

        // Remove space between `]` and `(`: `arr[i] (x)` → `arr[i](x)`,
        // `{ [Symbol.iterator] () {} }` → `{ [Symbol.iterator]() {} }`
        if ch == b'('
            && result.len() >= 2
            && result[result.len() - 1] == b' '
            && result[result.len() - 2] == b']'
        {
            result.pop(); // remove the space
        }

        // Normalize arrow `=>` spacing: `x=> y` → `x => y`, `x =>y` → `x => y`
        if ch == b'=' && i + 1 < len && bytes[i + 1] == b'>' {
            if i > 0 {
                let prev = bytes[i - 1];
                if prev != b' ' && prev != b'\n' && prev != b'\r' {
                    result.push(b' ');
                }
            }
            result.push(b'=');
            result.push(b'>');
            if i + 2 < len {
                let next2 = bytes[i + 2];
                if next2 != b' ' && next2 != b'\n' && next2 != b'\r' {
                    result.push(b' ');
                }
            }
            i += 2;
            continue;
        }

        // Normalize === and == spacing
        if ch == b'=' && i + 1 < len && bytes[i + 1] == b'=' {
            let triple = i + 2 < len && bytes[i + 2] == b'=';
            let op_len = if triple { 3 } else { 2 };
            if i > 0 {
                let prev = bytes[i - 1];
                if prev != b' ' && prev != b'\n' && prev != b'\r' && prev != b'!' {
                    result.push(b' ');
                }
            }
            for _ in 0..op_len {
                result.push(b'=');
            }
            if i + op_len < len {
                let next = bytes[i + op_len];
                if next != b' ' && next != b'\n' && next != b'\r' {
                    result.push(b' ');
                }
            }
            i += op_len;
            continue;
        }

        // Normalize !== and != spacing
        if ch == b'!' && i + 1 < len && bytes[i + 1] == b'=' {
            let triple = i + 2 < len && bytes[i + 2] == b'=';
            let op_len = if triple { 3 } else { 2 };
            if i > 0 {
                let prev = bytes[i - 1];
                if prev != b' ' && prev != b'\n' && prev != b'\r' {
                    result.push(b' ');
                }
            }
            result.push(b'!');
            for _ in 1..op_len {
                result.push(b'=');
            }
            if i + op_len < len {
                let next = bytes[i + op_len];
                if next != b' ' && next != b'\n' && next != b'\r' {
                    result.push(b' ');
                }
            }
            i += op_len;
            continue;
        }

        // Normalize >= and <= spacing
        if (ch == b'>' || ch == b'<') && i + 1 < len && bytes[i + 1] == b'=' {
            // Skip <<= and >>=
            if !(i > 0 && bytes[i - 1] == ch) {
                if i > 0 {
                    let prev = bytes[i - 1];
                    if prev != b' ' && prev != b'\n' && prev != b'\r' {
                        result.push(b' ');
                    }
                }
                result.push(ch);
                result.push(b'=');
                if i + 2 < len {
                    let next2 = bytes[i + 2];
                    if next2 != b' ' && next2 != b'\n' && next2 != b'\r' && next2 != b'=' {
                        result.push(b' ');
                    }
                }
                i += 2;
                continue;
            }
        }

        // Normalize shift operator spacing: `a>>b` → `a >> b`, `a<<b` → `a << b`,
        // `a>>>b` → `a >>> b`. Skip >>=, <<=, >>>=.
        if ch == b'>' && i + 1 < len && bytes[i + 1] == b'>' {
            // >>> or >>
            let is_triple = i + 2 < len && bytes[i + 2] == b'>';
            let op_len = if is_triple { 3 } else { 2 };
            // Skip >>= and >>>=
            if i + op_len < len && bytes[i + op_len] == b'=' {
                // assignment variant — just push chars
            } else {
                if i > 0 {
                    let prev = bytes[i - 1];
                    if prev != b' ' && prev != b'\n' && prev != b'\r' {
                        result.push(b' ');
                    }
                }
                for _ in 0..op_len {
                    result.push(b'>');
                }
                if i + op_len < len {
                    let next = bytes[i + op_len];
                    if next != b' ' && next != b'\n' && next != b'\r' {
                        result.push(b' ');
                    }
                }
                i += op_len;
                continue;
            }
        }
        if ch == b'<' && i + 1 < len && bytes[i + 1] == b'<' {
            // Skip <<=
            if i + 2 < len && bytes[i + 2] == b'=' {
                // assignment variant — just push chars
            } else {
                if i > 0 {
                    let prev = bytes[i - 1];
                    if prev != b' ' && prev != b'\n' && prev != b'\r' {
                        result.push(b' ');
                    }
                }
                result.push(b'<');
                result.push(b'<');
                if i + 2 < len {
                    let next2 = bytes[i + 2];
                    if next2 != b' ' && next2 != b'\n' && next2 != b'\r' {
                        result.push(b' ');
                    }
                }
                i += 2;
                continue;
            }
        }

        // Normalize standalone < and > comparison operator spacing: `a<b` → `a < b`
        // Skip <<, >>, >>>, <=, >=, => which are handled elsewhere.
        if (ch == b'<' || ch == b'>') && i + 1 < len {
            let next = bytes[i + 1];
            // Skip <=, >= (already handled), <<, >>, >>>, =>, />
            if next != b'='
                && next != ch
                && !(ch == b'>' && i > 0 && bytes[i - 1] == b'=')
                && !(ch == b'>' && i > 0 && bytes[i - 1] == b'/')
                && !(ch == b'>' && i > 0 && bytes[i - 1] == b'>')
            // end of >>
            {
                if i > 0 {
                    let prev = bytes[i - 1];
                    // Check if < or > is a binary operator by looking at
                    // the preceding non-space character.
                    let effective_prev = if prev == b' ' && i >= 2 {
                        bytes[i - 2]
                    } else {
                        prev
                    };
                    let is_binary = effective_prev.is_ascii_alphanumeric()
                        || effective_prev == b')'
                        || effective_prev == b']'
                        || effective_prev == b'\''
                        || effective_prev == b'"'
                        || effective_prev == b'`'
                        || effective_prev == b'_'
                        || effective_prev == b'$';
                    if is_binary {
                        if prev != b' ' && prev != b'\n' && prev != b'\r' {
                            result.push(b' ');
                        }
                        result.push(ch);
                        if next != b' ' && next != b'\n' && next != b'\r' {
                            result.push(b' ');
                        }
                        i += 1;
                        continue;
                    }
                }
            }
        }

        // Normalize **= (exponentiation assignment) spacing: `x**=y` → `x **= y`
        if ch == b'=' && i >= 2 && bytes[i - 1] == b'*' && bytes[i - 2] == b'*' {
            // Both `*` chars are already in result. Ensure space before them.
            let op_start = result.len().saturating_sub(2); // position of first *
            if op_start > 0
                && result[op_start - 1] != b' '
                && result[op_start - 1] != b'\n'
                && result[op_start - 1] != b'\r'
            {
                result.insert(op_start, b' ');
            }
            result.push(b'=');
            if i + 1 < len {
                let next = bytes[i + 1];
                if next != b' ' && next != b'\n' && next != b'\r' {
                    result.push(b' ');
                }
            }
            i += 1;
            continue;
        }

        // Normalize compound assignment operator spacing: `x+=y` → `x += y`
        if ch == b'=' && i > 0 && i + 1 < len {
            let prev = bytes[i - 1];
            let next = bytes[i + 1];
            if next != b'='
                && next != b'>'
                && (prev == b'+' || prev == b'-' || prev == b'*' || prev == b'%' || prev == b'^')
            {
                // The operator char is already in result. Ensure space before it.
                let op_pos = result.len() - 1;
                if op_pos > 0
                    && result[op_pos - 1] != b' '
                    && result[op_pos - 1] != b'\n'
                    && result[op_pos - 1] != b'\r'
                {
                    result.insert(op_pos, b' ');
                }
                result.push(b'=');
                if next != b' ' && next != b'\n' && next != b'\r' {
                    result.push(b' ');
                }
                i += 1;
                continue;
            }
        }

        // Normalize assignment operator spacing: `x=y` → `x = y`.
        // Skip compound assignments (+=, -=, *=, /=, %=, |=, &=, ^=, etc.)
        if ch == b'=' && i > 0 && i + 1 < len {
            let prev = bytes[i - 1];
            let next = bytes[i + 1];
            if prev != b'!'
                && prev != b'<'
                && prev != b'>'
                && prev != b'='
                && prev != b'+'
                && prev != b'-'
                && prev != b'*'
                && prev != b'/'
                && prev != b'%'
                && prev != b'|'
                && prev != b'&'
                && prev != b'^'
                && prev != b'?'
                && next != b'='
                && next != b'>'
            {
                // Collapse multiple trailing spaces to exactly one
                while result.len() >= 2
                    && result[result.len() - 1] == b' '
                    && result[result.len() - 2] == b' '
                {
                    result.pop();
                }
                if !result
                    .last()
                    .map(|&b| b == b' ' || b == b'\n' || b == b'\r')
                    .unwrap_or(false)
                {
                    result.push(b' ');
                }
                result.push(b'=');
                if next != b' ' && next != b'\n' && next != b'\r' {
                    result.push(b' ');
                }
                i += 1;
                continue;
            }
        }

        // Normalize comma spacing: add space after `,` when not already present.
        // Don't add space before `]` or `)` (trailing commas, elisions).
        if ch == b',' && i + 1 < len {
            let next = bytes[i + 1];
            if next != b' ' && next != b'\n' && next != b'\r' && next != b']' && next != b')' {
                result.push(b',');
                result.push(b' ');
                i += 1;
                continue;
            }
        }

        // Normalize colon spacing.
        if ch == b':' && i > 0 {
            let is_ternary = ternary_stack.last().copied().unwrap_or(0) > 0;
            if is_ternary {
                // Ternary colon: decrement depth, ensure space before `:`
                if let Some(top) = ternary_stack.last_mut() {
                    *top -= 1;
                }
                if result.last() != Some(&b' ') {
                    result.push(b' ');
                }
            } else {
                // Property/label/case colon: remove space before `:`.
                while result.last() == Some(&b' ') {
                    // Don't remove space if it's after a non-word char (e.g. `{ }:` edge case)
                    if result.len() >= 2 {
                        let before_space = result[result.len() - 2];
                        if before_space.is_ascii_alphanumeric()
                            || before_space == b'_'
                            || before_space == b'$'
                            || before_space == b'\''
                            || before_space == b'"'
                            || before_space == b']'
                        {
                            result.pop();
                        } else {
                            break;
                        }
                    } else {
                        break;
                    }
                }
            }
            // Add space after `:` when not already present.
            if i + 1 < len {
                let next = bytes[i + 1];
                let prev_result = result.last().copied().unwrap_or(0);
                if next != b' '
                    && next != b'\n'
                    && next != b'\r'
                    && next != b':'
                    && (is_ternary
                        || prev_result.is_ascii_alphanumeric()
                        || prev_result == b'_'
                        || prev_result == b'\''
                        || prev_result == b'"'
                        || prev_result == b']'
                        || prev_result == b')')
                {
                    result.push(b':');
                    result.push(b' ');
                    i += 1;
                    continue;
                }
            }
        }

        // Normalize && spacing: `a&&b` → `a && b`
        if ch == b'&' && i + 1 < len && bytes[i + 1] == b'&' {
            if i + 2 >= len || bytes[i + 2] != b'=' {
                // skip &&=
                if i > 0 {
                    let prev = bytes[i - 1];
                    if prev != b' ' && prev != b'\n' && prev != b'\r' {
                        result.push(b' ');
                    }
                }
                result.push(b'&');
                result.push(b'&');
                if i + 2 < len {
                    let next2 = bytes[i + 2];
                    if next2 != b' ' && next2 != b'\n' && next2 != b'\r' {
                        result.push(b' ');
                    }
                }
                i += 2;
                continue;
            }
        }

        // Normalize || spacing: `a||b` → `a || b`
        if ch == b'|' && i + 1 < len && bytes[i + 1] == b'|' {
            if i + 2 >= len || bytes[i + 2] != b'=' {
                // skip ||=
                if i > 0 {
                    let prev = bytes[i - 1];
                    if prev != b' ' && prev != b'\n' && prev != b'\r' {
                        result.push(b' ');
                    }
                }
                result.push(b'|');
                result.push(b'|');
                if i + 2 < len {
                    let next2 = bytes[i + 2];
                    if next2 != b' ' && next2 != b'\n' && next2 != b'\r' {
                        result.push(b' ');
                    }
                }
                i += 2;
                continue;
            }
        }

        // Normalize binary + spacing: `a+b` → `a + b` (skip ++, +=, unary +, exponent 12e+3)
        if ch == b'+' && i + 1 < len {
            let next = bytes[i + 1];
            if next != b'+' && next != b'=' {
                // skip ++, +=
                if i > 0 && bytes[i - 1] != b'+' {
                    // skip second + of ++
                    let prev = bytes[i - 1];
                    // Skip exponent sign: 12e+3, 1.5E+10
                    if (prev == b'e' || prev == b'E') && i >= 2 && bytes[i - 2].is_ascii_digit() {
                        result.push(ch);
                        i += 1;
                        continue;
                    }
                    // Look back past whitespace for meaningful preceding char
                    let mut pi = i - 1;
                    while pi > 0 && (bytes[pi] == b' ' || bytes[pi] == b'\t') {
                        pi -= 1;
                    }
                    let prev_meaningful = bytes[pi];
                    let is_binary = (prev_meaningful.is_ascii_alphanumeric()
                        && !preceding_word_is_unary_keyword(bytes, pi))
                        || prev_meaningful == b')'
                        || prev_meaningful == b']'
                        || prev_meaningful == b'\''
                        || prev_meaningful == b'"'
                        || prev_meaningful == b'`'
                        || prev_meaningful == b'_'
                        // Trailing decimal point on numeric literal: `1.+` → `1. +`
                        || (prev_meaningful == b'.' && pi > 0 && bytes[pi - 1].is_ascii_digit());
                    if is_binary {
                        if !result.last().is_some_and(|&b| b == b' ') {
                            result.push(b' ');
                        }
                        result.push(b'+');
                        if next != b' ' && next != b'\n' && next != b'\r' {
                            result.push(b' ');
                        }
                        i += 1;
                        continue;
                    }
                }
            }
        }

        // Normalize binary - spacing: `a-b` → `a - b` (skip --, -=, unary -, exponent 12e-3)
        if ch == b'-' && i + 1 < len {
            let next = bytes[i + 1];
            if next != b'-' && next != b'=' {
                // skip --, -=
                if i > 0 && bytes[i - 1] != b'-' {
                    // skip second - of --
                    let prev = bytes[i - 1];
                    // Skip exponent sign: 12e-3, 1.5E-10
                    if (prev == b'e' || prev == b'E') && i >= 2 && bytes[i - 2].is_ascii_digit() {
                        result.push(ch);
                        i += 1;
                        continue;
                    }
                    // Look back past whitespace for meaningful preceding char
                    let mut pi = i - 1;
                    while pi > 0 && (bytes[pi] == b' ' || bytes[pi] == b'\t') {
                        pi -= 1;
                    }
                    let prev_meaningful = bytes[pi];
                    let is_binary = (prev_meaningful.is_ascii_alphanumeric()
                        && !preceding_word_is_unary_keyword(bytes, pi))
                        || prev_meaningful == b')'
                        || prev_meaningful == b']'
                        || prev_meaningful == b'\''
                        || prev_meaningful == b'"'
                        || prev_meaningful == b'`'
                        || prev_meaningful == b'_'
                        // Trailing decimal point on numeric literal: `1.-` → `1. -`
                        || (prev_meaningful == b'.' && pi > 0 && bytes[pi - 1].is_ascii_digit());
                    if is_binary {
                        if !result.last().is_some_and(|&b| b == b' ') {
                            result.push(b' ');
                        }
                        result.push(b'-');
                        if next != b' ' && next != b'\n' && next != b'\r' {
                            result.push(b' ');
                        }
                        i += 1;
                        continue;
                    }
                }
            }
        }

        // Normalize binary * spacing: `a*b` → `a * b` (skip **, *=, function*, yield*)
        if ch == b'*' && i + 1 < len {
            let next = bytes[i + 1];
            // Check for generator context FIRST: `function *` or `yield *` should become
            // `function*` or `yield*` (no space before, space after for readability).
            // This must run before the `*/` guard because `yield */*comment*/` has
            // next=='/' but the `*` is a generator delegate, not a comment closer.
            // Also handles `yield /*comment*/ *` where * follows a block comment.
            {
                let mut search_end = result.len();
                // Skip trailing whitespace
                while search_end > 0 && matches!(result[search_end - 1], b' ' | b'\t') {
                    search_end -= 1;
                }
                // If result ends with `*/`, skip backwards past the block comment
                // to find the keyword before it (e.g., `yield /*comment*/ *`)

                if search_end >= 2
                    && result[search_end - 2] == b'*'
                    && result[search_end - 1] == b'/'
                {
                    // Find matching `/*`
                    if let Some(comment_start) = result[..search_end - 2]
                        .windows(2)
                        .rposition(|w| w[0] == b'/' && w[1] == b'*')
                    {
                        // Skip whitespace before the block comment
                        let mut before_comment = comment_start;
                        while before_comment > 0
                            && matches!(result[before_comment - 1], b' ' | b'\t')
                        {
                            before_comment -= 1;
                        }
                        let word_start = result[..before_comment]
                            .iter()
                            .rposition(|b| !b.is_ascii_alphanumeric() && *b != b'_')
                            .map(|p| p + 1)
                            .unwrap_or(0);
                        let preceding_word = &result[word_start..before_comment];
                        if preceding_word == b"function" || preceding_word == b"yield" {
                            // `yield /*comment*/ *` → `yield /*comment*/*`
                            // Truncate to remove whitespace after comment, attach * directly
                            result.truncate(search_end);
                            result.push(b'*');
                            if next != b' ' && next != b'\n' && next != b'\r' {
                                result.push(b' ');
                            }
                            i += 1;
                            continue;
                        }
                    }
                }
                // Simple case: no block comment between keyword and *
                let trimmed_end = search_end;
                let word_start = result[..trimmed_end]
                    .iter()
                    .rposition(|b| !b.is_ascii_alphanumeric() && *b != b'_')
                    .map(|p| p + 1)
                    .unwrap_or(0);
                let preceding_word = &result[word_start..trimmed_end];
                let is_generator = preceding_word == b"function" || preceding_word == b"yield";
                if is_generator {
                    // Strip whitespace between keyword and *, attach * directly
                    result.truncate(trimmed_end);
                    result.push(b'*');
                    // Ensure space after * if next char isn't whitespace
                    if next != b' ' && next != b'\n' && next != b'\r' {
                        result.push(b' ');
                    }
                    i += 1;
                    continue;
                }
            }
            if next != b'*' && next != b'=' && next != b'/' {
                // skip **, *=, closing */
                if i > 0 {
                    // Look back past whitespace for meaningful preceding char
                    let mut pi = i - 1;
                    while pi > 0 && (bytes[pi] == b' ' || bytes[pi] == b'\t') {
                        pi -= 1;
                    }
                    let prev = bytes[pi];
                    let is_binary = prev.is_ascii_alphanumeric()
                        || prev == b')'
                        || prev == b']'
                        || prev == b'\''
                        || prev == b'"'
                        || prev == b'`'
                        || prev == b'_'
                        // Trailing decimal point on numeric literal: `1.*` → `1. *`
                        || (prev == b'.' && pi > 0 && bytes[pi - 1].is_ascii_digit());
                    if is_binary {
                        // Not a generator, not export/import — binary operator
                        if !result.last().is_some_and(|&b| b == b' ') {
                            result.push(b' ');
                        }
                        result.push(b'*');
                        if next != b' ' && next != b'\n' && next != b'\r' {
                            result.push(b' ');
                        }
                        i += 1;
                        continue;
                    }
                }
            }
        }

        // Normalize binary % spacing: `a%b` → `a % b` (skip %=)
        if ch == b'%' && i + 1 < len {
            let next = bytes[i + 1];
            if next != b'=' {
                // skip %=
                if i > 0 {
                    let prev = bytes[i - 1];
                    let is_binary = prev.is_ascii_alphanumeric()
                        || prev == b')'
                        || prev == b']'
                        || prev == b'\''
                        || prev == b'"'
                        || prev == b'`'
                        || prev == b'_';
                    if is_binary {
                        if prev != b' ' && prev != b'\n' && prev != b'\r' {
                            result.push(b' ');
                        }
                        result.push(b'%');
                        if next != b' ' && next != b'\n' && next != b'\r' {
                            result.push(b' ');
                        }
                        i += 1;
                        continue;
                    }
                }
            }
        }

        // Normalize binary ^ spacing: `a^b` → `a ^ b` (skip ^=)
        if ch == b'^' && i + 1 < len {
            let next = bytes[i + 1];
            if next != b'=' {
                // skip ^=
                if i > 0 {
                    let prev = bytes[i - 1];
                    let is_binary = prev.is_ascii_alphanumeric()
                        || prev == b')'
                        || prev == b']'
                        || prev == b'\''
                        || prev == b'"'
                        || prev == b'`'
                        || prev == b'_';
                    if is_binary {
                        if prev != b' ' && prev != b'\n' && prev != b'\r' {
                            result.push(b' ');
                        }
                        result.push(b'^');
                        if next != b' ' && next != b'\n' && next != b'\r' {
                            result.push(b' ');
                        }
                        i += 1;
                        continue;
                    }
                }
            }
        }

        // Normalize binary single & spacing: `a&b` → `a & b` (skip &&, &=)
        if ch == b'&' && i + 1 < len {
            let next = bytes[i + 1];
            if next != b'&' && next != b'=' {
                // skip &&, &=
                if i > 0 && bytes[i - 1] != b'&' {
                    // skip second & of &&
                    let prev = bytes[i - 1];
                    let is_binary = prev.is_ascii_alphanumeric()
                        || prev == b')'
                        || prev == b']'
                        || prev == b'\''
                        || prev == b'"'
                        || prev == b'`'
                        || prev == b'_';
                    if is_binary {
                        if prev != b' ' && prev != b'\n' && prev != b'\r' {
                            result.push(b' ');
                        }
                        result.push(b'&');
                        if next != b' ' && next != b'\n' && next != b'\r' {
                            result.push(b' ');
                        }
                        i += 1;
                        continue;
                    }
                }
            }
        }

        // Normalize binary single | spacing: `a|b` → `a | b` (skip ||, |=)
        if ch == b'|' && i + 1 < len {
            let next = bytes[i + 1];
            if next != b'|' && next != b'=' {
                // skip ||, |=
                if i > 0 && bytes[i - 1] != b'|' {
                    // skip second | of ||
                    let prev = bytes[i - 1];
                    let is_binary = prev.is_ascii_alphanumeric()
                        || prev == b')'
                        || prev == b']'
                        || prev == b'\''
                        || prev == b'"'
                        || prev == b'`'
                        || prev == b'_';
                    if is_binary {
                        if prev != b' ' && prev != b'\n' && prev != b'\r' {
                            result.push(b' ');
                        }
                        result.push(b'|');
                        if next != b' ' && next != b'\n' && next != b'\r' {
                            result.push(b' ');
                        }
                        i += 1;
                        continue;
                    }
                }
            }
        }

        // Collapse consecutive spaces on the same line (not leading indentation).
        // Only collapse if the line has non-space content before these spaces.
        if ch == b' ' && result.last() == Some(&b' ') {
            // Check if there's non-space content after the last newline.
            // Treat tabs as whitespace for this check — tab characters in
            // leading indentation should not trigger space collapsing.
            let has_non_space = result
                .iter()
                .rev()
                .skip(1) // skip the existing space
                .take_while(|&&b| b != b'\n' && b != b'\r')
                .any(|&b| b != b' ' && b != b'\t');
            if has_non_space {
                i += 1;
                continue;
            }
        }

        result.push(ch);
        i += 1;
    }

    // If no changes were made, return borrowed to avoid allocation overhead downstream.
    if result.len() == text.len() && result == text.as_bytes() {
        return Cow::Borrowed(text);
    }
    Cow::Owned(String::from_utf8(result).unwrap_or_else(|_| text.to_string()))
}

/// Normalize keyword-paren spacing: `if(` → `if (`, `while(` → `while (`, etc.
/// Only applies at word boundaries to avoid changing identifiers like `notif(`.
pub(crate) fn normalize_keyword_paren(text: &str) -> Cow<'_, str> {
    // Quick check: no `(` means no keyword-paren patterns possible.
    if !text.contains('(') {
        return Cow::Borrowed(text);
    }
    // Single-pass: scan for `(` and check if preceded by a keyword at word boundary.
    // Inserts a space before `(` when matched. This replaces the old 11-iteration approach.
    static KEYWORDS: &[&[u8]] = &[
        b"function*", // must be before "function" to match first
        b"function",
        b"if",
        b"for",
        b"while",
        b"switch",
        b"catch",
        b"with",
        b"void",
        b"typeof",
        b"new",
    ];
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut out: Vec<u8> = Vec::with_capacity(len + 16);
    let mut state = LineState::new();
    let mut i = 0;
    while i < len {
        let ch = bytes[i];
        // The rule fires only on `(` in code context (not inside a string,
        // comment, or regex). `LineState::in_code()` returns true inside a
        // template-expression `${...}`, which matches the existing
        // semantics — code inside backtick interpolations is normalized
        // like any other JS.
        if state.in_code() && ch == b'(' && !out.is_empty() && out[out.len() - 1] != b' ' {
            for kw in KEYWORDS {
                let klen = kw.len();
                if out.len() >= klen && &out[out.len() - klen..] == *kw {
                    let at_boundary = out.len() == klen || {
                        let prev = out[out.len() - klen - 1];
                        !prev.is_ascii_alphanumeric() && prev != b'_' && prev != b'.'
                    };
                    if at_boundary {
                        out.push(b' ');
                        break;
                    }
                }
            }
        }
        let advance = state.advance(bytes, i);
        out.extend_from_slice(&bytes[i..i + advance]);
        i += advance;
    }
    // SAFETY: we only inserted ASCII space bytes into valid UTF-8 input.
    Cow::Owned(unsafe { String::from_utf8_unchecked(out) })
}

/// Replace Unicode NEL (U+0085) with regular space, but only outside string literals.
pub(crate) fn normalize_nel(text: &str) -> Cow<'_, str> {
    let nel = "\u{0085}";
    let mut result = String::with_capacity(text.len());
    let mut in_string: char = '\0';
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if in_string != '\0' {
            result.push(ch);
            if ch == in_string && !result.ends_with("\\\\") {
                // Simple escape check (not fully robust but sufficient)
                in_string = '\0';
            }
            continue;
        }
        if ch == '\'' || ch == '"' || ch == '`' {
            in_string = ch;
            result.push(ch);
            continue;
        }
        if ch == '\u{0085}' {
            result.push(' ');
        } else {
            result.push(ch);
        }
    }
    let _ = nel; // suppress unused warning
    Cow::Owned(result)
}

/// Replace Unicode NBSP (U+00A0) with regular space outside string/template literals.
pub(crate) fn normalize_nbsp(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut in_string: char = '\0';
    for ch in text.chars() {
        if in_string != '\0' {
            result.push(ch);
            if ch == in_string && !result.ends_with("\\\\") {
                in_string = '\0';
            }
            continue;
        }
        if ch == '\'' || ch == '"' || ch == '`' {
            in_string = ch;
            result.push(ch);
            continue;
        }
        if ch == '\u{00A0}' {
            result.push(' ');
        } else {
            result.push(ch);
        }
    }
    result
}

/// Convert empty block bodies `=> {}` → `=> { }`, `) {}` → `) { }`, etc.
/// TypeScript's structured emit always puts a space inside empty block statements.
/// This is separate from `normalize_brace_spacing` because it must run even when
/// brace normalization is skipped (e.g. JSX preserve mode).
/// Normalize Allman-style braces: join `)\n<whitespace>{` into `) {`.
/// TypeScript always emits K&R style for control flow statements.
pub(crate) fn normalize_allman_braces(text: &str) -> Cow<'_, str> {
    if !text.contains(")\n") {
        return Cow::Borrowed(text);
    }
    let mut result = text.to_string();
    let mut search_from = 0;
    while let Some(rel_pos) = result[search_from..].find(")\n") {
        let paren_pos = search_from + rel_pos;
        let after_paren = paren_pos + 2; // skip ")\n"
                                         // Find start of next line content (skip spaces/tabs only, not newlines)
        let next_non_ws = result[after_paren..]
            .find(|c: char| c != ' ' && c != '\t')
            .map(|p| after_paren + p)
            .unwrap_or(result.len());
        // If the next non-whitespace char is `{`
        if next_non_ws < result.len() && result.as_bytes()[next_non_ws] == b'{' {
            // Replace )\n...{ with ) {
            result.replace_range(paren_pos + 1..next_non_ws, " ");
            search_from = paren_pos + 2; // continue after the replacement
        } else {
            search_from = paren_pos + 2;
        }
    }
    Cow::Owned(result)
}

pub(crate) fn normalize_empty_blocks(text: &str) -> Cow<'_, str> {
    if !text.contains("{}") {
        return Cow::Borrowed(text);
    }
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut result = Vec::with_capacity(len + 8);
    let mut in_string: u8 = 0;
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut i = 0;
    while i < len {
        let ch = bytes[i];

        // Skip comments and strings
        if in_line_comment {
            if ch == b'\n' {
                in_line_comment = false;
            }
            result.push(ch);
            i += 1;
            continue;
        }
        if in_block_comment {
            if ch == b'*' && i + 1 < len && bytes[i + 1] == b'/' {
                in_block_comment = false;
                result.push(b'*');
                result.push(b'/');
                i += 2;
            } else {
                result.push(ch);
                i += 1;
            }
            continue;
        }
        if in_string != 0 {
            if closes_string(bytes, i, in_string) {
                in_string = 0;
            }
            result.push(ch);
            i += 1;
            continue;
        }
        if ch == b'\'' || ch == b'"' || ch == b'`' {
            in_string = ch;
            result.push(ch);
            i += 1;
            continue;
        }
        if ch == b'/' && i + 1 < len {
            if bytes[i + 1] == b'/' {
                in_line_comment = true;
            } else if bytes[i + 1] == b'*' {
                in_block_comment = true;
            }
        }

        // Detect `{}` preceded by `)`, `=>`, `else`, `try`, `catch`, `finally`
        if ch == b'{' && i + 1 < len && bytes[i + 1] == b'}' {
            // Check context before `{`
            let before = if !result.is_empty() {
                // Find last non-space character
                result.iter().rposition(|&b| b != b' ')
            } else {
                None
            };
            // Skip past block comments to find the real context character.
            // e.g. `with (false) /*5*/ {}` → skip `/*5*/` to find `)`.
            let effective_before = before.and_then(|mut p| {
                loop {
                    if p >= 1 && result[p] == b'/' && result[p - 1] == b'*' {
                        // At end of block comment `*/` — find matching `/*`
                        if p < 3 {
                            return None;
                        }
                        let mut j = p - 2;
                        loop {
                            if result[j] == b'/' && j + 1 < result.len() && result[j + 1] == b'*' {
                                // Found `/*` at j
                                if j == 0 {
                                    return None;
                                }
                                p = j - 1;
                                // Skip spaces
                                while p > 0 && result[p] == b' ' {
                                    p -= 1;
                                }
                                if result[p] == b' ' {
                                    return None;
                                }
                                break;
                            }
                            if j == 0 {
                                return None;
                            }
                            j -= 1;
                        }
                    } else {
                        return Some(p);
                    }
                }
            });
            let is_block = if let Some(p) = effective_before {
                result[p] == b')'
                    || (result[p] == b'>' && p > 0 && result[p - 1] == b'=')
                    || (p >= 3
                        && result[p - 3] == b'e'
                        && result[p - 2] == b'l'
                        && result[p - 1] == b's'
                        && result[p] == b'e')
                    || (p >= 2
                        && result[p - 2] == b't'
                        && result[p - 1] == b'r'
                        && result[p] == b'y')
                    || (p >= 4
                        && result[p - 4] == b'c'
                        && result[p - 3] == b'a'
                        && result[p - 2] == b't'
                        && result[p - 1] == b'c'
                        && result[p] == b'h')
                    || (p >= 6
                        && result[p - 6] == b'f'
                        && result[p - 5] == b'i'
                        && result[p - 4] == b'n'
                        && result[p - 3] == b'a'
                        && result[p - 2] == b'l'
                        && result[p - 1] == b'l'
                        && result[p] == b'y')
            } else {
                false
            };
            if is_block {
                // Ensure space before {
                if !result.last().map(|&b| b == b' ').unwrap_or(false) {
                    result.push(b' ');
                }
                result.push(b'{');
                result.push(b' ');
                result.push(b'}');
                i += 2;
                continue;
            }
        }

        result.push(ch);
        i += 1;
    }
    // SAFETY: we only push ASCII bytes
    Cow::Owned(unsafe { String::from_utf8_unchecked(result) })
}

/// Strip spaces after unary `~` operator: `(~ expr)` → `(~expr)`.
/// TypeScript always emits symbolic unary operators without trailing space.
pub(crate) fn normalize_unary_spacing(text: &str) -> Cow<'_, str> {
    // Quick check: any of `~ `, `! `, `- `, `+ ` would trigger work. One pass
    // for all four 2-byte patterns instead of four `str::contains` SIMD scans.
    let bytes = text.as_bytes();
    let has_unary_space = bytes
        .windows(2)
        .any(|w| matches!(w[0], b'~' | b'!' | b'-' | b'+') && w[1] == b' ');
    if !has_unary_space {
        return Cow::Borrowed(text);
    }
    let len = bytes.len();
    let mut result = Vec::with_capacity(len);
    let mut state = LineState::new();
    let mut i = 0;
    while i < len {
        let ch = bytes[i];
        // The unary-collapse rule only fires in code context.
        if state.in_code() {
            let is_unary_op = ch == b'~' || ch == b'!';
            let is_ambiguous_unary = ch == b'-' || ch == b'+';
            if (is_unary_op || is_ambiguous_unary) && i + 1 < len && bytes[i + 1] == b' ' {
                let prev = {
                    let mut p = result.len();
                    while p > 0 && result[p - 1] == b' ' {
                        p -= 1;
                    }
                    if p == 0 {
                        0
                    } else {
                        result[p - 1]
                    }
                };
                if is_ambiguous_unary && (prev == b'+' || prev == b'-') {
                    // `x++ + y` — the `+ ` after `++` is binary, leave alone.
                    result.push(ch);
                    let advance = state.advance(bytes, i);
                    debug_assert_eq!(advance, 1);
                    i += advance;
                    continue;
                }
                let is_unary_context = matches!(
                    prev,
                    0 | b'('
                        | b','
                        | b'='
                        | b'['
                        | b':'
                        | b';'
                        | b'{'
                        | b'!'
                        | b'~'
                        | b'?'
                        | b'|'
                        | b'&'
                        | b'^'
                        | b'*'
                        | b'/'
                        | b'%'
                        | b'<'
                        | b'>'
                );
                if is_unary_context {
                    if is_ambiguous_unary {
                        // Don't collapse if the next non-space is also `+`/`-`
                        // (would create `+++` / `---` ambiguity).
                        let mut j = i + 2;
                        while j < len && bytes[j] == b' ' {
                            j += 1;
                        }
                        if j < len && (bytes[j] == b'+' || bytes[j] == b'-') {
                            result.push(ch);
                            let advance = state.advance(bytes, i);
                            debug_assert_eq!(advance, 1);
                            i += advance;
                            continue;
                        }
                    }
                    // Emit operator, drop the trailing space, advance state
                    // for both consumed bytes.
                    result.push(ch);
                    state.advance(bytes, i);
                    state.advance(bytes, i + 1);
                    i += 2;
                    continue;
                }
            }
        }
        let advance = state.advance(bytes, i);
        result.extend_from_slice(&bytes[i..i + advance]);
        i += advance;
    }
    if result.len() == text.len() && result == text.as_bytes() {
        return Cow::Borrowed(text);
    }
    Cow::Owned(String::from_utf8(result).unwrap_or_else(|_| text.to_string()))
}

/// Like `collapse_consecutive_spaces` but JSX-aware: preserves whitespace in
/// JSX text content (between `>` and `<`/`{` at the top level).
/// Inside JSX tags and `{...}` expressions, consecutive spaces are collapsed.
pub(crate) fn collapse_consecutive_spaces_jsx(text: &str) -> Cow<'_, str> {
    if !text.contains("  ") {
        return Cow::Borrowed(text);
    }
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut result = Vec::with_capacity(len);
    let mut in_string: u8 = 0;
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    // JSX text content tracking:
    // in_tag: inside a JSX tag (between `<` and `>`)
    // brace_depth: depth of `{...}` JSX expression nesting
    // When !in_tag && brace_depth == 0, we're in JSX text content.
    let mut in_tag = false;
    let mut brace_depth: u32 = 0;
    let mut i = 0;
    while i < len {
        let ch = bytes[i];
        if in_line_comment {
            result.push(ch);
            if ch == b'\n' {
                in_line_comment = false;
            }
            i += 1;
            continue;
        }
        if in_block_comment {
            result.push(ch);
            if ch == b'*' && i + 1 < len && bytes[i + 1] == b'/' {
                result.push(bytes[i + 1]);
                in_block_comment = false;
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if in_string != 0 {
            result.push(ch);
            if closes_string(bytes, i, in_string) {
                in_string = 0;
            }
            i += 1;
            continue;
        }
        if ch == b'\'' || ch == b'"' || ch == b'`' {
            in_string = ch;
            result.push(ch);
            i += 1;
            continue;
        }
        if ch == b'/' && i + 1 < len {
            if bytes[i + 1] == b'/' {
                in_line_comment = true;
                result.push(ch);
                i += 1;
                continue;
            }
            if bytes[i + 1] == b'*' {
                in_block_comment = true;
                result.push(ch);
                i += 1;
                continue;
            }
        }
        // Track JSX structure
        if ch == b'<' && brace_depth == 0 {
            in_tag = true;
        } else if ch == b'>' && in_tag {
            in_tag = false;
        } else if ch == b'{' && !in_tag {
            brace_depth += 1;
        } else if ch == b'}' && brace_depth > 0 {
            brace_depth -= 1;
        }
        // In JSX text content (!in_tag && brace_depth == 0), preserve all whitespace.
        // In tags/expressions, collapse consecutive spaces (but preserve leading indent).
        let in_jsx_text = !in_tag && brace_depth == 0;
        if ch == b' ' && !result.is_empty() && *result.last().unwrap() == b' ' && !in_jsx_text {
            let is_leading = result.iter().rev().all(|&b| b == b' ')
                || result
                    .iter()
                    .rev()
                    .take_while(|&&b| b != b'\n')
                    .all(|&b| b == b' ');
            if !is_leading {
                i += 1;
                continue;
            }
        }
        result.push(ch);
        i += 1;
    }
    Cow::Owned(String::from_utf8(result).unwrap_or_else(|_| text.to_string()))
}

/// Remove padding immediately before the closing brace of an outer JSX
/// expression container (`attr={value }` -> `attr={value}`). Spaces inside a
/// nested object literal remain untouched.
pub(crate) fn normalize_jsx_outer_expr_closing_space(text: &str) -> Cow<'_, str> {
    if !text.contains(" }") {
        return Cow::Borrowed(text);
    }
    let bytes = text.as_bytes();
    let mut result = Vec::with_capacity(bytes.len());
    let mut brace_depth = 0u32;
    let mut in_string = 0u8;
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut i = 0usize;
    while i < bytes.len() {
        let ch = bytes[i];
        if in_line_comment {
            result.push(ch);
            if ch == b'\n' {
                in_line_comment = false;
            }
            i += 1;
            continue;
        }
        if in_block_comment {
            result.push(ch);
            if ch == b'*' && bytes.get(i + 1) == Some(&b'/') {
                result.push(b'/');
                in_block_comment = false;
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if in_string != 0 {
            result.push(ch);
            if closes_string(bytes, i, in_string) {
                in_string = 0;
            }
            i += 1;
            continue;
        }
        match ch {
            b'\'' | b'"' | b'`' => in_string = ch,
            b'/' if bytes.get(i + 1) == Some(&b'/') => in_line_comment = true,
            b'/' if bytes.get(i + 1) == Some(&b'*') => in_block_comment = true,
            b'{' => brace_depth += 1,
            b'}' => {
                if brace_depth == 1 {
                    while result.last() == Some(&b' ') || result.last() == Some(&b'\t') {
                        result.pop();
                    }
                }
                brace_depth = brace_depth.saturating_sub(1);
            }
            _ => {}
        }
        result.push(ch);
        i += 1;
    }
    if result == bytes {
        Cow::Borrowed(text)
    } else {
        Cow::Owned(String::from_utf8(result).unwrap_or_else(|_| text.to_string()))
    }
}

/// Normalize whitespace before semicolons: `foo ;` → `foo;`.
/// Handles both trailing ` ;` and interior ` ;` (e.g. `null ; }` → `null; }`).
pub(crate) fn normalize_trailing_semi(text: &str) -> Cow<'_, str> {
    fn should_collapse(before: &str) -> bool {
        let trimmed = before.trim_end();
        // Don't strip space after keywords that can appear bare in error recovery
        if trimmed.ends_with("return")
            || trimmed.ends_with("throw")
            || trimmed.ends_with("await")
            || trimmed.ends_with("void")
            || trimmed.ends_with("delete")
            || trimmed.ends_with("typeof")
            || trimmed.ends_with("using")
            || trimmed.ends_with("=>")
        {
            return false;
        }
        // Don't strip space when ending with an operator (missing RHS in error recovery)
        let last_ch = trimmed.as_bytes().last().copied().unwrap_or(0);
        if matches!(
            last_ch,
            b'+' | b'-'
                | b'*'
                | b'/'
                | b'%'
                | b'^'
                | b'&'
                | b'|'
                | b'~'
                | b'='
                | b'<'
                | b'>'
                | b'?'
                | b'!'
                | b','
                | b':'
        ) {
            // Exception: postfix ++ or -- should still collapse
            let tb = trimmed.as_bytes();
            let is_postfix_incr = (last_ch == b'+' || last_ch == b'-')
                && tb.len() >= 2
                && tb[tb.len() - 2] == last_ch;
            if !is_postfix_incr {
                return false;
            }
        }
        true
    }

    // Fast path: no ` ;` anywhere
    if !text.contains(" ;") {
        return Cow::Borrowed(text);
    }

    // Scan for ` ;` occurrences and collapse them, skipping strings/comments
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut result = Vec::with_capacity(len);
    let mut i = 0;
    let mut in_string: u8 = 0; // b'\'' or b'"' or b'`'
    let mut in_line_comment = false;
    let mut in_block_comment = false;

    while i < len {
        let ch = bytes[i];

        if in_line_comment {
            result.push(ch);
            if ch == b'\n' {
                in_line_comment = false;
            }
            i += 1;
            continue;
        }
        if in_block_comment {
            result.push(ch);
            if ch == b'*' && i + 1 < len && bytes[i + 1] == b'/' {
                result.push(b'/');
                i += 2;
                in_block_comment = false;
            } else {
                i += 1;
            }
            continue;
        }
        if in_string != 0 {
            result.push(ch);
            if ch == b'\\' && i + 1 < len {
                result.push(bytes[i + 1]);
                i += 2;
            } else {
                if ch == in_string {
                    in_string = 0;
                }
                i += 1;
            }
            continue;
        }

        // Check for string/comment starts
        if ch == b'\'' || ch == b'"' || ch == b'`' {
            in_string = ch;
            result.push(ch);
            i += 1;
            continue;
        }
        if ch == b'/' && i + 1 < len {
            if bytes[i + 1] == b'/' {
                in_line_comment = true;
                result.push(ch);
                i += 1;
                continue;
            }
            if bytes[i + 1] == b'*' {
                in_block_comment = true;
                result.push(ch);
                i += 1;
                continue;
            }
        }

        // Check for ` ;` pattern
        if ch == b' ' && i + 1 < len && bytes[i + 1] == b';' {
            // Check if we should collapse
            let before = std::str::from_utf8(&bytes[..i]).unwrap_or("");
            if should_collapse(before) {
                // Skip the space, next iteration will emit `;`
                i += 1;
                continue;
            }
        }

        result.push(ch);
        i += 1;
    }

    if result.len() == text.len() && result == text.as_bytes() {
        return Cow::Borrowed(text);
    }
    Cow::Owned(String::from_utf8(result).unwrap_or_else(|_| text.to_string()))
}

/// Strip trailing whitespace before `)` and `]` in source-copied text.
/// TypeScript normalizes `expr )` to `expr)` and `expr ]` to `expr]`.
/// Only applies outside of string literals and comments.
pub(crate) fn normalize_close_paren(text: &str) -> Cow<'_, str> {
    let bytes = text.as_bytes();
    let len = bytes.len();
    // Fast path: the strip/rewrite rules here trigger on `)`, `]`, `;`, `[`, or
    // `(` (strip-space-before / strip-trailing-comma / strip-space-after-open),
    // and the JSX self-closing rule triggers on `/>` (space insertion/removal
    // around the slash). If none of those are present, the output is
    // byte-identical to the input. Text lacking ALL of them is rare for real
    // source lines but very common for plain text fragments passed through this
    // normalizer (e.g., comment bodies, simple identifiers). The `/>` check only
    // runs when no bracket/semicolon byte is found (short-circuit), so it costs
    // nothing on typical statement text.
    if !bytes
        .iter()
        .any(|&b| matches!(b, b')' | b']' | b';' | b'[' | b'('))
        && !text.contains("/>")
    {
        return Cow::Borrowed(text);
    }
    let mut result: Vec<u8> = Vec::with_capacity(len);
    let mut state = LineState::new();
    let mut i = 0;
    while i < len {
        let ch = bytes[i];
        // Regex-start detection: when in code and we see `/` not followed by
        // `/` or `*` (i.e., not a comment-open), check the result-buffer tail
        // to disambiguate division from a regex literal. If a regex literal
        // opens, push `/` and manually flip `state.in_regex` (LineState's
        // `advance` doesn't auto-detect this — it's a context-dependent
        // decision based on the *output* buffer's tail, which reflects any
        // earlier rule edits).
        if state.in_code()
            && ch == b'/'
            && i + 1 < len
            && bytes[i + 1] != b'/'
            && bytes[i + 1] != b'*'
        {
            let prev_non_ws = result.iter().rposition(|&b| b != b' ' && b != b'\t');
            let (prev_byte, prev_word): (Option<u8>, &[u8]) = match prev_non_ws {
                None => (None, b""),
                Some(p) => {
                    let pc = result[p];
                    if pc.is_ascii_alphanumeric() || pc == b'_' || pc == b'$' {
                        let word_start = result[..=p]
                            .iter()
                            .rposition(|b| !b.is_ascii_alphanumeric() && *b != b'_' && *b != b'$')
                            .map(|pos| pos + 1)
                            .unwrap_or(0);
                        (Some(pc), &result[word_start..=p])
                    } else {
                        (Some(pc), b"")
                    }
                }
            };
            if LineState::regex_start_after(prev_byte, prev_word) {
                result.push(b'/');
                state.in_regex = true;
                state.in_regex_class = false;
                i += 1;
                continue;
            }
            // JSX self-closing `/>` — normalize spaces before `/`. Same
            // result-tail manipulation as before.
            if bytes[i + 1] == b'>' && !result.is_empty() {
                let mut count = 0;
                for j in (0..result.len()).rev() {
                    if result[j] == b' ' {
                        count += 1;
                    } else {
                        break;
                    }
                }
                let before = result.len() - count;
                if before > 0 && result[before - 1] != b'\n' && result[before - 1] != b'\r' {
                    let prev = result[before - 1];
                    if prev.is_ascii_alphanumeric() || prev == b'-' || prev == b':' || prev == b'.'
                    {
                        let mut scan = before - 1;
                        while scan > 0
                            && (result[scan - 1].is_ascii_alphanumeric()
                                || result[scan - 1] == b'-'
                                || result[scan - 1] == b':'
                                || result[scan - 1] == b'.'
                                || result[scan - 1] == b'_')
                        {
                            scan -= 1;
                        }
                        let is_tag_name = scan > 0 && result[scan - 1] == b'<';
                        if is_tag_name && count == 0 {
                            result.push(b' ');
                        }
                    } else if count > 0 {
                        result.truncate(before);
                    }
                }
            }
        }
        // The remaining pre-push rules only fire on plain code bytes. Inside
        // regex / strings / comments, `state.in_code()` is false and these
        // are skipped — the byte just gets appended via `state.advance`
        // below, mirroring the original normalizer's per-state branches.
        if state.in_code() && (ch == b')' || ch == b']') && !result.is_empty() {
            // Strip trailing spaces before ) or ] on the same line.
            // Don't strip across newline boundaries (that would eat indentation).
            let mut count = 0;
            for j in (0..result.len()).rev() {
                if result[j] == b' ' {
                    count += 1;
                } else {
                    break;
                }
            }
            // Only strip if there's a non-newline char before the spaces
            // (i.e., these are trailing spaces, not line indentation).
            // Don't strip when the char before the spaces is a comma and
            // the original source had a space between `,` and `)` — this
            // is a comma expression with empty right operand like `(ANY, )`.
            if count > 0 {
                let before = result.len() - count;
                let is_comma_expr_space = ch == b')' && before > 0 && result[before - 1] == b',';
                if before > 0
                    && result[before - 1] != b'\n'
                    && result[before - 1] != b'\r'
                    && !is_comma_expr_space
                {
                    result.truncate(before);
                }
            }
            // NOTE: `[await]` → `[await ]` normalization intentionally omitted;
            // too few tests benefit and false positives in array indexing.
            // Strip trailing comma before ) — TypeScript strips trailing commas
            // in function call arguments and parameter lists.
            // Don't strip trailing commas before ] (array elisions need them).
            // Don't strip when the original had a space after the comma
            // (comma expression with empty right operand).
            if ch == b')' && !result.is_empty() && result[result.len() - 1] == b',' {
                // Check if original source had space between , and )
                let orig_had_space = i > 0 && bytes[i - 1] == b' ';
                if !orig_had_space {
                    result.pop();
                }
            }
        }
        // Strip trailing spaces before `;` on the same line, but not after
        // `=>` (error recovery: `() => ;`) or keyword-unary operators with
        // missing operands (`void ;`, `delete ;`, `typeof ;`).
        if state.in_code() && ch == b';' && !result.is_empty() {
            let mut count = 0;
            for j in (0..result.len()).rev() {
                if result[j] == b' ' {
                    count += 1;
                } else {
                    break;
                }
            }
            if count > 0 {
                let before = result.len() - count;
                // Check if the word before the spaces is a keyword that needs
                // the space preserved (void, delete, typeof, return, throw).
                let preserve = if before > 0 {
                    let prev_byte = result[before - 1];
                    if prev_byte == b'>' || prev_byte == b'\n' || prev_byte == b'\r' {
                        true
                    } else if prev_byte.is_ascii_alphabetic() {
                        // Extract the word ending at `before - 1`
                        let word_start = result[..before]
                            .iter()
                            .rposition(|b| !b.is_ascii_alphabetic())
                            .map(|p| p + 1)
                            .unwrap_or(0);
                        let word = &result[word_start..before];
                        word == b"void"
                            || word == b"delete"
                            || word == b"typeof"
                            || word == b"return"
                            || word == b"throw"
                            || word == b"await"
                            || word == b"yield"
                            || word == b"using"
                    } else if matches!(
                        prev_byte,
                        b'+' | b'-'
                            | b'*'
                            | b'/'
                            | b'%'
                            | b'^'
                            | b'&'
                            | b'|'
                            | b'~'
                            | b'='
                            | b'!'
                            | b'?'
                            | b','
                    ) {
                        // Preserve space before ; after binary operator
                        // (error recovery: missing RHS), but NOT after:
                        // - postfix ++ or -- (x++ ; → x++;)
                        // - block comment close */ (/*comment*/ ; → /*comment*/;)
                        if (prev_byte == b'+' || prev_byte == b'-')
                            && before >= 2
                            && result[before - 2] == prev_byte
                        {
                            false // postfix ++/--
                        } else if (prev_byte == b'/' || prev_byte == b'*')
                            && before >= 2
                            && result[before - 2] == b'*'
                        {
                            false // block comment close */
                        } else {
                            true
                        }
                    } else {
                        false
                    }
                } else {
                    false
                };
                if !preserve {
                    result.truncate(before);
                }
            }
        }
        // Strip trailing spaces before commas, but only when preceded by an
        // identifier-like char (alphanumeric, _, $, ), ]).  Preserve spaces
        // before commas after other commas (elisions) or after block comments.
        if state.in_code() && ch == b',' && !result.is_empty() {
            let mut count = 0;
            for j in (0..result.len()).rev() {
                if result[j] == b' ' {
                    count += 1;
                } else {
                    break;
                }
            }
            if count > 0 {
                let before = result.len() - count;
                if before > 0 {
                    let prev = result[before - 1];
                    if prev.is_ascii_alphanumeric()
                        || prev == b'_'
                        || prev == b'$'
                        || prev == b')'
                        || prev == b']'
                        || prev == b'"'
                        || prev == b'\''
                        || prev == b'`'
                        || prev == b'}'
                    {
                        result.truncate(before);
                    }
                }
            }
        }
        // Capture whether the current byte is `[` or `(` in code context
        // *before* `state.advance` mutates state — the post-push "skip
        // spaces after open bracket" rule needs this signal.
        let was_open_bracket_in_code = state.in_code() && (ch == b'[' || ch == b'(');
        let advance = state.advance(bytes, i);
        result.extend_from_slice(&bytes[i..i + advance]);
        i += advance;
        // After pushing `[` or `(`, skip spaces that follow on the same
        // line. Normalizes `[ this]` → `[this]`.
        if was_open_bracket_in_code {
            let start = i;
            while i < len && bytes[i] == b' ' {
                state.advance(bytes, i);
                i += 1;
            }
            let should_keep = i >= len
                || bytes[i] == b'\n'
                || bytes[i] == b'\r'
                || (bytes[i] == b'/' && i + 1 < len && bytes[i + 1] == b'*');
            if should_keep {
                result.extend_from_slice(&bytes[start..i]);
            }
        }
    }
    // If no changes were made, return borrowed to avoid allocation overhead downstream.
    if result.len() == text.len() && result == text.as_bytes() {
        return Cow::Borrowed(text);
    }
    Cow::Owned(String::from_utf8(result).unwrap_or_else(|_| text.to_string()))
}

/// Normalize `async ident =>` to `async (ident) =>`.
/// TypeScript always emits parentheses around single-parameter async arrows.
pub(crate) fn normalize_async_arrow_parens(text: &str) -> Cow<'_, str> {
    if !text.contains("async ") {
        return Cow::Borrowed(text);
    }
    // Match: `async <identifier> =>`
    // We use a simple regex-like approach: find "async " followed by a JS identifier, then " =>"
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut result = String::with_capacity(len + 4);
    let mut i = 0;
    while i < len {
        if i + 7 < len && text.get(i..i + 6) == Some("async ") {
            // Check that the char before 'async' is not alphanumeric (word boundary)
            let at_word_boundary =
                i == 0 || !bytes[i - 1].is_ascii_alphanumeric() && bytes[i - 1] != b'_';
            if at_word_boundary {
                // Read the identifier after "async "
                let start = i + 6;
                let mut j = start;
                // Skip whitespace after "async"
                while j < len && bytes[j] == b' ' {
                    j += 1;
                }
                // Check if next char starts an identifier (not '(' or '{' etc)
                if j < len
                    && (bytes[j].is_ascii_alphabetic() || bytes[j] == b'_' || bytes[j] == b'$')
                {
                    let ident_start = j;
                    // Read identifier chars
                    while j < len
                        && (bytes[j].is_ascii_alphanumeric()
                            || bytes[j] == b'_'
                            || bytes[j] == b'$')
                    {
                        j += 1;
                    }
                    let ident_end = j;
                    // Skip whitespace after identifier
                    while j < len && bytes[j] == b' ' {
                        j += 1;
                    }
                    // Check for =>
                    if j + 1 < len && bytes[j] == b'=' && bytes[j + 1] == b'>' {
                        // Found pattern: async ident =>
                        result.push_str("async (");
                        result.push_str(&text[ident_start..ident_end]);
                        result.push_str(") ");
                        i = j; // continue from =>
                        continue;
                    }
                }
            }
        }
        let ch = text[i..].chars().next().unwrap();
        result.push(ch);
        i += ch.len_utf8();
    }
    Cow::Owned(result)
}

/// Ensure a space before a backtick when preceded by `)`, `]`, or a word char
/// in tagged template expressions.  TypeScript's structured emit always adds a
/// space between the tag expression and the template literal.  Only applies to
/// the *opening* backtick of a template (not closing backticks inside one).
pub(crate) fn normalize_tagged_template_space(text: &str) -> Cow<'_, str> {
    if !text.contains('`') {
        return Cow::Borrowed(text);
    }
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut result = Vec::with_capacity(len + 4);
    let mut in_string: u8 = 0;
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    // Track template nesting: stack of booleans.
    // Each entry represents a `${}` expression level.
    // When in_template_depth > 0, we're inside a template literal.
    let mut template_depth: u32 = 0;
    // Stack tracking whether each `{` is a template expression brace
    let mut template_brace_stack: Vec<bool> = Vec::new();
    let mut i = 0;
    while i < len {
        let ch = bytes[i];
        if in_line_comment {
            if ch == b'\n' {
                in_line_comment = false;
            }
            result.push(ch);
            i += 1;
            continue;
        }
        if in_block_comment {
            if ch == b'*' && i + 1 < len && bytes[i + 1] == b'/' {
                in_block_comment = false;
                result.push(ch);
                result.push(bytes[i + 1]);
                i += 2;
                continue;
            }
            result.push(ch);
            i += 1;
            continue;
        }
        if in_string != 0 {
            // Handle template `${` inside template string
            if in_string == b'`' && ch == b'$' && i + 1 < len && bytes[i + 1] == b'{' {
                result.push(b'$');
                result.push(b'{');
                template_brace_stack.push(true);
                in_string = 0;
                i += 2;
                continue;
            }
            if closes_string(bytes, i, in_string) {
                in_string = 0;
                if ch == b'`' {
                    template_depth = template_depth.saturating_sub(1);
                }
            }
            result.push(ch);
            i += 1;
            continue;
        }
        if ch == b'\'' || ch == b'"' {
            in_string = ch;
            result.push(ch);
            i += 1;
            continue;
        }
        if ch == b'/' && i + 1 < len {
            if bytes[i + 1] == b'/' {
                in_line_comment = true;
                result.push(ch);
                i += 1;
                continue;
            }
            if bytes[i + 1] == b'*' {
                in_block_comment = true;
                result.push(ch);
                i += 1;
                continue;
            }
        }
        // Track braces for template expression nesting
        if ch == b'{' {
            let is_template = i > 0 && bytes[i - 1] == b'$';
            if !is_template {
                template_brace_stack.push(false);
            }
            // template `${` is already handled above
            result.push(ch);
            i += 1;
            continue;
        }
        if ch == b'}' {
            let is_template = template_brace_stack.pop().unwrap_or(false);
            if is_template {
                // Re-enter template string mode
                in_string = b'`';
            }
            result.push(ch);
            i += 1;
            continue;
        }
        // When we hit a backtick in code context, it opens a new template.
        // Only insert a space before the *opening* backtick of a tagged template.
        if ch == b'`' {
            template_depth += 1;
            // Opening backtick – add space if preceded by tag-like char.
            if !result.is_empty() {
                let prev = result[result.len() - 1];
                if prev == b')'
                    || prev == b']'
                    || prev.is_ascii_alphanumeric()
                    || prev == b'_'
                    || prev == b'$'
                {
                    result.push(b' ');
                }
            }
            in_string = b'`';
            result.push(ch);
            i += 1;
            continue;
        }
        result.push(ch);
        i += 1;
    }
    // SAFETY: we only inserted ASCII bytes into valid UTF-8 input.
    Cow::Owned(unsafe { String::from_utf8_unchecked(result) })
}

/// Ensure a space after `*/` when directly followed by a word character.
/// TypeScript's structured emit adds a space between block comment endings
/// and identifiers (e.g. `/*3*/point` → `/*3*/ point`), except in member
/// access context (e.g. `obj./*c*/prop` stays as-is).
pub(crate) fn normalize_comment_word_space(text: &str) -> Cow<'_, str> {
    let bytes = text.as_bytes();
    let len = bytes.len();
    if !text.contains("*/") {
        return Cow::Borrowed(text);
    }
    let mut result = Vec::with_capacity(len + 16);
    let mut state = LineState::new();
    let mut i = 0;
    while i < len {
        let was_in_block = state.in_block_comment;
        let advance = state.advance(bytes, i);
        result.extend_from_slice(&bytes[i..i + advance]);
        i += advance;
        // Rule: when we just exited a block comment via `*/`, consider
        // inserting a space before the following byte and rewriting any
        // following whitespace based on the surrounding-context heuristic.
        if was_in_block && !state.in_block_comment && i < len {
            // Locate the `/*` that opened this comment within `result`
            // (skipping the trailing `*/` we just pushed).
            let rlen = result.len();
            let mut cs = rlen.saturating_sub(2);
            while cs > 0 {
                if result[cs] == b'/' && cs + 1 < rlen && result[cs + 1] == b'*' {
                    break;
                }
                cs -= 1;
            }
            let mut before_idx = if cs > 0 { cs - 1 } else { 0 };
            while before_idx > 0 && result[before_idx] == b' ' {
                before_idx -= 1;
            }
            let dot_before = cs > 0 && result[before_idx] == b'.';
            let paren_before = cs > 0 && result[before_idx] == b'(' && {
                if before_idx == 0 {
                    true
                } else {
                    let pre_paren = result[before_idx - 1];
                    !pre_paren.is_ascii_alphanumeric()
                        && pre_paren != b'_'
                        && pre_paren != b'$'
                        && pre_paren != b')'
                        && pre_paren != b']'
                }
            };

            let after = bytes[i];
            if after.is_ascii_alphanumeric() || after == b'_' || after == b'$' {
                if !dot_before && !paren_before {
                    result.push(b' ');
                }
            } else if after == b'(' && !dot_before {
                if !paren_before {
                    result.push(b' ');
                }
                if !paren_before && i + 2 < len && bytes[i + 1] == b'/' && bytes[i + 2] == b'*' {
                    // Nested JSDoc cast: emit `(` + space, then advance state
                    // for the consumed `(` so block-comment tracking stays
                    // correct when we re-enter the loop on the inner `/*`.
                    result.push(b'(');
                    result.push(b' ');
                    state.advance(bytes, i);
                    i += 1;
                }
            } else if after == b' ' {
                let bracket_before = cs > 0 && result[before_idx] == b'[';
                let start = i;
                while i < len && bytes[i] == b' ' {
                    state.advance(bytes, i);
                    i += 1;
                }
                if i < len {
                    let next = bytes[i];
                    if (next == b'"' || next == b'\'') && (bracket_before || paren_before) {
                        // Strip spaces before string delimiters in element
                        // access `[/*c*/ "key"]` or grouping paren context.
                    } else if paren_before
                        && (next.is_ascii_alphanumeric()
                            || next == b'_'
                            || next == b'$'
                            || next == b'(')
                    {
                        // Grouping paren context: strip spaces before word chars.
                    } else {
                        // Restore spaces.
                        result.extend_from_slice(&bytes[start..i]);
                    }
                } else {
                    result.extend_from_slice(&bytes[start..i]);
                }
            }
        }
    }
    // SAFETY: we only push ASCII bytes
    Cow::Owned(unsafe { String::from_utf8_unchecked(result) })
}

/// Strip trailing `// ...` comments from lines that end with `{`.
/// TypeScript's structured emit doesn't preserve trailing comments on
/// function/method opening brace lines.
pub(crate) fn strip_trailing_comment_on_brace_line(text: &str) -> Cow<'_, str> {
    if !text.contains("//") {
        return Cow::Borrowed(text);
    }

    /// Check if a single line should have its trailing comment stripped after `{`.
    /// Returns Some(truncation_pos) if it should be truncated, None otherwise.
    fn find_strip_pos(line: &str) -> Option<usize> {
        let trimmed = line.trim();
        let brace_pos = trimmed.rfind('{')?;
        // Skip template expression `${`.
        if brace_pos > 0 && trimmed.as_bytes()[brace_pos - 1] == b'$' {
            return None;
        }
        let after_brace = &trimmed[brace_pos + 1..];
        let comment_pos = after_brace.find("//")?;
        let between = &after_brace[..comment_pos];
        if !between.trim().is_empty() {
            return None;
        }
        let before_brace = trimmed[..brace_pos].trim_end();
        let is_control_flow = (before_brace.ends_with(')')
            && (before_brace.contains("if ")
                || before_brace.contains("if(")
                || before_brace.contains("for ")
                || before_brace.contains("for(")
                || before_brace.contains("while ")
                || before_brace.contains("while(")
                || before_brace.contains("switch ")
                || before_brace.contains("switch(")
                || before_brace.contains("with ")
                || before_brace.contains("with(")))
            || before_brace == "else"
            || before_brace.ends_with(" else")
            || before_brace == "try"
            || before_brace.ends_with(" try")
            || before_brace == "finally"
            || before_brace.ends_with(" finally")
            || before_brace.contains("catch ")
            || before_brace.contains("catch(");
        if is_control_flow {
            return None;
        }
        line.rfind('{').map(|pos| pos + 1)
    }

    // Fast path for single-line text (common case from copy_expr_span).
    if !text.contains('\n') {
        return match find_strip_pos(text) {
            Some(pos) => Cow::Owned(text[..pos].to_string()),
            None => Cow::Borrowed(text),
        };
    }

    // Multi-line: process each line.
    let mut any_changed = false;
    let result_lines: Vec<Cow<str>> = text
        .lines()
        .map(|line| match find_strip_pos(line) {
            Some(pos) => {
                any_changed = true;
                Cow::Owned(line[..pos].to_string())
            }
            None => Cow::Borrowed(line),
        })
        .collect();
    if !any_changed {
        return Cow::Borrowed(text);
    }
    let joined: String = result_lines
        .iter()
        .map(|l| l.as_ref())
        .collect::<Vec<_>>()
        .join("\n");
    Cow::Owned(joined)
}

/// Strip comments from source-copied text when `removeComments` is true.
/// Handles both `// line comments` and `/* block comments */`.
/// Removes entire lines that are only comments. Strips inline comments.
pub(crate) fn strip_source_comments(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut in_string: u8 = 0;

    let mut chars = text.char_indices().peekable();
    while let Some((i, ch)) = chars.next() {
        // Track string literals
        if in_string == 0 && (ch == '\'' || ch == '"' || ch == '`') {
            in_string = ch as u8;
            result.push(ch);
            continue;
        }
        if in_string != 0 {
            result.push(ch);
            if ch as u8 == in_string && (i == 0 || text.as_bytes()[i - 1] != b'\\') {
                in_string = 0;
            }
            continue;
        }

        // Line comment: strip to end of line
        if ch == '/' && chars.peek().map(|(_, c)| *c) == Some('/') {
            // Strip everything until newline (keep the newline)
            for (_, c) in chars.by_ref() {
                if c == '\n' {
                    // Check if the line was only whitespace + comment
                    // If so, remove the line entirely
                    let line_start = result.rfind('\n').map(|p| p + 1).unwrap_or(0);
                    if result[line_start..].trim().is_empty() {
                        result.truncate(line_start);
                    } else {
                        // There's code before the comment — keep line, add newline
                        // Trim trailing spaces before the comment
                        let trimmed = result.trim_end_matches(' ');
                        let new_len = trimmed.len();
                        result.truncate(new_len);
                        result.push('\n');
                    }
                    break;
                }
            }
            continue;
        }

        // Block comment: strip
        if ch == '/' && chars.peek().map(|(_, c)| *c) == Some('*') {
            chars.next(); // consume *
                          // Find end of block comment
            let mut prev_star = false;
            for (_, c) in chars.by_ref() {
                if c == '/' && prev_star {
                    break;
                }
                prev_star = c == '*';
            }
            // After stripping block comment, check if current line is now empty
            // If the block comment was on its own line, remove the empty line
            if let Some(&(_next_i, next_ch)) = chars.peek() {
                if next_ch == '\n' || next_ch == '\r' {
                    let line_start = result.rfind('\n').map(|p| p + 1).unwrap_or(0);
                    if result[line_start..].trim().is_empty() {
                        result.truncate(line_start);
                        // Consume the newline
                        chars.next();
                        if next_ch == '\r' {
                            if chars.peek().map(|(_, c)| *c) == Some('\n') {
                                chars.next();
                            }
                        }
                        continue;
                    }
                }
            }
            continue;
        }

        result.push(ch);
    }

    // Clean up: remove trailing empty lines and extra whitespace
    result
}

/// Normalize yield spacing in source-copied text that contains generator
/// function bodies. TypeScript's structured emit adds a space between
/// `yield` and `(` for yield expressions: `yield(foo)` → `yield (foo)`.
/// Also normalizes `yield*;` → `yield* ;` for empty delegate yields.
/// Only applies when the text contains `function*` (generator context).
pub(crate) fn normalize_yield_spacing(text: &str) -> Cow<'_, str> {
    if !text.contains("function*") {
        return Cow::Borrowed(text);
    }
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut result = Vec::with_capacity(len + 8);
    let mut in_string: u8 = 0;
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut i = 0;
    while i < len {
        let ch = bytes[i];
        // Track string/comment context
        if in_line_comment {
            if ch == b'\n' {
                in_line_comment = false;
            }
            result.push(ch);
            i += 1;
            continue;
        }
        if in_block_comment {
            if ch == b'*' && i + 1 < len && bytes[i + 1] == b'/' {
                in_block_comment = false;
                result.push(ch);
                result.push(bytes[i + 1]);
                i += 2;
            } else {
                result.push(ch);
                i += 1;
            }
            continue;
        }
        if in_string != 0 {
            if ch == b'\\' && i + 1 < len {
                result.push(ch);
                result.push(bytes[i + 1]);
                i += 2;
                continue;
            }
            if ch == in_string {
                in_string = 0;
            }
            result.push(ch);
            i += 1;
            continue;
        }
        if ch == b'\'' || ch == b'"' || ch == b'`' {
            in_string = ch;
            result.push(ch);
            i += 1;
            continue;
        }
        if ch == b'/' && i + 1 < len {
            if bytes[i + 1] == b'/' {
                in_line_comment = true;
                result.push(ch);
                i += 1;
                continue;
            }
            if bytes[i + 1] == b'*' {
                in_block_comment = true;
                result.push(ch);
                i += 1;
                continue;
            }
        }
        // Check for `yield(` at word boundary → `yield (`
        // But NOT when `yield` is used as a generator function name (e.g. `function* yield()`)
        if ch == b'y'
            && i + 5 < len
            && &bytes[i..i + 5] == b"yield"
            && bytes[i + 5] == b'('
            && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric() && bytes[i - 1] != b'_')
        {
            // Check if `yield` is a function name by looking at previous non-ws in result
            let prev_non_ws = result.iter().rposition(|&b| b != b' ' && b != b'\t');
            let is_fn_name = prev_non_ws.is_some_and(|p| result[p] == b'*');
            if !is_fn_name {
                result.extend_from_slice(b"yield ");
                i += 5; // next iteration will push `(`
                continue;
            }
        }
        // Check for `yield*;` → `yield* ;`
        if ch == b'y'
            && i + 6 < len
            && &bytes[i..i + 6] == b"yield*"
            && bytes[i + 6] == b';'
            && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric() && bytes[i - 1] != b'_')
        {
            result.extend_from_slice(b"yield* ");
            i += 6; // next iteration will push `;`
            continue;
        }
        result.push(ch);
        i += 1;
    }
    // SAFETY: input was valid UTF-8 and we only inserted ASCII
    Cow::Owned(unsafe { String::from_utf8_unchecked(result) })
}

/// Normalize `import (` → `import(` for dynamic import calls.
/// TypeScript removes the space between `import` and `(` in the emit.
pub(crate) fn normalize_import_call_spacing(text: &str) -> Cow<'_, str> {
    if !text.contains("import ") {
        return Cow::Borrowed(text);
    }
    let bytes = text.as_bytes();
    let len = bytes.len();
    let pat = b"import ";
    let mut result = Vec::with_capacity(len);
    let mut state = LineState::new();
    let mut i = 0;
    while i < len {
        let ch = bytes[i];
        // Match `import +(` only in code context — at a word boundary, with
        // any number of spaces between `import` and `(`. Collapse the run of
        // spaces and skip `i` past them.
        if state.in_code()
            && ch == b'i'
            && i + pat.len() < len
            && &bytes[i..i + pat.len()] == pat
            && (i == 0
                || !(bytes[i - 1].is_ascii_alphanumeric()
                    || bytes[i - 1] == b'_'
                    || bytes[i - 1] == b'.'))
        {
            let mut j = i + pat.len();
            while j < len && bytes[j] == b' ' {
                j += 1;
            }
            if j < len && bytes[j] == b'(' && j > i + pat.len() - 1 {
                result.extend_from_slice(b"import");
                // Walk LineState forward over the bytes we just consumed
                // (`import` + spaces) so subsequent string/comment tracking
                // stays accurate. `import` is plain ASCII identifier text;
                // each byte advances state by 1.
                let mut k = i;
                while k < j {
                    state.advance(bytes, k);
                    k += 1;
                }
                i = j;
                continue;
            }
        }
        let advance = state.advance(bytes, i);
        result.extend_from_slice(&bytes[i..i + advance]);
        i += advance;
    }
    if result.len() == text.len() && result == text.as_bytes() {
        return Cow::Borrowed(text);
    }
    Cow::Owned(String::from_utf8(result).unwrap_or_else(|_| text.to_string()))
}

/// Strip space before `[` in element access expressions.
/// TypeScript normalizes `ident [expr]` → `ident[expr]` for computed member access
/// but our source-copy preserves the original spacing.
/// Only strip when `[` is preceded by an identifier char, `)`, or `]` (element access context),
/// not when preceded by `=`, `,`, `(`, `{`, or keyword boundaries (array literal context).
pub(crate) fn normalize_element_access_spacing(text: &str) -> Cow<'_, str> {
    let bytes = text.as_bytes();
    let len = bytes.len();
    if len < 3 {
        return Cow::Borrowed(text);
    }
    let mut result: Option<Vec<u8>> = None;
    let mut in_string: u8 = 0;
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut i = 0;
    while i < len {
        let ch = bytes[i];
        if in_line_comment {
            if let Some(ref mut r) = result {
                r.push(ch);
            }
            if ch == b'\n' {
                in_line_comment = false;
            }
            i += 1;
            continue;
        }
        if in_block_comment {
            if let Some(ref mut r) = result {
                r.push(ch);
            }
            if ch == b'*' && i + 1 < len && bytes[i + 1] == b'/' {
                in_block_comment = false;
                if let Some(ref mut r) = result {
                    r.push(bytes[i + 1]);
                }
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if in_string != 0 {
            if let Some(ref mut r) = result {
                r.push(ch);
            }
            if ch == b'\\' && i + 1 < len {
                if let Some(ref mut r) = result {
                    r.push(bytes[i + 1]);
                }
                i += 2;
            } else {
                if ch == in_string {
                    in_string = 0;
                }
                i += 1;
            }
            continue;
        }
        match ch {
            b'"' | b'\'' | b'`' => {
                in_string = ch;
            }
            b'/' if i + 1 < len && bytes[i + 1] == b'/' => {
                in_line_comment = true;
            }
            b'/' if i + 1 < len && bytes[i + 1] == b'*' => {
                in_block_comment = true;
            }
            b' ' if i + 1 < len && bytes[i + 1] == b'[' => {
                let prev = if i > 0 { bytes[i - 1] } else { 0 };
                let is_elem_access = prev.is_ascii_alphanumeric()
                    || prev == b'_'
                    || prev == b'$'
                    || prev == b')'
                    || prev == b']';
                if is_elem_access {
                    // Check if the word before the space is a JS keyword —
                    // keywords like `of`, `in`, `var`, `let`, `new` etc. need
                    // the space before `[` preserved (it's an array literal, not
                    // element access).
                    let word_start = {
                        let mut s = i;
                        while s > 0
                            && (bytes[s - 1].is_ascii_alphanumeric()
                                || bytes[s - 1] == b'_'
                                || bytes[s - 1] == b'$')
                        {
                            s -= 1;
                        }
                        s
                    };
                    let word = &text[word_start..i];
                    let is_keyword = matches!(
                        word,
                        "of" | "in"
                            | "var"
                            | "let"
                            | "const"
                            | "new"
                            | "return"
                            | "case"
                            | "delete"
                            | "typeof"
                            | "void"
                            | "throw"
                            | "instanceof"
                            | "yield"
                            | "await"
                            | "import"
                            | "export"
                            | "for"
                            | "if"
                            | "while"
                            | "else"
                            | "do"
                            | "switch"
                            | "with"
                            | "try"
                            | "catch"
                            | "finally"
                            | "using"
                    );
                    if !is_keyword {
                        if result.is_none() {
                            result = Some(bytes[..i].to_vec());
                        }
                        i += 1;
                        continue;
                    }
                }
            }
            _ => {}
        }
        if let Some(ref mut r) = result {
            r.push(ch);
        }
        i += 1;
    }
    match result {
        Some(r) => Cow::Owned(String::from_utf8(r).unwrap_or_else(|_| text.to_string())),
        None => Cow::Borrowed(text),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Curated corpus of statement/expression text snippets that exercise
    /// every normalizer in the chain. Used by `chain_v2_matches_chain_v1` to
    /// keep the in-progress single-pass driver byte-for-byte compatible with
    /// the existing multi-pass chain.
    ///
    /// Categories (roughly in order):
    ///  - identifiers / no-op inputs (should return Borrowed)
    ///  - simple statements with declarations
    ///  - object literals (brace spacing)
    ///  - for/while/if headers (keyword-paren)
    ///  - ternary and label colons
    ///  - close-paren / close-bracket spacing
    ///  - unary operators
    ///  - import call spacing
    ///  - comment-adjacent word spacing
    ///  - regex literals (must NOT normalize content)
    ///  - strings and templates (must NOT normalize content)
    ///  - JSX-ish tag fragments
    ///  - parser-recovery shapes (empty comma operands, etc.)
    ///  - tab + CRLF inputs (CRLF and `\u{0085}`/`\u{00A0}` are handled
    ///    upstream — the chain itself still has to walk past tabs cleanly)
    ///  - multi-line spans with templates spanning lines
    const CHAIN_CORPUS: &[&str] = &[
        // No-op inputs — exercise the all-Borrowed fast paths.
        "",
        " ",
        "x",
        "foo",
        "abc123",
        "foo.bar",
        // Simple statements.
        "let x = 1;",
        "const y = foo();",
        "var z = a + b;",
        "return x;",
        "throw new Error(msg);",
        // Object/array literals — brace_spacing rules.
        "{a:1}",
        "{ a: 1 }",
        "{a:1,b:2}",
        "{ a: 1, b: 2 }",
        "{}",
        "{ }",
        "[]",
        "[ ]",
        "[1,2,3]",
        "[ 1, 2, 3 ]",
        // for/while/if/switch headers — keyword_paren spacing.
        "for(let i=0;i<n;i++){}",
        "for (let i = 0; i < n; i++) {}",
        "for (let x of arr) {}",
        "for(let x in obj){}",
        "while(true){}",
        "if(x){}",
        "if (x) {}",
        "switch(v){}",
        "do{}while(x);",
        // Ternary / label / property colons.
        "a ? b : c",
        "a?b:c",
        "x = c ? a : b;",
        "foo: while (true) break foo;",
        // Close-paren / close-bracket — close_paren rules.
        "foo( a )",
        "foo(a, )",
        "arr[ 0 ]",
        "arr[0 ]",
        "[ a, b, ]",
        // Unary operators — unary_spacing.
        "- x",
        "-x",
        "! x",
        "!x",
        "~ x",
        "+ x",
        "return - 1;",
        "throw + x;",
        // Import call.
        "import (\"./mod.js\")",
        "import(\"./mod.js\")",
        "await import (m)",
        // Comment + adjacent word — comment_word_space.
        "/*x*/y",
        "/* x */ y",
        "/*x*/.foo",
        "obj./*c*/prop",
        "foo // line comment\n",
        "/* multi\nline */",
        // Regex literals — must NOT be normalized internally.
        "/foo/g",
        "/ a /.test(x)",
        "/[abc]/i",
        "/\\//",
        "x = /a\\/b/g",
        // Strings — must NOT be normalized internally.
        "'a'",
        "\"hello\"",
        "'a + b'",
        "\"x { y }\"",
        "'\\\\'",
        "\"\\\\\"",
        // Template literals.
        "`hi`",
        "`a${b}c`",
        "`$ {x}`", // not an interpolation
        "`a\\nb`",
        // JSX-ish fragments (not the full JSX path, but tag-shaped text).
        "<div />",
        "</div>",
        "<a:b />",
        // Parser-recovery shapes.
        "(, x)",
        "(x, )",
        "( , )",
        "[, x, ,]",
        // Tabs & whitespace (the chain shouldn't touch tabs — caller does
        // tab expansion separately, but must not break on tab-bearing input).
        "\tlet x = 1;",
        "let x =\t1;",
        "  let y = 2;",
        // Async arrow + yield (rules that only fire near these keywords).
        "async x => x",
        "function* g() { yield(x); }",
        "yield * 1",
        // Mixed: real-ish source line.
        "if (x === 0 && y !== null) { return foo(a, b, c); }",
        "const obj = { key: value, other: [1, 2, 3] };",
        "for (const [k, v] of Object.entries(map)) acc[k] = v;",
        // Multiline (chain runs on the whole text, not per-line).
        "{\n  a: 1,\n  b: 2,\n}",
        "/*\n * jsdoc\n */",
        "function f(x) {\n  return x + 1;\n}",
    ];

    /// Drives `normalize_emit_chain_v2` against the canonical multi-pass
    /// `normalize_emit_chain` for the entire corpus. Until v2 has its own
    /// implementation, both call the same code so this trivially passes —
    /// but it's the safety net that catches every divergence as soon as v2
    /// starts replacing stages.
    #[test]
    fn chain_v2_matches_chain_v1() {
        let mut failures: Vec<(usize, &str, String, String)> = Vec::new();
        for (idx, &input) in CHAIN_CORPUS.iter().enumerate() {
            let v1 = normalize_emit_chain(input).into_owned();
            let v2 = normalize_emit_chain_v2(input).into_owned();
            if v1 != v2 {
                failures.push((idx, input, v1, v2));
            }
        }
        if !failures.is_empty() {
            let mut msg = format!("{} corpus mismatch(es):\n", failures.len());
            for (idx, input, v1, v2) in failures {
                msg.push_str(&format!(
                    "  [{idx}] input={input:?}\n    v1={v1:?}\n    v2={v2:?}\n"
                ));
            }
            panic!("{msg}");
        }
    }

    /// Walk `input` through `LineState` byte-by-byte and return one label
    /// per *input* byte. Labels describe the lexical context active *while
    /// consuming* that byte (e.g. the opening `'` of a string is labelled
    /// `code` because the state transition fires *after* that byte advances
    /// the cursor). For multi-byte transitions, the label of each byte
    /// reflects the state when that specific byte is consumed.
    fn trace_state(input: &str) -> Vec<&'static str> {
        let bytes = input.as_bytes();
        let mut state = LineState::new();
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            let label = if state.in_line_comment {
                "line_comment"
            } else if state.in_block_comment {
                "block_comment"
            } else if state.in_regex {
                "regex"
            } else if state.in_string != 0 {
                "string"
            } else {
                "code"
            };
            let advance = state.advance(bytes, i);
            for _ in 0..advance {
                out.push(label);
            }
            i += advance;
        }
        out
    }

    #[test]
    fn line_state_tracks_strings() {
        // Code, then opening quote (still labelled code while consumed),
        // then string interior, then closing quote (still string), then code.
        // Note: `closes_string` runs before the label is recorded for the
        // closing quote, so the closing quote shows as "code". That's the
        // contract: labels describe the state ON ENTRY to consuming each
        // byte. Switch out of string mode happens at the closing quote.
        let labels = trace_state("a'bc'd");
        assert_eq!(
            labels,
            vec!["code", "code", "string", "string", "string", "code"]
        );
    }

    #[test]
    fn line_state_tracks_line_comments() {
        let labels = trace_state("a // x\nb");
        // `a`, ` `, `/`, `/` (still code on the second `/` — transition fires
        // after consuming both), then ` x` are line_comment, `\n` exits.
        assert_eq!(
            labels,
            vec![
                "code",
                "code",
                "code",
                "code",
                "line_comment",
                "line_comment",
                "line_comment",
                "code"
            ]
        );
    }

    #[test]
    fn line_state_tracks_block_comments() {
        let labels = trace_state("/*ab*/x");
        assert_eq!(
            labels,
            vec![
                "code",
                "code",
                "block_comment",
                "block_comment",
                "block_comment",
                "block_comment",
                "code"
            ]
        );
    }

    #[test]
    fn line_state_tracks_template_interpolation() {
        // `\`a${1}b\`` — backtick opens template, `${` exits string for
        // expression, `1` is code, `}` re-enters string, `b` is string,
        // closing backtick exits.
        let labels = trace_state("`a${1}b`");
        assert_eq!(
            labels,
            vec!["code", "string", "string", "string", "code", "code", "string", "string"]
        );
    }

    #[test]
    fn line_state_paren_depth() {
        let mut s = LineState::new();
        let bytes = b"(a(b))";
        let mut i = 0;
        while i < bytes.len() {
            i += s.advance(bytes, i);
        }
        assert_eq!(s.paren_depth, 0);
    }

    /// Sanity-check that the corpus exercises every normalizer in the chain
    /// (i.e., for each stage there is at least one input where its output
    /// differs from its input). Catches accidental corpus thinning when
    /// edits land later in the project.
    #[test]
    fn corpus_exercises_every_chain_stage() {
        fn check(name: &str, hits: bool) {
            assert!(hits, "no corpus input changes via {name}; add at least one",);
        }
        let mut hits_brace = false;
        let mut hits_keyword = false;
        let mut hits_import = false;
        let mut hits_unary = false;
        let mut hits_close = false;
        let mut hits_comment = false;
        for &input in CHAIN_CORPUS {
            if !matches!(normalize_brace_spacing(input), Cow::Borrowed(_)) {
                hits_brace = true;
            }
            if !matches!(normalize_keyword_paren(input), Cow::Borrowed(_)) {
                hits_keyword = true;
            }
            if !matches!(normalize_import_call_spacing(input), Cow::Borrowed(_)) {
                hits_import = true;
            }
            if !matches!(normalize_unary_spacing(input), Cow::Borrowed(_)) {
                hits_unary = true;
            }
            if !matches!(normalize_close_paren(input), Cow::Borrowed(_)) {
                hits_close = true;
            }
            if !matches!(normalize_comment_word_space(input), Cow::Borrowed(_)) {
                hits_comment = true;
            }
        }
        check("normalize_brace_spacing", hits_brace);
        check("normalize_keyword_paren", hits_keyword);
        check("normalize_import_call_spacing", hits_import);
        check("normalize_unary_spacing", hits_unary);
        check("normalize_close_paren", hits_close);
        check("normalize_comment_word_space", hits_comment);
    }
}
