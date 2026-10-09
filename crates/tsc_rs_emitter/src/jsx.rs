//! JSX emission - transforms JSX AST nodes to React.createElement calls
//! or _jsx/_jsxs calls (react-jsx/react-jsxdev modes).

use super::Emitter;
use crate::source_transform::expr_is_error_placeholder;
use tsc_rs_ast::*;
use tsc_rs_scanner::{Scanner, TokenKind};

pub(crate) fn jsx_dev_filename_ident(source: &str, jsx_text_spans: &[Span]) -> String {
    let tokens = Scanner::new(source).scan_all();
    let is_reserved = |candidate: &str| {
        tokens.iter().any(|token| {
            token.kind == TokenKind::Identifier
                && !jsx_text_spans
                    .iter()
                    .any(|span| token.span.start >= span.start && token.span.end <= span.end)
                && source
                    .get(token.span.start as usize..token.span.end as usize)
                    .is_some_and(|text| crate::normalize_unicode_escapes(text) == candidate)
        })
    };
    if !is_reserved("_jsxFileName") {
        return "_jsxFileName".to_string();
    }
    let mut suffix = 1usize;
    loop {
        let candidate = format!("_jsxFileName_{suffix}");
        if !is_reserved(&candidate) {
            return candidate;
        }
        suffix += 1;
    }
}

/// Trim JSX text content following React's whitespace rules:
/// - Single-line text (no newlines): preserved as-is, even if only whitespace
/// - Multi-line: each line is trimmed (first: trailing, last: leading, middle: both)
/// - Whitespace-only lines in multi-line text are removed
/// - Remaining lines are joined with a single space
pub fn jsx_trim_text(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let total = lines.len();

    // Single-line text: preserve as-is (including whitespace-only)
    if total <= 1 {
        return lines.first().copied().unwrap_or("").to_string();
    }

    // Multi-line text: trim per-line and join
    let mut result = String::new();
    for (i, line) in lines.iter().enumerate() {
        let is_first = i == 0;
        let is_last = i == total - 1;
        let processed = if is_first {
            line.trim_end()
        } else if is_last {
            line.trim_start()
        } else {
            line.trim()
        };
        if processed.is_empty() {
            continue;
        }
        if !result.is_empty() {
            result.push(' ');
        }
        result.push_str(processed);
    }
    result
}

/// Decode HTML/XML entities in JSX text and string attribute values.
/// Handles named entities (`&amp;`), decimal (`&#123;`), and hex (`&#x7B;`).
/// Unknown named entities are preserved as-is (e.g. `&notAnEntity;`).
/// Characters outside the BMP are emitted as UTF-16 surrogate pairs using
/// `\uXXXX\uXXXX` escape sequences, matching TypeScript's behavior.
///
/// Iterates over *characters* (via `char_indices`), not bytes. An earlier
/// implementation did `out.push(bytes[i] as char)` which split multi-byte
/// UTF-8 sequences into per-byte Latin-1 code points — `é` (UTF-8
/// `0xC3 0xA9`) became `Ã©` in the return value, and the downstream
/// [`jsx_escape_string_decoded`] then emitted `\u00C3\u00A9` (two escaped
/// code points) instead of the correct `\u00E9`. Any JSX text containing
/// non-ASCII characters was silently double-encoded in the compiled
/// output.
pub fn jsx_decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        let b = bytes[i];
        if b == b'&' {
            // Look for the closing semicolon
            if let Some(semi_offset) = text[i + 1..].find(';') {
                let entity = &text[i + 1..i + 1 + semi_offset];
                if let Some(decoded) = decode_entity(entity) {
                    out.push(decoded);
                    i += 2 + semi_offset; // skip & + entity + ;
                    continue;
                }
            }
        }
        // Decode the current char (possibly multi-byte UTF-8) and push it
        // as a *character*, not as a raw byte. The earlier implementation
        // did `out.push(bytes[i] as char)` which split multi-byte sequences
        // into per-byte Latin-1 code points — `é` (UTF-8 `0xC3 0xA9`)
        // became `Ã©`, and `jsx_escape_string_decoded` then emitted
        // `\u00C3\u00A9` instead of `\u00E9`.
        let ch_len = utf8_char_len(b);
        if ch_len > 1 && i + ch_len <= len {
            if let Ok(s) = std::str::from_utf8(&bytes[i..i + ch_len]) {
                if let Some(ch) = s.chars().next() {
                    out.push(ch);
                    i += ch_len;
                    continue;
                }
            }
        }
        // ASCII (or malformed continuation — fall through as Latin-1 to
        // preserve the legacy behavior for garbage input).
        out.push(b as char);
        i += 1;
    }
    out
}

/// Decode the UTF-8 length of the char starting at this byte. Returns 1
/// for ASCII and 2/3/4 for multi-byte leads. Continuation bytes (10xxxxxx)
/// return 1 so the caller falls back to byte-at-a-time handling on
/// malformed input rather than panicking.
fn utf8_char_len(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b < 0xC0 {
        1 // continuation byte without a lead — malformed
    } else if b < 0xE0 {
        2
    } else if b < 0xF0 {
        3
    } else {
        4
    }
}

#[cfg(test)]
mod jsx_unicode_tests {
    use super::{jsx_decode_entities, jsx_escape_string_decoded};

    /// Regression: JSX text "Négoce — é" was transpiled to
    /// `"N\u00C3\u00A9goce \u00E2\u0080\u0094 \u00C3\u00A9"` — the UTF-8
    /// bytes of each multi-byte char escaped as individual code points.
    /// After `jsx_decode_entities` + `jsx_escape_string_decoded` the
    /// output must carry the correct Unicode escapes (`\u00E9` for `é`,
    /// `\u2014` for `—`).
    #[test]
    fn decodes_multibyte_utf8_chars_as_single_code_points() {
        let decoded = jsx_decode_entities("Négoce — é");
        assert_eq!(
            decoded, "Négoce — é",
            "decode_entities must preserve multi-byte chars"
        );
        let escaped = jsx_escape_string_decoded(&decoded);
        assert!(
            escaped.contains("\\u00E9"),
            "expected \\u00E9 for é, got: {escaped}"
        );
        assert!(
            escaped.contains("\\u2014"),
            "expected \\u2014 for em-dash, got: {escaped}"
        );
        assert!(
            !escaped.contains("\\u00C3"),
            "\\u00C3 is the UTF-8 lead byte of é and shouldn't leak into output: {escaped}"
        );
        assert!(
            !escaped.contains("\\u00E2"),
            "\\u00E2 is the UTF-8 lead byte of em-dash: {escaped}"
        );
    }

    #[test]
    fn decode_entities_still_resolves_named_entities() {
        assert_eq!(jsx_decode_entities("A&amp;B"), "A&B");
        assert_eq!(jsx_decode_entities("&lt;tag&gt;"), "<tag>");
    }

    #[test]
    fn decode_entities_preserves_unknown_entity() {
        assert_eq!(jsx_decode_entities("&notAnEntity;"), "&notAnEntity;");
    }

    #[test]
    fn decode_entities_numeric_escape_roundtrips() {
        assert_eq!(jsx_decode_entities("&#233;"), "é");
        assert_eq!(jsx_decode_entities("&#x00E9;"), "é");
    }
}

/// Escape a decoded JSX string for JS output. Non-ASCII characters decoded from
/// HTML entities are emitted as `\uXXXX` escapes. Non-BMP characters use
/// `\uXXXX\uXXXX` UTF-16 surrogate pairs (matching TypeScript's behavior).
pub fn jsx_escape_string_decoded(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        let cp = ch as u32;
        if cp > 0xFFFF {
            // Emit UTF-16 surrogate pair
            let hi = ((cp - 0x10000) >> 10) + 0xD800;
            let lo = ((cp - 0x10000) & 0x3FF) + 0xDC00;
            out.push_str(&format!("\\u{:04X}\\u{:04X}", hi, lo));
        } else if cp > 0x7F {
            // Non-ASCII BMP: emit as \uXXXX
            out.push_str(&format!("\\u{:04X}", cp));
        } else {
            match ch {
                '\\' => out.push_str("\\\\"),
                '"' => out.push_str("\\\""),
                '\0' if chars.peek().is_some_and(|next| next.is_ascii_digit()) => {
                    out.push_str("\\x00")
                }
                '\0' => out.push_str("\\0"),
                '\u{0008}' => out.push_str("\\b"),
                '\u{000C}' => out.push_str("\\f"),
                '\u{000B}' => out.push_str("\\v"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                ch if ch < ' ' => out.push_str(&format!("\\u{:04X}", ch as u32)),
                _ => out.push(ch),
            }
        }
    }
    out
}

/// Escape a decoded JSX string for single-quoted JS output.
pub fn jsx_escape_string_single_quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        let cp = ch as u32;
        if cp > 0xFFFF {
            let hi = ((cp - 0x10000) >> 10) + 0xD800;
            let lo = ((cp - 0x10000) & 0x3FF) + 0xDC00;
            out.push_str(&format!("\\u{:04X}\\u{:04X}", hi, lo));
        } else if cp > 0x7F {
            out.push_str(&format!("\\u{:04X}", cp));
        } else {
            match ch {
                '\\' => out.push_str("\\\\"),
                '\'' => out.push_str("\\'"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                _ => out.push(ch),
            }
        }
    }
    out
}

fn decode_entity(entity: &str) -> Option<char> {
    // Numeric entities
    if let Some(rest) = entity.strip_prefix('#') {
        let code = if let Some(hex) = rest.strip_prefix('x').or_else(|| rest.strip_prefix('X')) {
            u32::from_str_radix(hex, 16).ok()?
        } else {
            rest.parse::<u32>().ok()?
        };
        return char::from_u32(code);
    }
    // Named entities — TypeScript uses the full HTML entity set
    Some(match entity {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{00A0}',
        "iexcl" => '\u{00A1}',
        "cent" => '\u{00A2}',
        "pound" => '\u{00A3}',
        "curren" => '\u{00A4}',
        "yen" => '\u{00A5}',
        "brvbar" => '\u{00A6}',
        "sect" => '\u{00A7}',
        "uml" => '\u{00A8}',
        "copy" => '\u{00A9}',
        "ordf" => '\u{00AA}',
        "laquo" => '\u{00AB}',
        "not" => '\u{00AC}',
        "shy" => '\u{00AD}',
        "reg" => '\u{00AE}',
        "macr" => '\u{00AF}',
        "deg" => '\u{00B0}',
        "plusmn" => '\u{00B1}',
        "sup2" => '\u{00B2}',
        "sup3" => '\u{00B3}',
        "acute" => '\u{00B4}',
        "micro" => '\u{00B5}',
        "para" => '\u{00B6}',
        "middot" => '\u{00B7}',
        "cedil" => '\u{00B8}',
        "sup1" => '\u{00B9}',
        "ordm" => '\u{00BA}',
        "raquo" => '\u{00BB}',
        "frac14" => '\u{00BC}',
        "frac12" => '\u{00BD}',
        "frac34" => '\u{00BE}',
        "iquest" => '\u{00BF}',
        "Agrave" => '\u{00C0}',
        "Aacute" => '\u{00C1}',
        "Acirc" => '\u{00C2}',
        "Atilde" => '\u{00C3}',
        "Auml" => '\u{00C4}',
        "Aring" => '\u{00C5}',
        "AElig" => '\u{00C6}',
        "Ccedil" => '\u{00C7}',
        "Egrave" => '\u{00C8}',
        "Eacute" => '\u{00C9}',
        "Ecirc" => '\u{00CA}',
        "Euml" => '\u{00CB}',
        "Igrave" => '\u{00CC}',
        "Iacute" => '\u{00CD}',
        "Icirc" => '\u{00CE}',
        "Iuml" => '\u{00CF}',
        "ETH" => '\u{00D0}',
        "Ntilde" => '\u{00D1}',
        "Ograve" => '\u{00D2}',
        "Oacute" => '\u{00D3}',
        "Ocirc" => '\u{00D4}',
        "Otilde" => '\u{00D5}',
        "Ouml" => '\u{00D6}',
        "times" => '\u{00D7}',
        "Oslash" => '\u{00D8}',
        "Ugrave" => '\u{00D9}',
        "Uacute" => '\u{00DA}',
        "Ucirc" => '\u{00DB}',
        "Uuml" => '\u{00DC}',
        "Yacute" => '\u{00DD}',
        "THORN" => '\u{00DE}',
        "szlig" => '\u{00DF}',
        "agrave" => '\u{00E0}',
        "aacute" => '\u{00E1}',
        "acirc" => '\u{00E2}',
        "atilde" => '\u{00E3}',
        "auml" => '\u{00E4}',
        "aring" => '\u{00E5}',
        "aelig" => '\u{00E6}',
        "ccedil" => '\u{00E7}',
        "egrave" => '\u{00E8}',
        "eacute" => '\u{00E9}',
        "ecirc" => '\u{00EA}',
        "euml" => '\u{00EB}',
        "igrave" => '\u{00EC}',
        "iacute" => '\u{00ED}',
        "icirc" => '\u{00EE}',
        "iuml" => '\u{00EF}',
        "eth" => '\u{00F0}',
        "ntilde" => '\u{00F1}',
        "ograve" => '\u{00F2}',
        "oacute" => '\u{00F3}',
        "ocirc" => '\u{00F4}',
        "otilde" => '\u{00F5}',
        "ouml" => '\u{00F6}',
        "divide" => '\u{00F7}',
        "oslash" => '\u{00F8}',
        "ugrave" => '\u{00F9}',
        "uacute" => '\u{00FA}',
        "ucirc" => '\u{00FB}',
        "uuml" => '\u{00FC}',
        "yacute" => '\u{00FD}',
        "thorn" => '\u{00FE}',
        "yuml" => '\u{00FF}',
        // Greek letters
        "Alpha" => '\u{0391}',
        "Beta" => '\u{0392}',
        "Gamma" => '\u{0393}',
        "Delta" => '\u{0394}',
        "Epsilon" => '\u{0395}',
        "Zeta" => '\u{0396}',
        "Eta" => '\u{0397}',
        "Theta" => '\u{0398}',
        "Iota" => '\u{0399}',
        "Kappa" => '\u{039A}',
        "Lambda" => '\u{039B}',
        "Mu" => '\u{039C}',
        "Nu" => '\u{039D}',
        "Xi" => '\u{039E}',
        "Omicron" => '\u{039F}',
        "Pi" => '\u{03A0}',
        "Rho" => '\u{03A1}',
        "Sigma" => '\u{03A3}',
        "Tau" => '\u{03A4}',
        "Upsilon" => '\u{03A5}',
        "Phi" => '\u{03A6}',
        "Chi" => '\u{03A7}',
        "Psi" => '\u{03A8}',
        "Omega" => '\u{03A9}',
        "alpha" => '\u{03B1}',
        "beta" => '\u{03B2}',
        "gamma" => '\u{03B3}',
        "delta" => '\u{03B4}',
        "epsilon" => '\u{03B5}',
        "zeta" => '\u{03B6}',
        "eta" => '\u{03B7}',
        "theta" => '\u{03B8}',
        "iota" => '\u{03B9}',
        "kappa" => '\u{03BA}',
        "lambda" => '\u{03BB}',
        "mu" => '\u{03BC}',
        "nu" => '\u{03BD}',
        "xi" => '\u{03BE}',
        "omicron" => '\u{03BF}',
        "pi" => '\u{03C0}',
        "rho" => '\u{03C1}',
        "sigmaf" => '\u{03C2}',
        "sigma" => '\u{03C3}',
        "tau" => '\u{03C4}',
        "upsilon" => '\u{03C5}',
        "phi" => '\u{03C6}',
        "chi" => '\u{03C7}',
        "psi" => '\u{03C8}',
        "omega" => '\u{03C9}',
        "thetasym" => '\u{03D1}',
        "upsih" => '\u{03D2}',
        "piv" => '\u{03D6}',
        // General punctuation
        "ensp" => '\u{2002}',
        "emsp" => '\u{2003}',
        "thinsp" => '\u{2009}',
        "zwnj" => '\u{200C}',
        "zwj" => '\u{200D}',
        "lrm" => '\u{200E}',
        "rlm" => '\u{200F}',
        "ndash" => '\u{2013}',
        "mdash" => '\u{2014}',
        "lsquo" => '\u{2018}',
        "rsquo" => '\u{2019}',
        "sbquo" => '\u{201A}',
        "ldquo" => '\u{201C}',
        "rdquo" => '\u{201D}',
        "bdquo" => '\u{201E}',
        "dagger" => '\u{2020}',
        "Dagger" => '\u{2021}',
        "bull" => '\u{2022}',
        "hellip" => '\u{2026}',
        "permil" => '\u{2030}',
        "prime" => '\u{2032}',
        "Prime" => '\u{2033}',
        "lsaquo" => '\u{2039}',
        "rsaquo" => '\u{203A}',
        "oline" => '\u{203E}',
        "frasl" => '\u{2044}',
        "euro" => '\u{20AC}',
        "image" => '\u{2111}',
        "weierp" => '\u{2118}',
        "real" => '\u{211C}',
        "trade" => '\u{2122}',
        "alefsym" => '\u{2135}',
        // Arrows
        "larr" => '\u{2190}',
        "uarr" => '\u{2191}',
        "rarr" => '\u{2192}',
        "darr" => '\u{2193}',
        "harr" => '\u{2194}',
        "crarr" => '\u{21B5}',
        "lArr" => '\u{21D0}',
        "uArr" => '\u{21D1}',
        "rArr" => '\u{21D2}',
        "dArr" => '\u{21D3}',
        "hArr" => '\u{21D4}',
        // Math
        "forall" => '\u{2200}',
        "part" => '\u{2202}',
        "exist" => '\u{2203}',
        "empty" => '\u{2205}',
        "nabla" => '\u{2207}',
        "isin" => '\u{2208}',
        "notin" => '\u{2209}',
        "ni" => '\u{220B}',
        "prod" => '\u{220F}',
        "sum" => '\u{2211}',
        "minus" => '\u{2212}',
        "lowast" => '\u{2217}',
        "radic" => '\u{221A}',
        "prop" => '\u{221D}',
        "infin" => '\u{221E}',
        "ang" => '\u{2220}',
        "and" => '\u{2227}',
        "or" => '\u{2228}',
        "cap" => '\u{2229}',
        "cup" => '\u{222A}',
        "int" => '\u{222B}',
        "there4" => '\u{2234}',
        "sim" => '\u{223C}',
        "cong" => '\u{2245}',
        "asymp" => '\u{2248}',
        "ne" => '\u{2260}',
        "equiv" => '\u{2261}',
        "le" => '\u{2264}',
        "ge" => '\u{2265}',
        "sub" => '\u{2282}',
        "sup" => '\u{2283}',
        "nsub" => '\u{2284}',
        "sube" => '\u{2286}',
        "supe" => '\u{2287}',
        "oplus" => '\u{2295}',
        "otimes" => '\u{2297}',
        "perp" => '\u{22A5}',
        "sdot" => '\u{22C5}',
        // Misc symbols
        "lceil" => '\u{2308}',
        "rceil" => '\u{2309}',
        "lfloor" => '\u{230A}',
        "rfloor" => '\u{230B}',
        "lang" => '\u{2329}',
        "rang" => '\u{232A}',
        "loz" => '\u{25CA}',
        "spades" => '\u{2660}',
        "clubs" => '\u{2663}',
        "hearts" => '\u{2665}',
        "diams" => '\u{2666}',
        // Special characters
        "OElig" => '\u{0152}',
        "oelig" => '\u{0153}',
        "Scaron" => '\u{0160}',
        "scaron" => '\u{0161}',
        "Yuml" => '\u{0178}',
        "fnof" => '\u{0192}',
        "circ" => '\u{02C6}',
        "tilde" => '\u{02DC}',
        _ => return None,
    })
}

/// Extract a trailing `/* ... */` comment from a JSX expression container
/// (`{expr /*comment*/}`), if one exists between `expr` end and container end.
fn jsx_expr_trailing_block_comment<'a>(
    source: &'a str,
    expr_span: Span,
    container_span: Span,
) -> Option<&'a str> {
    let from = expr_span.end as usize;
    let to = container_span.end as usize;
    if from >= to || to > source.len() {
        return None;
    }
    let between = &source[from..to];
    let start = between.find("/*")?;
    let tail = &between[start..];
    let end = tail.find("*/")?;
    Some(tail[..end + 2].trim())
}

/// Check if a string is a valid JSX factory/fragment factory name.
/// Must be an identifier or dotted identifier (e.g. `React.createElement`, `React.Fragment`).
/// Rejects numeric literals, operators, etc.
#[allow(dead_code)]
fn is_valid_jsx_factory_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    for part in name.split('.') {
        if part.is_empty() {
            return false;
        }
        let mut chars = part.chars();
        // First char must be letter, underscore, or $
        match chars.next() {
            Some(c) if c.is_ascii_alphabetic() || c == '_' || c == '$' => {}
            Some(c) if !c.is_ascii() && c.is_alphabetic() => {} // Unicode identifiers
            _ => return false,
        }
        // Remaining chars can also include digits
        for c in chars {
            if !(c.is_ascii_alphanumeric()
                || c == '_'
                || c == '$'
                || (!c.is_ascii() && c.is_alphanumeric()))
            {
                return false;
            }
        }
    }
    true
}

// ---------------------------------------------------------------------------
// Helper: count effective (non-whitespace-only) children of a JSX element
// ---------------------------------------------------------------------------

/// Count the effective children of a JSX element (non-whitespace-only text,
/// non-empty expression containers).
/// Returns true if an object literal in a JSX spread can be fully inlined
/// into the parent object without needing Object.assign. This is false when
/// the object contains non-object-literal spread elements or `__proto__` keys
/// (non-computed, non-shorthand `__proto__` or `"__proto__"` property assignments).
fn jsx_obj_lit_is_fully_inlinable(props: &[ObjLitProp]) -> bool {
    for prop in props {
        match prop {
            ObjLitProp::Spread(expr, _) => match &expr.kind {
                ExprKind::ObjectLit(inner) => {
                    if !jsx_obj_lit_is_fully_inlinable(inner) {
                        return false;
                    }
                }
                _ => return false,
            },
            ObjLitProp::Property(p) => {
                // Only non-computed __proto__ properties need Object.assign to avoid
                // prototype mutation. Computed [__proto__], ["__proto__"], and shorthand
                // __proto__ don't set the prototype.
                if !p.computed {
                    let is_proto = match &p.key {
                        PropName::Ident(n, _) => n == "__proto__",
                        PropName::String(n, _) => n == "__proto__",
                        _ => false,
                    };
                    if is_proto {
                        return false;
                    }
                }
            }
            _ => {}
        }
    }
    true
}

/// Returns true when flattening this object literal into another object literal
/// would change a `__proto__` setter into a normal data property. Computed and
/// shorthand `__proto__` properties are safe to flatten.
fn jsx_obj_lit_has_proto_setter(props: &[ObjLitProp]) -> bool {
    props.iter().any(|prop| {
        let ObjLitProp::Property(prop) = prop else {
            return false;
        };
        !prop.computed
            && matches!(
                &prop.key,
                PropName::Ident(name, _) | PropName::String(name, _) if name == "__proto__"
            )
    })
}

/// Flattening is only CopyDataProperties-equivalent for data properties and
/// shorthand entries. Accessors must be read eagerly and copied as data
/// properties; methods also carry an object-literal [[HomeObject]]. Keep those
/// object literals behind a real spread boundary.
fn jsx_obj_lit_is_native_flatten_safe(props: &[ObjLitProp]) -> bool {
    !jsx_obj_lit_has_proto_setter(props)
        && props.iter().all(|prop| {
            matches!(
                prop,
                ObjLitProp::Property(_)
                    | ObjLitProp::Shorthand(_, _)
                    | ObjLitProp::ShorthandDefault(_, _, _)
                    | ObjLitProp::Spread(_, _)
            )
        })
}

/// The automatic runtime's merged-object fast path recursively erases every
/// object-spread boundary, so every nested object literal must also be safe.
fn jsx_obj_lit_is_automatic_merge_safe(props: &[ObjLitProp]) -> bool {
    !jsx_obj_lit_has_proto_setter(props)
        && props.iter().all(|prop| match prop {
            ObjLitProp::Property(_)
            | ObjLitProp::Shorthand(_, _)
            | ObjLitProp::ShorthandDefault(_, _, _) => true,
            ObjLitProp::Spread(expr, _) => {
                matches!(&expr.kind, ExprKind::ObjectLit(inner) if jsx_obj_lit_is_automatic_merge_safe(inner))
            }
            ObjLitProp::Method(_) | ObjLitProp::Get(_) | ObjLitProp::Set(_) => false,
        })
}

/// Check if all spread attributes in a JSX element are object literals that
/// can be fully inlined into a single props object (no Object.assign needed).
fn jsx_all_spreads_inlinable(attrs: &[JsxAttribute]) -> bool {
    for attr in attrs {
        if let JsxAttribute::Spread(expr, _) = attr {
            match &expr.kind {
                ExprKind::ObjectLit(props) => {
                    if !jsx_obj_lit_is_fully_inlinable(props)
                        || !jsx_obj_lit_is_automatic_merge_safe(props)
                    {
                        return false;
                    }
                }
                _ => return false,
            }
        }
    }
    true
}

fn jsx_expr_is_recovery_error_element(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Ident(n) => n == "<error>",
        ExprKind::JsxElement(el) => matches!(&el.name.kind, ExprKind::Ident(n) if n == "<error>"),
        ExprKind::JsxSelfClosing(el) => {
            matches!(&el.name.kind, ExprKind::Ident(n) if n == "<error>")
        }
        _ => false,
    }
}

fn jsx_attr_span(attr: &JsxAttribute) -> Span {
    match attr {
        JsxAttribute::Normal { span, .. } => *span,
        JsxAttribute::Spread(_, span) => *span,
    }
}

fn count_effective_children(children: &[JsxChild]) -> usize {
    children
        .iter()
        .filter(|c| match c {
            JsxChild::Text(text, _) => !jsx_trim_text(text).is_empty(),
            JsxChild::Expression(None, _) => false,
            JsxChild::Expression(Some(expr), _) => !expr_is_error_placeholder(expr),
            JsxChild::Element(expr) => !jsx_expr_is_recovery_error_element(expr),
            _ => true,
        })
        .count()
}

/// The automatic JSX runtime represents multiple children as an array. A
/// spread child also requires that array container even when it is the only
/// effective child: `children: [...value]`, not the invalid
/// `children: ...value`.
fn jsx_runtime_has_spread_child(children: &[JsxChild]) -> bool {
    children.iter().any(|child| {
        matches!(
            child,
            JsxChild::Expression(Some(expr), _)
                if matches!(expr.kind, ExprKind::Spread(_))
                    && !expr_is_error_placeholder(expr)
        )
    })
}

fn jsx_runtime_children_need_array(children: &[JsxChild]) -> bool {
    count_effective_children(children) > 1 || jsx_runtime_has_spread_child(children)
}

// ---------------------------------------------------------------------------
// Helper: determine if key comes after a spread in the attribute list
// ---------------------------------------------------------------------------

/// Returns true if there is a `key` attribute AND it appears after at least one
/// spread attribute. In this case, react-jsx falls back to createElement.
fn has_key_after_spread(attrs: &[JsxAttribute]) -> bool {
    let mut seen_spread = false;
    for attr in attrs {
        match attr {
            JsxAttribute::Spread(_, _) => {
                seen_spread = true;
            }
            JsxAttribute::Normal { name, .. } if name == "key" => {
                if seen_spread {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

/// Extract the first key attribute, including the boolean shorthand form.
fn find_key_attr(attrs: &[JsxAttribute]) -> Option<&JsxAttribute> {
    for attr in attrs {
        if let JsxAttribute::Normal { name, .. } = attr {
            if name == "key" {
                return Some(attr);
            }
        }
    }
    None
}

fn find_key_attr_index(attrs: &[JsxAttribute]) -> Option<usize> {
    attrs.iter().position(|attr| {
        matches!(
            attr,
            JsxAttribute::Normal {
                name,
                ..
            } if name == "key"
        )
    })
}

// ---------------------------------------------------------------------------
// Pre-scan: determine which JSX runtime functions are needed
// ---------------------------------------------------------------------------

/// Recursively scan statements to find JSX usage and set the runtime flags.
pub(crate) fn scan_jsx_runtime_needs(stmts: &[Stmt], emitter: &mut Emitter) {
    for stmt in stmts {
        scan_jsx_stmt(stmt, emitter);
    }
}

fn scan_jsx_stmt(stmt: &Stmt, emitter: &mut Emitter) {
    match &stmt.kind {
        StmtKind::Expr(e) => scan_jsx_expr(e, emitter),
        StmtKind::Var(var_stmt) => {
            for decl in &var_stmt.declarations {
                if let Some(ref init) = decl.init {
                    scan_jsx_expr(init, emitter);
                }
            }
        }
        StmtKind::FnDecl(f) => {
            if let Some(ref body) = f.body {
                for s in body {
                    scan_jsx_stmt(s, emitter);
                }
            }
        }
        StmtKind::Return(Some(e)) => scan_jsx_expr(e, emitter),
        StmtKind::If(if_stmt) => {
            scan_jsx_expr(&if_stmt.test, emitter);
            scan_jsx_stmt(&if_stmt.consequent, emitter);
            if let Some(ref alt) = if_stmt.alternate {
                scan_jsx_stmt(alt, emitter);
            }
        }
        StmtKind::Block(stmts) => {
            for s in stmts {
                scan_jsx_stmt(s, emitter);
            }
        }
        StmtKind::Export(e) => {
            if let ExportDeclKind::Decl(inner) = &e.kind {
                scan_jsx_stmt(inner, emitter);
            } else if let ExportDeclKind::Default(expr) = &e.kind {
                scan_jsx_expr(expr, emitter);
            } else if let ExportDeclKind::DefaultDecl(inner) = &e.kind {
                scan_jsx_stmt(inner, emitter);
            }
        }
        StmtKind::ClassDecl(c) => {
            for m in &c.members {
                scan_jsx_class_member(m, emitter);
            }
        }
        StmtKind::For(for_stmt) => scan_jsx_stmt(&for_stmt.body, emitter),
        StmtKind::ForIn(for_in) => scan_jsx_stmt(&for_in.body, emitter),
        StmtKind::ForOf(for_of) => scan_jsx_stmt(&for_of.body, emitter),
        StmtKind::While(w) => scan_jsx_stmt(&w.body, emitter),
        StmtKind::DoWhile(dw) => scan_jsx_stmt(&dw.body, emitter),
        StmtKind::Switch(sw) => {
            for case in &sw.cases {
                for s in &case.consequent {
                    scan_jsx_stmt(s, emitter);
                }
            }
        }
        StmtKind::Try(try_stmt) => {
            for s in &try_stmt.block {
                scan_jsx_stmt(s, emitter);
            }
            if let Some(ref cc) = try_stmt.handler {
                for s in &cc.body {
                    scan_jsx_stmt(s, emitter);
                }
            }
            if let Some(ref fb) = try_stmt.finalizer {
                for s in fb {
                    scan_jsx_stmt(s, emitter);
                }
            }
        }
        StmtKind::Labeled(labeled) => scan_jsx_stmt(&labeled.body, emitter),
        StmtKind::ModuleDecl(md) => {
            if let Some(ref body) = md.body {
                scan_jsx_module_body(body, emitter);
            }
        }
        StmtKind::Throw(e) => scan_jsx_expr(e, emitter),
        _ => {}
    }
}

fn scan_jsx_module_body(body: &ModuleBody, emitter: &mut Emitter) {
    match body {
        ModuleBody::Block(stmts) => {
            for s in stmts {
                scan_jsx_stmt(s, emitter);
            }
        }
        ModuleBody::Module(inner) => {
            if let Some(ref body) = inner.body {
                scan_jsx_module_body(body, emitter);
            }
        }
    }
}

fn scan_jsx_class_member(member: &ClassMember, emitter: &mut Emitter) {
    match &member.kind {
        ClassMemberKind::Method(m) => {
            if let Some(ref body) = m.body {
                for s in body {
                    scan_jsx_stmt(s, emitter);
                }
            }
        }
        ClassMemberKind::Property(p) => {
            if let Some(ref init) = p.initializer {
                scan_jsx_expr(init, emitter);
            }
        }
        ClassMemberKind::Constructor(c) => {
            if let Some(ref body) = c.body {
                for s in body {
                    scan_jsx_stmt(s, emitter);
                }
            }
        }
        ClassMemberKind::StaticBlock(stmts) => {
            for s in stmts {
                scan_jsx_stmt(s, emitter);
            }
        }
        _ => {}
    }
}

fn scan_jsx_expr(expr: &Expr, emitter: &mut Emitter) {
    match &expr.kind {
        ExprKind::JsxElement(el) => {
            if has_key_after_spread(&el.attributes) {
                emitter.jsx_runtime_needs_create_element = true;
            } else {
                if jsx_runtime_children_need_array(&el.children) {
                    emitter.jsx_runtime_needs_jsxs = true;
                } else {
                    emitter.jsx_runtime_needs_jsx = true;
                }
            }
            // Scan children recursively
            scan_jsx_children(&el.children, emitter);
            // Scan attribute expressions
            for attr in &el.attributes {
                match attr {
                    JsxAttribute::Normal { value: Some(v), .. } => scan_jsx_expr(v, emitter),
                    JsxAttribute::Spread(e, _) => scan_jsx_expr(e, emitter),
                    _ => {}
                }
            }
        }
        ExprKind::JsxSelfClosing(el) => {
            if has_key_after_spread(&el.attributes) {
                emitter.jsx_runtime_needs_create_element = true;
            } else {
                emitter.jsx_runtime_needs_jsx = true;
            }
            // Scan attribute expressions
            for attr in &el.attributes {
                match attr {
                    JsxAttribute::Normal { value: Some(v), .. } => scan_jsx_expr(v, emitter),
                    JsxAttribute::Spread(e, _) => scan_jsx_expr(e, emitter),
                    _ => {}
                }
            }
        }
        ExprKind::JsxFragment(frag) => {
            emitter.jsx_runtime_needs_fragment = true;
            if jsx_runtime_children_need_array(&frag.children) {
                emitter.jsx_runtime_needs_jsxs = true;
            } else {
                emitter.jsx_runtime_needs_jsx = true;
            }
            scan_jsx_children(&frag.children, emitter);
        }
        // Recurse into sub-expressions
        ExprKind::Paren(inner) => scan_jsx_expr(inner, emitter),
        ExprKind::Cond(cond) => {
            scan_jsx_expr(&cond.test, emitter);
            scan_jsx_expr(&cond.consequent, emitter);
            scan_jsx_expr(&cond.alternate, emitter);
        }
        ExprKind::Binary(bin) => {
            scan_jsx_expr(&bin.left, emitter);
            scan_jsx_expr(&bin.right, emitter);
        }
        ExprKind::Call(call) => {
            scan_jsx_expr(&call.callee, emitter);
            for arg in &call.args {
                scan_jsx_expr(arg, emitter);
            }
        }
        ExprKind::Arrow(a) => match &a.body {
            ArrowBody::Expr(e) => scan_jsx_expr(e, emitter),
            ArrowBody::Block(stmts) => {
                for s in stmts {
                    scan_jsx_stmt(s, emitter);
                }
            }
        },
        ExprKind::Assign(a) => {
            scan_jsx_expr(&a.left, emitter);
            scan_jsx_expr(&a.right, emitter);
        }
        ExprKind::ObjectLit(props) => {
            for prop in props {
                match prop {
                    ObjLitProp::Property(p) => {
                        scan_jsx_expr(&p.value, emitter);
                    }
                    ObjLitProp::Spread(e, _) => scan_jsx_expr(e, emitter),
                    ObjLitProp::Method(m) => {
                        for s in &m.body {
                            scan_jsx_stmt(s, emitter);
                        }
                    }
                    ObjLitProp::Get(acc) | ObjLitProp::Set(acc) => {
                        for s in &acc.body {
                            scan_jsx_stmt(s, emitter);
                        }
                    }
                    _ => {}
                }
            }
        }
        ExprKind::ArrayLit(elems) => {
            for e in elems {
                if let Some(e) = e {
                    scan_jsx_expr(e, emitter);
                }
            }
        }
        ExprKind::Spread(inner) => scan_jsx_expr(inner, emitter),
        ExprKind::Unary(u) => scan_jsx_expr(&u.argument, emitter),
        ExprKind::Update(u) => scan_jsx_expr(&u.argument, emitter),
        ExprKind::Comma(exprs) => {
            for e in exprs {
                scan_jsx_expr(e, emitter);
            }
        }
        ExprKind::TaggedTemplate(tt) => scan_jsx_expr(&tt.tag, emitter),
        ExprKind::TypeAssertion(inner) => scan_jsx_expr(&inner.expr, emitter),
        ExprKind::As(a) => {
            scan_jsx_expr(&a.expr, emitter);
        }
        ExprKind::Satisfies(s) => {
            scan_jsx_expr(&s.expr, emitter);
        }
        ExprKind::Member(m) => scan_jsx_expr(&m.object, emitter),
        ExprKind::ElemAccess(ea) => {
            scan_jsx_expr(&ea.object, emitter);
            scan_jsx_expr(&ea.index, emitter);
        }
        ExprKind::New(n) => {
            scan_jsx_expr(&n.callee, emitter);
            if let Some(ref args) = n.args {
                for arg in args {
                    scan_jsx_expr(arg, emitter);
                }
            }
        }
        ExprKind::NonNull(inner) => {
            scan_jsx_expr(inner, emitter);
        }
        ExprKind::Instantiation(inst) => {
            scan_jsx_expr(&inst.expr, emitter);
        }
        ExprKind::Await(inner)
        | ExprKind::Delete(inner)
        | ExprKind::Typeof(inner)
        | ExprKind::Void(inner) => {
            scan_jsx_expr(inner, emitter);
        }
        ExprKind::Yield(_, Some(inner)) => scan_jsx_expr(inner, emitter),
        ExprKind::FnExpr(f) => {
            if let Some(ref body) = f.body {
                for s in body {
                    scan_jsx_stmt(s, emitter);
                }
            }
        }
        ExprKind::ClassExpr(c) => {
            for m in &c.members {
                scan_jsx_class_member(m, emitter);
            }
        }
        _ => {}
    }
}

fn scan_jsx_children(children: &[JsxChild], emitter: &mut Emitter) {
    for child in children {
        match child {
            JsxChild::Text(_, span) => emitter.jsx_text_spans.push(*span),
            JsxChild::Element(e) => scan_jsx_expr(e, emitter),
            JsxChild::Expression(Some(e), _) => scan_jsx_expr(e, emitter),
            JsxChild::Fragment(frag) => {
                emitter.jsx_runtime_needs_fragment = true;
                if jsx_runtime_children_need_array(&frag.children) {
                    emitter.jsx_runtime_needs_jsxs = true;
                } else {
                    emitter.jsx_runtime_needs_jsx = true;
                }
                scan_jsx_children(&frag.children, emitter);
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Existing React.createElement emit (classic mode)
// ---------------------------------------------------------------------------

impl<'a> Emitter<'a> {
    fn jsx_has_emitted_comments_in_range(&self, start: u32, end: u32) -> bool {
        (self.options.remove_comments != Some(true) || self.preserve_comments)
            && self.has_comments_in_range(start, end)
    }

    pub(crate) fn jsx_factory(&self) -> &str {
        // Per-file @jsx pragma takes priority over compiler options.
        if let Some(ref pragma) = self.jsx_pragma_factory {
            return pragma.as_str();
        }
        self.options
            .jsx_factory
            .as_deref()
            .unwrap_or("React.createElement")
    }

    pub(crate) fn jsx_fragment_factory(&self) -> &str {
        // Per-file @jsxFrag pragma takes priority over compiler options.
        if let Some(ref pragma) = self.jsx_pragma_fragment {
            return pragma.as_str();
        }
        self.options
            .jsx_fragment_factory
            .as_deref()
            .unwrap_or("React.Fragment")
    }

    /// Emit a JSX attribute name, quoting it if it contains a hyphen or colon.
    /// Flatten a JSX spread expression into Object.assign arguments.
    /// Only the top-level object literal is flattened — nested object literals
    /// with spreads or __proto__ keys are emitted as regular expressions.
    fn jsx_flatten_spread_into_assign(
        &mut self,
        spread_expr: &Expr,
        spread_span: Span,
        in_obj: &mut bool,
        first_arg: &mut bool,
        prop_count: &mut usize,
        automatic_copy_semantics: bool,
    ) {
        if let ExprKind::ObjectLit(ref props) = spread_expr.kind {
            let has_comments =
                self.jsx_has_emitted_comments_in_range(spread_span.start, spread_span.end);
            let automatic_boundary = automatic_copy_semantics
                && (!jsx_obj_lit_is_native_flatten_safe(props) || has_comments);
            // Classic React retains its TypeScript-compatible flattening. The
            // automatic runtime requires a distinct source object for direct
            // accessors, methods, __proto__ setters, and comments.
            if jsx_obj_lit_has_proto_setter(props) || automatic_boundary {
                if *in_obj {
                    self.indent -= 1;
                    self.write(" }");
                    *in_obj = false;
                    *prop_count = 0;
                }
                if !*first_arg {
                    self.write(", ");
                }
                if automatic_copy_semantics {
                    self.emit_jsx_comments_after_spread_dots(spread_expr, spread_span);
                }
                if automatic_copy_semantics
                    && self.jsx_has_emitted_comments_in_range(
                        spread_expr.span.start,
                        spread_expr.span.end,
                    )
                {
                    self.emit_jsx_comment_aware_object_literal(spread_expr, props);
                } else {
                    self.emit_jsx_expression_value(spread_expr);
                }
                if automatic_copy_semantics {
                    self.emit_inline_comments_in_range(spread_expr.span.end, spread_span.end);
                }
                *first_arg = false;
            } else if !jsx_obj_lit_is_fully_inlinable(props)
                || (automatic_copy_semantics && !jsx_obj_lit_is_automatic_merge_safe(props))
            {
                // Flatten this level: each prop gets processed individually.
                for prop in props {
                    match prop {
                        ObjLitProp::Spread(inner_expr, inner_span) => {
                            if automatic_copy_semantics && jsx_obj_lit_is_fully_inlinable(props) {
                                self.jsx_flatten_spread_into_assign(
                                    inner_expr,
                                    *inner_span,
                                    in_obj,
                                    first_arg,
                                    prop_count,
                                    true,
                                );
                                continue;
                            }
                            // Inner spread becomes a separate Object.assign argument.
                            // Close current object group first.
                            if *in_obj {
                                self.indent -= 1;
                                self.write(" }");
                                *in_obj = false;
                                *prop_count = 0;
                            }
                            if !*first_arg {
                                self.write(", ");
                            }
                            self.emit_expr(inner_expr);
                            *first_arg = false;
                        }
                        _ => {
                            // Non-spread property — add to current object group.
                            if !*in_obj {
                                if !*first_arg {
                                    self.write(", ");
                                }
                                self.write("{ ");
                                self.indent += 1;
                                *in_obj = true;
                            } else if *prop_count > 0 {
                                self.write(", ");
                            }
                            *prop_count += 1;
                            self.emit_obj_lit_prop(prop);
                            *first_arg = false;
                        }
                    }
                }
            } else {
                // Fully inlinable — inline all properties into current group.
                if !*in_obj {
                    if !*first_arg {
                        self.write(", ");
                    }
                    self.write("{ ");
                    self.indent += 1;
                    *in_obj = true;
                }
                for prop in props {
                    if *prop_count > 0 {
                        self.write(", ");
                    }
                    *prop_count += 1;
                    self.emit_obj_lit_prop(prop);
                }
                *first_arg = false;
            }
        } else {
            // Non-object-literal spread — close current group, emit as argument.
            if *in_obj {
                self.indent -= 1;
                self.write(" }");
                *in_obj = false;
                *prop_count = 0;
            }
            if !*first_arg {
                self.write(", ");
            }
            self.emit_jsx_expression_value(spread_expr);
            *first_arg = false;
        }
    }

    fn emit_jsx_attr_name(&mut self, name: &str) {
        // Resolve unicode escapes to check for dashes/colons and to emit
        // the resolved form in quoted attribute names.
        let resolved = crate::normalize_unicode_escapes(name);
        if resolved.contains('-') || resolved.contains(':') {
            self.write("\"");
            self.write(&resolved);
            self.write("\"");
        } else {
            // Keep original (with unicode escapes) for simple attribute names
            self.write(name);
        }
    }

    fn jsx_attr_value_is_missing_recovery(&self, value: &Expr) -> bool {
        if expr_is_error_placeholder(value) || matches!(value.kind, ExprKind::Omitted) {
            return true;
        }
        let s = value.span.start as usize;
        let e = value.span.end as usize;
        if s >= e || e > self.source.len() {
            return true;
        }
        let text = self.source[s..e].trim();
        if text.is_empty() {
            return true;
        }
        if text.starts_with('}') || text.starts_with('>') || text.starts_with("/>") {
            return true;
        }
        if text.starts_with("</") {
            return true;
        }
        matches!(text, "}" | ">" | "/>" | "</")
    }

    fn jsx_attr_value_is_immediate_spread_recovery(&self, value: &Expr) -> bool {
        if !matches!(value.kind, ExprKind::Spread(_)) {
            return false;
        }
        let start = value.span.start as usize;
        if start == 0 || start > self.source.len() {
            return false;
        }
        self.source[..start].trim_end().ends_with("={")
    }

    /// Emit a JSX attribute value, decoding HTML entities for string literals
    /// that came from JSX string attributes (not expression containers).
    /// JSX string attrs like `attr="&amp;"` get entity decoding.
    /// Expression attrs like `attr={"&amp;"}` do NOT get entity decoding.
    /// Preserves the original quote character from the source.
    fn emit_jsx_attr_value(&mut self, val: &Expr) {
        if matches!(val.kind, ExprKind::Omitted) {
            self.write("true");
            return;
        }
        if let ExprKind::StrLit(s) = &val.kind {
            let start = val.span.start as usize;
            let source_byte = if start < self.source.len() {
                self.source.as_bytes()[start]
            } else {
                0
            };
            // Check that this is a true JSX string attribute (attr="..."), not
            // an expression container (attr={"..."}) where the parser strips the { }.
            let is_jsx_string = if matches!(source_byte, b'"' | b'\'') && start > 0 {
                // Scan backwards to find the first non-whitespace byte before the quote.
                // If it's '=', this is a JSX string attr. If it's '{', it's an expression.
                let before = &self.source.as_bytes()[..start];
                let prev_non_ws = before.iter().rposition(|&b| !b.is_ascii_whitespace());
                prev_non_ws.is_some_and(|pos| before[pos] != b'{')
            } else {
                false
            };
            if is_jsx_string && s.contains('&') {
                // JSX string attribute with potential HTML entities — decode them
                let decoded = jsx_decode_entities(s);
                let quote = source_byte as char;
                self.write(&String::from(quote));
                if quote == '\'' {
                    self.write(&jsx_escape_string_single_quoted(&decoded));
                } else {
                    self.write(&jsx_escape_string_decoded(&decoded));
                }
                self.write(&String::from(quote));
            } else {
                // No entities or expression attribute — use normal emit
                self.emit_expr(val);
            }
        } else {
            self.emit_jsx_expression_value(val);
        }
    }

    /// A JSX container contributes one value to an argument, property, array,
    /// or spread operand even when recovery retains a comma expression.
    fn emit_jsx_expression_value(&mut self, expr: &Expr) {
        let comma = matches!(expr.kind, ExprKind::Comma(_));
        if comma {
            self.write("(");
        }
        self.emit_expr(expr);
        if comma {
            self.write(")");
        }
    }

    fn emit_jsx_key_attr_value(&mut self, attr: &JsxAttribute) {
        match attr {
            JsxAttribute::Normal {
                value: Some(value), ..
            } => self.emit_jsx_attr_value(value),
            JsxAttribute::Normal { value: None, .. } => self.write("true"),
            JsxAttribute::Spread(_, _) => unreachable!("a key attribute cannot be a spread"),
        }
    }

    fn jsx_child_is_recovery_error(&self, child_expr: &Expr) -> bool {
        jsx_expr_is_recovery_error_element(child_expr) || expr_is_error_placeholder(child_expr)
    }

    fn split_jsx_recovery_trailing_attrs<'b>(
        &self,
        attrs: &'b [JsxAttribute],
    ) -> (&'b [JsxAttribute], &'b [JsxAttribute]) {
        if !self.file_has_recovery_errors {
            return (attrs, &[]);
        }
        for (index, attr) in attrs.iter().enumerate() {
            let span = jsx_attr_span(attr);
            // A nested JSX value or a quoted string can legitimately contain
            // `/>`. Recovery can only own a delimiter outside that value.
            let s = match attr {
                JsxAttribute::Normal {
                    value: Some(value), ..
                }
                | JsxAttribute::Spread(value, _) => value.span.end.max(span.start) as usize,
                _ => span.start as usize,
            };
            let e = span.end as usize;
            if s < e && e <= self.source.len() {
                let text = &self.source[s..e];
                if text.contains("/>") {
                    // Comments in the remaining trivia are not delimiters.
                    let mut scanner = tsc_rs_scanner::TsScanner::new(text);
                    loop {
                        match scanner.scan() {
                            tsc_rs_scanner::TokenKind::EndOfFile => break,
                            tsc_rs_scanner::TokenKind::Slash
                                if text[scanner.text_pos()..].starts_with('>') =>
                            {
                                return attrs.split_at(index + 1);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        (attrs, &[])
    }

    fn jsx_recovered_outer_attrs_from_value<'b>(&self, value: &'b Expr) -> &'b [JsxAttribute] {
        match &value.kind {
            ExprKind::JsxElement(el) => {
                let (_, trailing) = self.split_jsx_recovery_trailing_attrs(&el.attributes);
                trailing
            }
            ExprKind::JsxSelfClosing(el) => {
                let (_, trailing) = self.split_jsx_recovery_trailing_attrs(&el.attributes);
                trailing
            }
            _ => &[],
        }
    }

    /// Rewrite a factory name using the CJS import map.
    /// For dotted names (e.g. `React.createElement`): if the first segment is in
    /// `cjs_import_map`, replace it (e.g. `react_1.default.createElement`).
    /// For simple names (e.g. `element`): if the name is in `cjs_import_map`,
    /// emit as `(0, var.prop)` when `as_call` is true, or `var.prop` when false.
    fn emit_cjs_factory(&mut self, factory: &str, as_call: bool) {
        if let Some(dot) = factory.find('.') {
            let first = &factory[..dot];
            if let Some((var_name, imported)) = self.cjs_import_map.get(first).cloned() {
                if imported.is_empty() {
                    self.write(&var_name);
                } else {
                    self.write_cjs_import_access(&var_name, &imported, first);
                }
                self.write(&factory[dot..]);
                return;
            }
        } else if let Some((var_name, imported)) = self.cjs_import_map.get(factory).cloned() {
            // Simple name rewrite through CJS import map.
            if as_call {
                self.write("(0, ");
            }
            if imported.is_empty() {
                self.write(&var_name);
                self.write(".");
                self.write(factory);
            } else {
                self.write_cjs_import_access(&var_name, &imported, factory);
            }
            if as_call {
                self.write(")");
            }
            return;
        }
        // Inside a namespace, qualify the first segment if it's a namespace export.
        if let Some(dot) = factory.find('.') {
            let first = &factory[..dot];
            if self.namespace_exports.contains(first) {
                if let Some(target) = self.export_target.clone() {
                    if target != "exports" {
                        self.write(&target);
                        self.write(".");
                    }
                }
            }
        }
        self.write(factory);
    }

    pub(crate) fn emit_jsx_element(&mut self, el: &JsxElement) {
        let factory = self.jsx_factory().to_string();
        self.emit_cjs_factory(&factory, true);
        self.write("(");
        self.emit_jsx_tag_name(&el.name);
        self.write(", ");
        self.emit_jsx_attributes(&el.attributes);
        self.emit_jsx_children_args(&el.children);
        self.write(")");
    }

    pub(crate) fn emit_jsx_self_closing(&mut self, el: &JsxSelfClosingElement) {
        let factory = self.jsx_factory().to_string();
        self.emit_cjs_factory(&factory, true);
        self.write("(");
        self.emit_jsx_tag_name(&el.name);
        self.write(", ");
        self.emit_jsx_attributes(&el.attributes);
        self.write(")");
    }

    pub(crate) fn emit_jsx_fragment(&mut self, frag: &JsxFragment) {
        let factory = self.jsx_factory().to_string();
        let frag_factory = self.jsx_fragment_factory().to_string();
        self.emit_cjs_factory(&factory, true);
        self.write("(");
        self.emit_cjs_factory(&frag_factory, false);
        self.write(", null");
        self.emit_jsx_children_args(&frag.children);
        self.write(")");
    }

    fn emit_jsx_tag_name(&mut self, name: &Expr) {
        match &name.kind {
            ExprKind::Ident(n) => {
                // Resolve unicode escapes to determine intrinsic vs component.
                let resolved = crate::normalize_unicode_escapes(n);
                // Lowercase = intrinsic HTML element (emitted as string)
                // Names containing `:` (namespace prefix) = always intrinsic
                // Uppercase = component reference (emitted as identifier)
                if resolved.contains(':')
                    || resolved.chars().next().is_some_and(|c| c.is_lowercase())
                {
                    // Intrinsic: use resolved name in string
                    self.write("\"");
                    self.write(&resolved);
                    self.write("\"");
                } else {
                    // Component: keep original name (with unicode escapes)
                    let n = n;
                    // Apply CJS import map qualification (e.g. `MySFC` → `component_1.MySFC`)
                    if let Some((var_name, imported)) = self.cjs_import_ref(n, name.span.start) {
                        if imported.is_empty() {
                            self.write(&var_name);
                        } else {
                            self.write_cjs_import_access(&var_name, &imported, n);
                        }
                    // Apply CJS export qualification (e.g. `MySFC` → `exports.MySFC`)
                    } else if self.export_target.as_ref().is_some_and(|t| t == "exports")
                        && self.cjs_var_export_names.contains(n.as_str())
                        && !self.import_shadows.is_shadowed(n, name.span.start)
                    {
                        self.write_cjs_export_access("exports", n);
                    } else {
                        self.write(n);
                    }
                }
            }
            _ => self.emit_expr(name),
        }
    }

    /// In JSX preserve mode, strip private field names from member-expression
    /// tag names when downleveling private fields. `<this.#prop />` → `<this. />`
    /// The parser may report the property as `"<error>"` since `#name` is invalid
    /// in JSX position and the name span may be truncated.
    pub(crate) fn collect_jsx_private_field_replacement(
        &self,
        mem: &tsc_rs_ast::MemberExpr,
        elem_span: tsc_rs_ast::Span,
        reps: &mut Vec<(usize, usize, String)>,
        flag: &mut bool,
    ) {
        if (mem.property.starts_with('#') || mem.property == "<error>")
            && self.needs_downlevel("private-fields")
        {
            let elem_start = elem_span.start as usize;
            let elem_end = (elem_span.end as usize).min(self.source.len());
            if elem_start < elem_end {
                let elem_src = &self.source[elem_start..elem_end];
                if let Some(hash_rel) = elem_src.find('#') {
                    let hash_abs = elem_start + hash_rel;
                    let after_hash = &self.source[hash_abs + 1..elem_end];
                    let name_len = after_hash
                        .find(|c: char| !c.is_alphanumeric() && c != '_')
                        .unwrap_or(after_hash.len());
                    reps.push((hash_abs, hash_abs + 1 + name_len, String::new()));
                    *flag = true;
                }
            }
        }
    }

    /// Namespace qualification for JSX member expression names.
    /// `<S.Bar />` → `<M.S.Bar />` when `S` is a namespace export.
    pub(crate) fn collect_jsx_member_ns_qualification(
        &self,
        mem: &tsc_rs_ast::MemberExpr,
        reps: &mut Vec<(usize, usize, String)>,
    ) {
        // Find the root object of the member chain
        let mut root = &*mem.object;
        while let ExprKind::Member(inner) = &root.kind {
            root = &inner.object;
        }
        if let ExprKind::Ident(name) = &root.kind {
            if self.export_target.as_ref().is_some_and(|t| t != "exports")
                && !self.cjs_param_shadows.contains(name.as_str())
            {
                let qual_target = if self.namespace_exports.contains(name.as_str()) {
                    self.export_target.clone()
                } else if !self.ns_local_bindings.contains(name.as_str()) {
                    self.ns_export_stack
                        .iter()
                        .rev()
                        .find(|(_, exports)| exports.contains(name.as_str()))
                        .map(|(t, _)| t.to_string())
                } else {
                    None
                };
                if let Some(target) = qual_target {
                    let rs = root.span.start as usize;
                    let re = root.span.end as usize;
                    if rs < re && re <= self.source.len() {
                        let qualified = format!("{}.{}", target, name);
                        reps.push((rs, re, qualified));
                    }
                }
            }
        }
    }

    fn emit_jsx_attributes(&mut self, attrs: &[JsxAttribute]) {
        if attrs.is_empty() {
            self.write("null");
            return;
        }

        let has_spread = attrs
            .iter()
            .any(|a| matches!(a, JsxAttribute::Spread(_, _)));

        if has_spread {
            // TypeScript optimizes JSX spread attributes:
            // - Spread of object literals (`{...{ key: val }}`) are inlined into
            //   the adjacent property group.
            // - If after inlining all spreads are object literals, no Object.assign
            //   is needed — emit a single `{ ... }`.
            // - Otherwise, use Object.assign with merged groups.

            // Check if all spreads are object literals (can be fully inlined).
            // An object literal spread is only inlinable if it doesn't itself
            // contain non-object-literal spreads or __proto__ keys.
            let all_spreads_are_obj_lits = attrs.iter().all(|a| match a {
                JsxAttribute::Spread(expr, span) => match &expr.kind {
                    ExprKind::ObjectLit(props) => {
                        jsx_obj_lit_is_fully_inlinable(props)
                            && !self.jsx_has_emitted_comments_in_range(span.start, span.end)
                    }
                    _ => false,
                },
                _ => true,
            });

            // Object spread is native starting at ES2018. Keep an actual
            // spread boundary for arbitrary expressions, __proto__ setters,
            // nested non-literal spreads, and comments. TypeScript still
            // flattens comment-free, fully inlinable object-literal JSX
            // spreads at native targets.
            if !self.needs_downlevel("object-spread") && !all_spreads_are_obj_lits {
                self.emit_jsx_native_spread_props(attrs, None, &[], 0);
                return;
            }

            let first_is_non_inlinable_spread = matches!(
                attrs.first(),
                Some(JsxAttribute::Spread(expr, _)) if !matches!(expr.kind, ExprKind::ObjectLit(_))
            );

            if !all_spreads_are_obj_lits {
                self.write("Object.assign(");
                if first_is_non_inlinable_spread {
                    self.write("{}");
                }
            }

            let mut in_obj = false;
            let mut first_arg = !first_is_non_inlinable_spread || all_spreads_are_obj_lits;
            let mut prop_count = 0usize;

            for attr in attrs {
                match attr {
                    JsxAttribute::Normal { name, value, .. } => {
                        if !in_obj {
                            if !first_arg {
                                self.write(", ");
                            }
                            self.write("{ ");
                            self.indent += 1;
                            in_obj = true;
                        } else if prop_count > 0 {
                            self.write(", ");
                        }
                        prop_count += 1;
                        self.emit_jsx_attr_name(name);
                        self.write(": ");
                        if let Some(val) = value {
                            if self.jsx_attr_value_is_missing_recovery(val) {
                                self.write("true");
                            } else {
                                self.emit_jsx_attr_value(val);
                            }
                        } else {
                            self.write("true");
                        }
                    }
                    JsxAttribute::Spread(spread_expr, spread_span) => {
                        self.jsx_flatten_spread_into_assign(
                            spread_expr,
                            *spread_span,
                            &mut in_obj,
                            &mut first_arg,
                            &mut prop_count,
                            false,
                        );
                    }
                }
                first_arg = false;
            }

            if in_obj {
                self.indent -= 1;
                self.write(" }");
            }
            if !all_spreads_are_obj_lits {
                self.write(")");
            }
        } else {
            // Simple object literal for attributes
            self.write("{ ");
            self.indent += 1;
            let mut emitted_count = 0usize;
            let (main_attrs, _) = self.split_jsx_recovery_trailing_attrs(attrs);
            for attr in main_attrs {
                if let JsxAttribute::Normal { name, value, .. } = attr {
                    if emitted_count > 0 {
                        self.write(", ");
                    }
                    emitted_count += 1;
                    self.emit_jsx_attr_name(name);
                    self.write(": ");
                    if let Some(val) = value {
                        let immediate_spread_recovery =
                            self.jsx_attr_value_is_immediate_spread_recovery(val);
                        if immediate_spread_recovery {
                            self.write(", ");
                            emitted_count += 1;
                            self.emit_jsx_attr_name(name);
                            self.write(": true");
                        } else if self.jsx_attr_value_is_missing_recovery(val) {
                            self.write("true");
                        } else {
                            self.emit_jsx_attr_value(val);
                        }
                        if !immediate_spread_recovery {
                            for recovered in self.jsx_recovered_outer_attrs_from_value(val) {
                                let JsxAttribute::Normal {
                                    name,
                                    value: recovered_val,
                                    ..
                                } = recovered
                                else {
                                    continue;
                                };
                                self.write(", ");
                                emitted_count += 1;
                                self.emit_jsx_attr_name(name);
                                self.write(": ");
                                if let Some(v) = recovered_val {
                                    if self.jsx_attr_value_is_missing_recovery(v) {
                                        self.write("true");
                                    } else {
                                        self.emit_expr(v);
                                    }
                                } else {
                                    self.write("true");
                                }
                            }
                        }
                    } else {
                        self.write("true");
                    }
                }
            }
            self.indent -= 1;
            self.write(" }");
        }
    }

    fn emit_jsx_children_args(&mut self, children: &[JsxChild]) {
        // Count non-whitespace-only children for multiline formatting.
        // Empty JSX expressions like `{/* comment */}` count for formatting
        // (they occupy source lines) even though they don't produce output.
        let source_child_count = children
            .iter()
            .filter(|c| match c {
                JsxChild::Text(text, _) => !jsx_trim_text(text).is_empty(),
                JsxChild::Element(expr) => !self.jsx_child_is_recovery_error(expr),
                JsxChild::Expression(Some(expr), _) => !expr_is_error_placeholder(expr),
                JsxChild::Expression(None, _) => false,
                _ => true,
            })
            .count();
        // Use multiline formatting when there are 2+ source children
        // and at least one is a complex child (element, fragment, or non-trivial expression).
        // Simple literals like `null` don't trigger multiline on their own.
        let has_complex_child = children.iter().any(|c| match c {
            JsxChild::Element(expr) => !self.jsx_child_is_recovery_error(expr),
            JsxChild::Fragment(_) => true,
            JsxChild::Expression(Some(e), _) => {
                !expr_is_error_placeholder(e)
                    && !matches!(
                        e.kind,
                        ExprKind::Ident(_)
                            | ExprKind::NumLit(_)
                            | ExprKind::StrLit(_)
                            | ExprKind::BoolLit(_)
                            | ExprKind::NullLit
                    )
            }
            _ => false,
        });
        let single_complex_child_on_newline = if source_child_count == 1 && has_complex_child {
            let child_start = children.iter().find_map(|c| match c {
                JsxChild::Text(text, _) if jsx_trim_text(text).is_empty() => None,
                JsxChild::Element(expr) if self.jsx_child_is_recovery_error(expr) => None,
                JsxChild::Expression(Some(expr), _) if expr_is_error_placeholder(expr) => None,
                JsxChild::Element(expr) => Some(expr.span.start as usize),
                JsxChild::Fragment(frag) => frag.children.first().and_then(|fc| match fc {
                    JsxChild::Text(_, span) => Some(span.start as usize),
                    JsxChild::Element(expr) => Some(expr.span.start as usize),
                    JsxChild::Expression(_, span) => Some(span.start as usize),
                    JsxChild::Fragment(_) => None,
                }),
                // Keep expression-container children inline to match TS baseline style.
                JsxChild::Text(_, _) | JsxChild::Expression(_, _) => None,
            });
            child_start.is_some_and(|start| {
                if start == 0 || start > self.source.len() {
                    return false;
                }
                let mut i = start;
                while i > 0 {
                    let b = self.source.as_bytes()[i - 1];
                    if b == b' ' || b == b'\t' || b == b'\r' {
                        i -= 1;
                        continue;
                    }
                    return b == b'\n';
                }
                false
            })
        } else {
            false
        };
        // TSC always wraps element/fragment children to a new line, even when
        // on the same source line. Only expression children stay inline.
        let has_element_or_fragment_child = children.iter().any(|c| match c {
            JsxChild::Element(expr) => !self.jsx_child_is_recovery_error(expr),
            JsxChild::Fragment(_) => true,
            _ => false,
        });
        let multiline = source_child_count >= 2
            || single_complex_child_on_newline
            || (source_child_count == 1 && has_element_or_fragment_child);
        if multiline {
            self.indent += 1;
        }
        for child in children {
            match child {
                JsxChild::Text(text, _) => {
                    let trimmed = jsx_trim_text(text);
                    if !trimmed.is_empty() {
                        self.write(",");
                        if multiline {
                            self.newline();
                        } else {
                            self.write(" ");
                        }
                        self.write("\"");
                        self.write(&jsx_escape_string_decoded(&jsx_decode_entities(&trimmed)));
                        self.write("\"");
                    }
                }
                JsxChild::Element(child_expr) => {
                    if self.jsx_child_is_recovery_error(child_expr) {
                        continue;
                    }
                    self.write(",");
                    if multiline {
                        self.newline();
                    } else {
                        self.write(" ");
                    }
                    self.emit_jsx_expression_value(child_expr);
                }
                JsxChild::Expression(Some(child_expr), span) => {
                    if expr_is_error_placeholder(child_expr) {
                        continue;
                    }
                    self.write(",");
                    if multiline {
                        self.newline();
                    } else {
                        self.write(" ");
                    }
                    self.emit_jsx_expression_value(child_expr);
                    if self.options.remove_comments != Some(true) {
                        if let Some(comment) =
                            jsx_expr_trailing_block_comment(self.source, child_expr.span, *span)
                        {
                            self.write(" ");
                            self.write(comment);
                        }
                    }
                }
                JsxChild::Expression(None, _) => {}
                JsxChild::Fragment(frag) => {
                    self.write(",");
                    if multiline {
                        self.newline();
                    } else {
                        self.write(" ");
                    }
                    self.emit_jsx_fragment(frag);
                }
            }
        }
        if multiline {
            self.indent -= 1;
        }
    }

    // -----------------------------------------------------------------------
    // react-jsx / react-jsxdev transform
    // -----------------------------------------------------------------------

    /// Emit the CJS `require()` calls for the JSX runtime.
    /// Called during the CJS module init phase, AFTER helpers but BEFORE __esModule.
    pub(crate) fn emit_jsx_runtime_cjs_require(&mut self) {
        if !self.jsx_is_react_jsx() {
            return;
        }
        // Only emit if we actually found JSX usage during pre-scan
        if !self.jsx_runtime_needs_jsx
            && !self.jsx_runtime_needs_jsxs
            && !self.jsx_runtime_needs_fragment
            && !self.jsx_runtime_needs_create_element
        {
            return;
        }

        let import_source = self.jsx_import_source().to_string();

        // If createElement fallback is needed, emit require for the base module
        if self.jsx_runtime_needs_create_element {
            let base_var = self.next_require_var(&import_source);
            self.write(self.generated_require_binding_keyword());
            self.write(" ");
            self.write(&base_var);
            self.write(" = require(\"");
            self.write(&import_source);
            self.writeln("\");");
            self.jsx_runtime_base_require_var = Some(base_var);
        }

        let needs_runtime = self.jsx_runtime_needs_jsx
            || self.jsx_runtime_needs_jsxs
            || self.jsx_runtime_needs_fragment;
        if !needs_runtime {
            return;
        }

        // Emit require for the jsx-runtime module
        let runtime_module = self.jsx_runtime_module();
        let runtime_var = self.next_require_var(&runtime_module);
        self.write(self.generated_require_binding_keyword());
        self.write(" ");
        self.write(&runtime_var);
        self.write(" = require(\"");
        self.write(&runtime_module);
        self.writeln("\");");
        self.jsx_runtime_require_var = Some(runtime_var);

        // In dev mode, emit the filename constant
        if self.jsx_is_dev() {
            self.emit_jsx_dev_filename_const();
        }
    }

    /// Emit the ESM `import` statements for the JSX runtime.
    /// Called during the ESM module init phase.
    pub(crate) fn emit_jsx_runtime_esm_import(&mut self) {
        if !self.jsx_is_react_jsx() {
            return;
        }
        if !self.jsx_runtime_needs_jsx
            && !self.jsx_runtime_needs_jsxs
            && !self.jsx_runtime_needs_fragment
            && !self.jsx_runtime_needs_create_element
        {
            return;
        }

        let import_source = self.jsx_import_source().to_string();

        // If createElement fallback is needed, emit import for the base module
        if self.jsx_runtime_needs_create_element {
            self.write("import { createElement as _createElement } from \"");
            self.write(&import_source);
            self.writeln("\";");
            self.emitted_esm_export = true;
        }

        // Build the import specifiers list
        let mut specifiers = Vec::new();
        let is_dev = self.jsx_is_dev();
        if self.jsx_runtime_needs_jsx
            || self.jsx_runtime_needs_jsxs
            || self.jsx_runtime_needs_fragment
        {
            if is_dev {
                specifiers.push("jsxDEV as _jsxDEV");
            } else {
                if self.jsx_runtime_needs_jsx {
                    specifiers.push("jsx as _jsx");
                }
                if self.jsx_runtime_needs_jsxs {
                    specifiers.push("jsxs as _jsxs");
                }
            }
            if self.jsx_runtime_needs_fragment {
                specifiers.push("Fragment as _Fragment");
            }
        }

        if !specifiers.is_empty() {
            let runtime_module = self.jsx_runtime_module();
            self.write("import { ");
            for (i, spec) in specifiers.iter().enumerate() {
                if i > 0 {
                    self.write(", ");
                }
                self.write(spec);
            }
            self.write(" } from \"");
            self.write(&runtime_module);
            self.writeln("\";");
            self.emitted_esm_export = true;
        }

        // In dev mode, emit the filename constant
        if is_dev && !specifiers.is_empty() {
            self.emit_jsx_dev_filename_const();
        }
    }

    /// Emit `const _jsxFileName = "filename.tsx";` for react-jsxdev mode.
    fn emit_jsx_dev_filename_const(&mut self) {
        let normalized_file_name = self.jsx_dev_file_name.replace('\\', "/");
        let file_name = jsx_escape_string_decoded(&normalized_file_name);
        self.write("const ");
        self.write(&self.jsx_dev_file_name_ident.clone());
        self.write(" = \"");
        self.write(&file_name);
        self.writeln("\";");
    }

    /// Emit a JSX element using the react-jsx transform.
    pub(crate) fn emit_jsx_element_react_jsx(&mut self, el: &JsxElement, _expr_span: Span) {
        // Check for key-after-spread fallback
        if has_key_after_spread(&el.attributes) {
            self.emit_jsx_create_element_fallback(&el.name, &el.attributes, &el.children);
            return;
        }

        let child_count = count_effective_children(&el.children);
        let use_jsxs = jsx_runtime_children_need_array(&el.children);
        let key = find_key_attr(&el.attributes);
        let key_index = find_key_attr_index(&el.attributes);

        // Emit the function call
        self.emit_jsx_runtime_call_start(use_jsxs, _expr_span);
        self.emit_jsx_tag_name(&el.name);
        self.write(", ");

        // Build the props object
        let has_spread = el
            .attributes
            .iter()
            .any(|a| matches!(a, JsxAttribute::Spread(_, _)));
        let spreads_inlinable = jsx_all_spreads_inlinable(&el.attributes)
            && el.attributes.iter().all(|attr| match attr {
                JsxAttribute::Spread(_, span) => {
                    !self.jsx_has_emitted_comments_in_range(span.start, span.end)
                }
                _ => true,
            });
        if has_spread && !spreads_inlinable {
            if self.needs_downlevel("object-spread") {
                self.emit_jsx_runtime_props_with_spread(
                    &el.attributes,
                    key_index,
                    &el.children,
                    child_count,
                );
            } else {
                self.emit_jsx_native_spread_props(
                    &el.attributes,
                    key_index,
                    &el.children,
                    child_count,
                );
            }
        } else if has_spread {
            self.emit_jsx_runtime_props_merged(
                &el.attributes,
                key_index,
                &el.children,
                child_count,
            );
        } else {
            self.emit_jsx_runtime_props(&el.attributes, key_index, &el.children, child_count);
        }

        // Emit key as 3rd argument if present
        if let Some(key_attr) = key {
            self.write(", ");
            self.emit_jsx_key_attr_value(key_attr);
        }

        // In dev mode, emit additional arguments
        if self.jsx_is_dev() {
            self.emit_jsx_dev_extra_args(key.is_some(), use_jsxs, _expr_span);
        }

        self.write(")");
    }

    /// Emit a self-closing JSX element using the react-jsx transform.
    pub(crate) fn emit_jsx_self_closing_react_jsx(
        &mut self,
        el: &JsxSelfClosingElement,
        _expr_span: Span,
    ) {
        // Check for key-after-spread fallback
        if has_key_after_spread(&el.attributes) {
            self.emit_jsx_create_element_fallback(&el.name, &el.attributes, &[]);
            return;
        }

        let key = find_key_attr(&el.attributes);
        let key_index = find_key_attr_index(&el.attributes);

        // Emit the function call
        self.emit_jsx_runtime_call_start(false, _expr_span);
        self.emit_jsx_tag_name(&el.name);
        self.write(", ");

        // Build the props object
        let has_spread = el
            .attributes
            .iter()
            .any(|a| matches!(a, JsxAttribute::Spread(_, _)));
        let spreads_inlinable = jsx_all_spreads_inlinable(&el.attributes)
            && el.attributes.iter().all(|attr| match attr {
                JsxAttribute::Spread(_, span) => {
                    !self.jsx_has_emitted_comments_in_range(span.start, span.end)
                }
                _ => true,
            });
        if has_spread && !spreads_inlinable {
            if self.needs_downlevel("object-spread") {
                self.emit_jsx_runtime_props_with_spread(&el.attributes, key_index, &[], 0);
            } else {
                self.emit_jsx_native_spread_props(&el.attributes, key_index, &[], 0);
            }
        } else if has_spread {
            self.emit_jsx_runtime_props_merged(&el.attributes, key_index, &[], 0);
        } else {
            self.emit_jsx_runtime_props(&el.attributes, key_index, &[], 0);
        }

        // Emit key as 3rd argument if present
        if let Some(key_attr) = key {
            self.write(", ");
            self.emit_jsx_key_attr_value(key_attr);
        }

        // In dev mode, emit additional arguments
        if self.jsx_is_dev() {
            self.emit_jsx_dev_extra_args(key.is_some(), false, _expr_span);
        }

        self.write(")");
    }

    /// Emit a JSX fragment using the react-jsx transform.
    pub(crate) fn emit_jsx_fragment_react_jsx(&mut self, frag: &JsxFragment, _expr_span: Span) {
        let child_count = count_effective_children(&frag.children);
        let use_jsxs = jsx_runtime_children_need_array(&frag.children);

        // Emit the function call
        self.emit_jsx_runtime_call_start(use_jsxs, _expr_span);

        // Fragment reference
        if self.is_cjs_like() {
            if let Some(ref var) = self.jsx_runtime_require_var.clone() {
                self.write(var);
                self.write(".Fragment");
            } else {
                self.write("_Fragment");
            }
        } else {
            self.write("_Fragment");
        }

        self.write(", ");

        // Props object with children
        self.write("{ ");
        self.emit_jsx_runtime_children_prop(&frag.children, child_count);
        self.write(" }");

        // In dev mode, emit additional arguments
        if self.jsx_is_dev() {
            self.emit_jsx_dev_extra_args(false, use_jsxs, _expr_span);
        }

        self.write(")");
    }

    /// Emit the start of a jsx/jsxs/jsxDEV call.
    fn emit_jsx_runtime_call_start(&mut self, use_jsxs: bool, _expr_span: Span) {
        if self.is_cjs_like() {
            let var = self.jsx_runtime_require_var.clone().unwrap_or_default();
            if self.jsx_is_dev() {
                self.write("(0, ");
                self.write(&var);
                self.write(".jsxDEV)(");
            } else if use_jsxs {
                self.write("(0, ");
                self.write(&var);
                self.write(".jsxs)(");
            } else {
                self.write("(0, ");
                self.write(&var);
                self.write(".jsx)(");
            }
        } else {
            // ESM mode
            if self.jsx_is_dev() {
                self.write("_jsxDEV(");
            } else if use_jsxs {
                self.write("_jsxs(");
            } else {
                self.write("_jsx(");
            }
        }
    }

    /// Emit the props object for react-jsx (without spreads).
    fn emit_jsx_runtime_props(
        &mut self,
        attrs: &[JsxAttribute],
        omitted_key_index: Option<usize>,
        children: &[JsxChild],
        child_count: usize,
    ) {
        // Omit only the key expression extracted as the call's third argument.
        // Later duplicate keys are recovery nodes whose evaluation must remain.
        let non_key_attrs: Vec<(usize, &JsxAttribute)> = attrs
            .iter()
            .enumerate()
            .filter(|(index, attr)| {
                !(omitted_key_index == Some(*index)
                    && matches!(attr, JsxAttribute::Normal { name, .. } if name == "key"))
            })
            .collect();

        if non_key_attrs.is_empty() && child_count == 0 {
            self.write("{}");
            return;
        }

        self.write("{ ");
        let mut first = true;
        for (_, attr) in &non_key_attrs {
            if let JsxAttribute::Normal { name, value, .. } = attr {
                if !first {
                    self.write(", ");
                }
                first = false;
                self.emit_jsx_attr_name(name);
                self.write(": ");
                if let Some(val) = value {
                    if self.jsx_attr_value_is_missing_recovery(val) {
                        self.write("true");
                    } else {
                        self.emit_jsx_attr_value(val);
                    }
                } else {
                    self.write("true");
                }
            }
        }

        // Add children prop
        if child_count > 0 {
            if !first {
                self.write(", ");
            }
            self.emit_jsx_runtime_children_prop(children, child_count);
        }

        self.write(" }");
    }

    /// Emit the props object for react-jsx when all spread attributes are
    /// inlinable object literals. Merges everything into a single plain object.
    fn emit_jsx_runtime_props_merged(
        &mut self,
        attrs: &[JsxAttribute],
        omitted_key_index: Option<usize>,
        children: &[JsxChild],
        child_count: usize,
    ) {
        let non_key_attrs: Vec<(usize, &JsxAttribute)> = attrs
            .iter()
            .enumerate()
            .filter(|(index, attr)| {
                !(omitted_key_index == Some(*index)
                    && matches!(attr, JsxAttribute::Normal { name, .. } if name == "key"))
            })
            .collect();

        if non_key_attrs.is_empty() && child_count == 0 {
            self.write("{}");
            return;
        }

        self.write("{ ");
        let mut first = true;

        for (_, attr) in &non_key_attrs {
            match attr {
                JsxAttribute::Normal { name, value, .. } => {
                    if !first {
                        self.write(", ");
                    }
                    first = false;
                    self.emit_jsx_attr_name(name);
                    self.write(": ");
                    if let Some(val) = value {
                        if self.jsx_attr_value_is_missing_recovery(val) {
                            self.write("true");
                        } else {
                            self.emit_jsx_attr_value(val);
                        }
                    } else {
                        self.write("true");
                    }
                }
                JsxAttribute::Spread(expr, _) => {
                    // Inline all properties from the spread object literal
                    if let ExprKind::ObjectLit(props) = &expr.kind {
                        self.jsx_inline_obj_lit_props(props, &mut first);
                    }
                }
            }
        }

        // Add children prop
        if child_count > 0 {
            if !first {
                self.write(", ");
            }
            self.emit_jsx_runtime_children_prop(children, child_count);
        }

        self.write(" }");
    }

    /// Emit one native object literal for JSX props at ES2018 and newer.
    /// Top-level object-literal spread attributes are flattened, while nested
    /// spreads stay as spreads. An object containing a `__proto__` setter must
    /// remain wrapped in a spread so it copies a property instead of mutating
    /// the prototype of the props object.
    fn emit_jsx_native_spread_props(
        &mut self,
        attrs: &[JsxAttribute],
        omitted_key_index: Option<usize>,
        children: &[JsxChild],
        child_count: usize,
    ) {
        self.write("{ ");
        let mut first = true;

        for (index, attr) in attrs.iter().enumerate() {
            match attr {
                JsxAttribute::Normal { name, value, .. } => {
                    if omitted_key_index == Some(index) && name == "key" {
                        continue;
                    }
                    if !first {
                        self.write(", ");
                    }
                    first = false;
                    self.emit_jsx_attr_name(name);
                    self.write(": ");
                    if let Some(value) = value {
                        if self.jsx_attr_value_is_missing_recovery(value) {
                            self.write("true");
                        } else {
                            self.emit_jsx_attr_value(value);
                        }
                    } else {
                        self.write("true");
                    }
                }
                JsxAttribute::Spread(expr, span) => {
                    self.emit_jsx_native_spread_expr(expr, *span, &mut first);
                }
            }
        }

        if child_count > 0 {
            if !first {
                self.write(", ");
            }
            self.emit_jsx_runtime_children_prop(children, child_count);
        }

        self.write(" }");
    }

    fn emit_jsx_native_spread_expr(&mut self, expr: &Expr, spread_span: Span, first: &mut bool) {
        let ExprKind::ObjectLit(props) = &expr.kind else {
            if !*first {
                self.write(", ");
            }
            *first = false;
            self.emit_jsx_native_spread_operand(expr, spread_span);
            return;
        };

        if !jsx_obj_lit_is_native_flatten_safe(props)
            || self.jsx_has_emitted_comments_in_range(spread_span.start, spread_span.end)
        {
            if !*first {
                self.write(", ");
            }
            *first = false;
            self.emit_jsx_native_spread_object_operand(expr, props, spread_span);
            return;
        }

        for prop in props {
            if !*first {
                self.write(", ");
            }
            *first = false;
            match prop {
                ObjLitProp::Spread(inner, _) => {
                    self.write("...");
                    self.emit_expr(inner);
                }
                _ => self.emit_obj_lit_prop(prop),
            }
        }
    }

    fn emit_jsx_native_spread_operand(&mut self, expr: &Expr, spread_span: Span) {
        self.emit_jsx_native_spread_prefix(expr, spread_span);
        self.emit_jsx_expression_value(expr);
        self.emit_inline_comments_in_range(expr.span.end, spread_span.end);
    }

    /// Emit an object-literal spread without flattening it, while retaining
    /// comments that the compact object-literal path would otherwise skip.
    /// The properties still go through the normal emitter so TypeScript-only
    /// syntax (for example setter parameter types) is erased correctly.
    fn emit_jsx_native_spread_object_operand(
        &mut self,
        expr: &Expr,
        props: &[ObjLitProp],
        spread_span: Span,
    ) {
        self.emit_jsx_native_spread_prefix(expr, spread_span);
        self.emit_jsx_comment_aware_object_literal(expr, props);
        self.emit_inline_comments_in_range(expr.span.end, spread_span.end);
    }

    fn emit_jsx_comment_aware_object_literal(&mut self, expr: &Expr, props: &[ObjLitProp]) {
        self.write("{ ");
        if props.is_empty() {
            self.emit_jsx_empty_object_comments(expr.span.start, expr.span.end);
        }
        for (index, prop) in props.iter().enumerate() {
            if index > 0 {
                self.write(", ");
            }
            let prop_span = super::obj_lit_prop_span(prop);
            self.emit_leading_comments(prop_span.start);
            self.emit_obj_lit_prop(prop);
        }
        if let Some(last) = props.last() {
            self.emit_inline_comments_in_range(super::obj_lit_prop_span(last).end, expr.span.end);
        }
        self.write(" }");
    }

    fn emit_jsx_empty_object_comments(&mut self, start: u32, end: u32) {
        if self.options.remove_comments == Some(true) && !self.preserve_comments {
            return;
        }
        while self.next_comment_idx < self.comments.len() {
            let comment = &self.comments[self.next_comment_idx];
            if comment.pos >= end {
                break;
            }
            if comment.pos < start || comment.pos < self.comment_emit_pos {
                self.next_comment_idx += 1;
                continue;
            }
            let comment_end = comment.end;
            let text = self.source
                [comment.pos as usize..(comment.end as usize).min(self.source.len())]
                .trim_end_matches('\r')
                .to_string();
            self.next_comment_idx += 1;
            if text.starts_with("//") {
                self.write(&text);
                self.newline();
            } else {
                self.write(&text);
                self.write(" ");
            }
            self.comment_emit_pos = self.comment_emit_pos.max(comment_end);
        }
    }

    fn emit_jsx_native_spread_prefix(&mut self, expr: &Expr, spread_span: Span) {
        self.write("...");
        self.emit_jsx_comments_after_spread_dots(expr, spread_span);
    }

    fn emit_jsx_comments_after_spread_dots(&mut self, expr: &Expr, spread_span: Span) {
        let start = (spread_span.start as usize).min(self.source.len());
        let expr_start = (expr.span.start as usize).min(self.source.len());
        if start < expr_start {
            if let Some(dots) = self.source[start..expr_start].find("...") {
                let dots_end = start + dots + 3;
                if !self.emit_compact_spread_comment(dots_end) {
                    self.emit_compact_comment_between(dots_end, expr_start, true);
                }
            }
        }
    }

    /// Inline object literal properties into the current output.
    /// Recursively flattens nested spreads of inlinable object literals.
    fn jsx_inline_obj_lit_props(&mut self, props: &[ObjLitProp], first: &mut bool) {
        for prop in props {
            match prop {
                ObjLitProp::Spread(expr, _) => {
                    if let ExprKind::ObjectLit(inner) = &expr.kind {
                        self.jsx_inline_obj_lit_props(inner, first);
                    }
                }
                ObjLitProp::Property(p) => {
                    if !*first {
                        self.write(", ");
                    }
                    *first = false;
                    self.emit_prop_name(&p.key);
                    self.write(": ");
                    self.emit_expr(&p.value);
                }
                ObjLitProp::Shorthand(name, _) => {
                    if !*first {
                        self.write(", ");
                    }
                    *first = false;
                    self.write(name);
                }
                ObjLitProp::ShorthandDefault(name, init, _) => {
                    if !*first {
                        self.write(", ");
                    }
                    *first = false;
                    self.write(name);
                    self.write(" = ");
                    self.emit_expr(init);
                }
                _ => {
                    // Methods, getters, setters — unlikely in JSX spreads
                }
            }
        }
    }

    /// Emit the props object for react-jsx with spread attributes.
    /// Uses Object.assign to merge spread props with the rest.
    fn emit_jsx_runtime_props_with_spread(
        &mut self,
        attrs: &[JsxAttribute],
        omitted_key_index: Option<usize>,
        children: &[JsxChild],
        child_count: usize,
    ) {
        let non_key_attrs: Vec<(usize, &JsxAttribute)> = attrs
            .iter()
            .enumerate()
            .filter(|(index, attr)| {
                !(omitted_key_index == Some(*index)
                    && matches!(attr, JsxAttribute::Normal { name, .. } if name == "key"))
            })
            .collect();

        // Check if first non-key attribute is a spread
        let first_is_spread =
            matches!(non_key_attrs.first(), Some((_, JsxAttribute::Spread(_, _))));

        self.write("Object.assign(");
        if first_is_spread {
            self.write("{}");
        }

        // Group consecutive normal attributes together, spreads as separate args
        let mut in_obj = false;
        let mut first_arg = !first_is_spread;
        let mut prop_count = 0usize;

        for (_, attr) in &non_key_attrs {
            match attr {
                JsxAttribute::Normal { name, value, .. } => {
                    if !in_obj {
                        if !first_arg {
                            self.write(", ");
                        }
                        self.write("{ ");
                        self.indent += 1;
                        in_obj = true;
                    } else if prop_count > 0 {
                        self.write(", ");
                    }
                    prop_count += 1;
                    self.emit_jsx_attr_name(name);
                    self.write(": ");
                    if let Some(val) = value {
                        if self.jsx_attr_value_is_missing_recovery(val) {
                            self.write("true");
                        } else {
                            self.emit_jsx_attr_value(val);
                        }
                    } else {
                        self.write("true");
                    }
                }
                JsxAttribute::Spread(expr, spread_span) => {
                    self.jsx_flatten_spread_into_assign(
                        expr,
                        *spread_span,
                        &mut in_obj,
                        &mut first_arg,
                        &mut prop_count,
                        true,
                    );
                    // first_arg already set by flatten
                    continue;
                }
            }
            first_arg = false;
        }

        if in_obj {
            self.indent -= 1;
            self.write(" }");
        }

        // Add children
        if child_count > 0 {
            self.write(", { ");
            self.emit_jsx_runtime_children_prop(children, child_count);
            self.write(" }");
        }

        self.write(")");
    }

    /// Emit the `children: ...` prop value.
    fn emit_jsx_runtime_children_prop(&mut self, children: &[JsxChild], child_count: usize) {
        self.write("children: ");
        let needs_array = child_count > 1 || jsx_runtime_has_spread_child(children);
        if needs_array {
            self.write("[");
        }

        let mut first = true;
        for child in children {
            match child {
                JsxChild::Text(text, _) => {
                    let trimmed = jsx_trim_text(text);
                    if !trimmed.is_empty() {
                        if !first {
                            self.write(", ");
                        }
                        first = false;
                        self.write("\"");
                        self.write(&jsx_escape_string_decoded(&jsx_decode_entities(&trimmed)));
                        self.write("\"");
                    }
                }
                JsxChild::Element(child_expr) => {
                    if self.jsx_child_is_recovery_error(child_expr) {
                        continue;
                    }
                    if !first {
                        self.write(", ");
                    }
                    first = false;
                    self.emit_jsx_expression_value(child_expr);
                }
                JsxChild::Expression(Some(child_expr), _) => {
                    if expr_is_error_placeholder(child_expr) {
                        continue;
                    }
                    if !first {
                        self.write(", ");
                    }
                    first = false;
                    self.emit_jsx_expression_value(child_expr);
                }
                JsxChild::Expression(None, _) => {}
                JsxChild::Fragment(frag) => {
                    if !first {
                        self.write(", ");
                    }
                    first = false;
                    // Use the expr span of the containing expression context
                    // For nested fragments, use a zero span as placeholder
                    self.emit_jsx_fragment_react_jsx(frag, Span::new(0, 0));
                }
            }
        }

        if needs_array {
            self.write("]");
        }
    }

    /// Emit the createElement fallback for key-after-spread pattern.
    fn emit_jsx_create_element_fallback(
        &mut self,
        name: &Expr,
        attrs: &[JsxAttribute],
        children: &[JsxChild],
    ) {
        // In CJS, use (0, react_1.createElement)(...)
        // In ESM, use _createElement(...)
        if self.is_cjs_like() {
            if let Some(ref var) = self.jsx_runtime_base_require_var.clone() {
                self.write("(0, ");
                self.write(var);
                self.write(".createElement)(");
            } else {
                self.write("_createElement(");
            }
        } else {
            self.write("_createElement(");
        }

        self.emit_jsx_tag_name(name);
        self.write(", ");

        // For createElement fallback, use Object.assign to merge spread + key attrs
        // Key stays in the props (not extracted as 3rd argument)
        let has_spread = attrs
            .iter()
            .any(|a| matches!(a, JsxAttribute::Spread(_, _)));
        if has_spread {
            if self.needs_downlevel("object-spread") {
                self.write("Object.assign({}");
                for attr in attrs {
                    self.write(", ");
                    match attr {
                        JsxAttribute::Normal { name, value, .. } => {
                            self.write("{ ");
                            self.emit_jsx_attr_name(name);
                            self.write(": ");
                            if let Some(val) = value {
                                self.emit_jsx_attr_value(val);
                            } else {
                                self.write("true");
                            }
                            self.write(" }");
                        }
                        JsxAttribute::Spread(expr, _) => {
                            self.emit_jsx_expression_value(expr);
                        }
                    }
                }
                self.write(")");
            } else {
                self.emit_jsx_native_spread_props(attrs, None, &[], 0);
            }
        } else {
            self.emit_jsx_attributes(attrs);
        }

        // Children as separate arguments (like classic React.createElement)
        for child in children {
            match child {
                JsxChild::Text(text, _) => {
                    let trimmed = jsx_trim_text(text);
                    if !trimmed.is_empty() {
                        self.write(", \"");
                        self.write(&jsx_escape_string_decoded(&jsx_decode_entities(&trimmed)));
                        self.write("\"");
                    }
                }
                JsxChild::Element(child_expr) => {
                    self.write(", ");
                    self.emit_jsx_expression_value(child_expr);
                }
                JsxChild::Expression(Some(child_expr), _) => {
                    self.write(", ");
                    self.emit_jsx_expression_value(child_expr);
                }
                JsxChild::Expression(None, _) => {}
                JsxChild::Fragment(frag) => {
                    self.write(", ");
                    self.emit_jsx_fragment(frag);
                }
            }
        }

        self.write(")");
    }

    /// Emit extra arguments for jsxDEV mode:
    /// key, isStaticChildren, { fileName, lineNumber, columnNumber }, this
    fn emit_jsx_dev_extra_args(
        &mut self,
        has_key: bool,
        is_static_children: bool,
        expr_span: Span,
    ) {
        // The production path already emitted an explicit key as the third
        // argument. Only synthesize the empty key slot when no key exists.
        if !has_key {
            self.write(", void 0");
        }

        // isStaticChildren
        self.write(", ");
        if is_static_children {
            self.write("true");
        } else {
            self.write("false");
        }

        // source location object
        self.write(", { fileName: ");
        self.write(&self.jsx_dev_file_name_ident.clone());
        if expr_span.end > expr_span.start {
            // Compute line/column from the source
            let (line, col) = self.compute_source_line_col(expr_span.start);
            self.write(", lineNumber: ");
            self.write(&line.to_string());
            self.write(", columnNumber: ");
            self.write(&col.to_string());
        }
        self.write(" }");

        // this
        self.write(", this");
    }

    /// Compute the 1-based line number and 1-based column number for a source offset.
    fn compute_source_line_col(&self, offset: u32) -> (u32, u32) {
        let offset = offset as usize;
        let text = &self.source[..offset.min(self.source.len())];
        let mut line = 1u32;
        let mut line_start = 0usize;
        let mut chars = text.char_indices().peekable();
        while let Some((index, ch)) = chars.next() {
            let mut next_line_start = index + ch.len_utf8();
            let is_line_break = match ch {
                '\r' => {
                    if chars.peek().is_some_and(|(_, next)| *next == '\n') {
                        let (lf_index, lf) = chars.next().expect("peeked CRLF tail");
                        next_line_start = lf_index + lf.len_utf8();
                    }
                    true
                }
                '\n' | '\u{2028}' | '\u{2029}' => true,
                _ => false,
            };
            if is_line_break {
                line += 1;
                line_start = next_line_start;
            }
        }
        let col = text[line_start..].encode_utf16().count() as u32 + 1;
        (line, col)
    }

    /// Scan source text for @jsxImportSource pragma in leading comments.
    pub(crate) fn scan_jsx_import_source_pragma(&mut self) {
        // Look for /* @jsxImportSource <source> */ or /** @jsxImportSource <source> */
        // in the first few comments of the file.
        for comment in self.comments.iter().take(10) {
            if !comment.is_multiline {
                continue;
            }
            let start = comment.pos as usize;
            let end = comment.end as usize;
            if end > self.source.len() || start >= end {
                continue;
            }
            let text = &self.source[start..end];
            if let Some(idx) = text.find("@jsxImportSource") {
                let rest = &text[idx + "@jsxImportSource".len()..];
                let rest = rest.trim_start();
                // Take until whitespace or end of comment
                let source: String = rest
                    .chars()
                    .take_while(|c| !c.is_whitespace() && *c != '*')
                    .collect();
                if !source.is_empty() {
                    self.jsx_import_source_pragma = Some(source);
                    return;
                }
            }
        }
    }

    /// Scan source text for `@jsxRuntime automatic` or `@jsxRuntime classic` pragma
    /// in leading comments.  When `@jsxRuntime automatic` is found, the file uses
    /// the automatic JSX runtime even if the global `jsx` option is `react` (classic).
    /// The LAST `@jsxRuntime` pragma in the leading comments wins.
    pub(crate) fn scan_jsx_runtime_pragma(&mut self) {
        let mut found_automatic = false;
        for comment in self.comments.iter().take(20) {
            let start = comment.pos as usize;
            let end = comment.end as usize;
            if end > self.source.len() || start >= end {
                continue;
            }
            let text = &self.source[start..end];
            if let Some(idx) = text.find("@jsxRuntime") {
                let rest = &text[idx + "@jsxRuntime".len()..];
                let rest = rest.trim_start();
                if rest.starts_with("automatic") {
                    found_automatic = true;
                } else if rest.starts_with("classic") {
                    found_automatic = false;
                }
            }
        }
        self.jsx_runtime_pragma_automatic = found_automatic;
    }

    /// Scan source text for `@jsx <factory>` and `@jsxFrag <fragment>` pragmas
    /// in leading comments.  These override the compiler-option jsxFactory and
    /// jsxFragmentFactory for this file.  Both can appear in the same comment.
    pub(crate) fn scan_jsx_factory_pragmas(&mut self) {
        for comment in self.comments.iter().take(20) {
            let start = comment.pos as usize;
            let end = comment.end as usize;
            if end > self.source.len() || start >= end {
                continue;
            }
            let text = &self.source[start..end];
            // Scan for @jsxFrag / @jsxfrag first (more specific match).
            if self.jsx_pragma_fragment.is_none() {
                for frag_tag in ["@jsxFrag", "@jsxfrag"] {
                    if let Some(idx) = text.find(frag_tag) {
                        let rest = &text[idx + frag_tag.len()..];
                        let rest = rest.trim_start();
                        let name: String = rest
                            .chars()
                            .take_while(|c| !c.is_whitespace() && *c != '*' && *c != '/')
                            .collect();
                        if !name.is_empty() {
                            self.jsx_pragma_fragment = Some(name);
                            break;
                        }
                    }
                }
            }

            // Scan for bare @jsx (not @jsxFrag, @jsxImportSource, @jsxRuntime).
            // Find all occurrences in the text since one comment may contain
            // both @jsx and @jsxFrag.
            if self.jsx_pragma_factory.is_none() {
                let mut search_from = 0;
                while let Some(rel_idx) = text[search_from..].find("@jsx") {
                    let idx = search_from + rel_idx;
                    let after_jsx = &text[idx + 4..];
                    search_from = idx + 4;
                    // Skip known compound pragmas
                    if after_jsx.starts_with("Frag")
                        || after_jsx.starts_with("frag")
                        || after_jsx.starts_with("Import")
                        || after_jsx.starts_with("Runtime")
                        || after_jsx.starts_with("runtime")
                    {
                        continue;
                    }
                    let rest = after_jsx.trim_start();
                    let name: String = rest
                        .chars()
                        .take_while(|c| !c.is_whitespace() && *c != '*' && *c != '/')
                        .collect();
                    if !name.is_empty() {
                        self.jsx_pragma_factory = Some(name);
                        break;
                    }
                }
            }
        }
    }
}
