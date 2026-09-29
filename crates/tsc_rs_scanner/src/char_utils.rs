//! Character classification utilities for the TypeScript scanner.
//!
//! Provides ASCII fast-path checks for identifier characters, whitespace,
//! and line terminators.

use unicode_ident::{is_xid_continue, is_xid_start};

/// Returns `true` if `ch` can start an identifier (`a-zA-Z_$`).
///
/// Deliberately register-only (no lookup table). These run inside the scanner's
/// serial byte loop, where `pos += 1` makes each iteration depend on the last; a
/// table turns that into a dependent load chain (byte -> table[byte]) whose
/// latency costs more than the handful of ALU compares it removes. Measured: a
/// 256-entry LUT cut 16% of scanner instructions but *raised* cycles 4.5%.
#[inline(always)]
pub fn is_identifier_start(ch: u8) -> bool {
    ch.is_ascii_alphabetic() || ch == b'_' || ch == b'$'
}

/// Returns `true` if `ch` can continue an identifier (`a-zA-Z0-9_$`).
#[inline(always)]
pub fn is_identifier_continue(ch: u8) -> bool {
    ch.is_ascii_alphanumeric() || ch == b'_' || ch == b'$'
}

// ---------------------------------------------------------------------------
// SWAR (SIMD-within-a-register) identifier scanning
// ---------------------------------------------------------------------------
//
// The identifier loop is the hottest loop in the scanner. Byte-at-a-time it is a
// serial dependency chain: each `pos += 1` gates the next load. These helpers
// classify 8 bytes at once with plain u64 arithmetic (safe, portable, no table),
// so a typical identifier is consumed in one or two steps instead of 6-10.

const ONES: u64 = 0x0101_0101_0101_0101;
const HIGH: u64 = 0x8080_8080_8080_8080;

/// High bit set in each byte whose value is in `lo..=hi`.
/// Requires every byte of `x` to be ASCII (< 0x80) so no carry crosses a byte.
#[inline(always)]
const fn in_range(x: u64, lo: u8, hi: u8) -> u64 {
    let ge = x.wrapping_add((0x80 - lo as u64).wrapping_mul(ONES));
    let gt = x.wrapping_add((0x7F - hi as u64).wrapping_mul(ONES));
    ge & !gt & HIGH
}

/// High bit set in each ASCII byte equal to the ASCII value `n`.
/// Requires every byte of `x` to be ASCII, as in [`in_range`].
#[inline(always)]
const fn eq_ascii_byte(x: u64, n: u8) -> u64 {
    let y = x ^ (n as u64).wrapping_mul(ONES);
    // Each lane is <= 127, so adding 127 cannot carry into its neighbor.
    // Only zero stays below 128. The usual subtract-one zero-byte test can
    // borrow across lanes and incorrectly mark the byte after a match: '$%'
    // and '_^' would then swallow the operator into the identifier.
    !y.wrapping_add(0x7F * ONES) & HIGH
}

/// Given 8 source bytes (little-endian), return the index of the first byte that
/// cannot continue an identifier, or `None` if all 8 can.
///
/// Returns `Some(0)` for any chunk containing a non-ASCII byte, so the caller
/// falls back to the byte path, which handles Unicode identifiers correctly.
#[inline(always)]
pub fn first_non_identifier_continue(chunk: [u8; 8]) -> Option<u32> {
    let x = u64::from_le_bytes(chunk);
    if x & HIGH != 0 {
        // Non-ASCII present: let the scalar path decode UTF-8.
        return Some(0);
    }
    // `| 0x20` folds A-Z into a-z. Digits, `_` and `$` are unaffected by the fold
    // in a way that could alias into a-z, so they stay correctly classified.
    let ident = in_range(x | (0x20 * ONES), b'a', b'z')
        | in_range(x, b'0', b'9')
        | eq_ascii_byte(x, b'_')
        | eq_ascii_byte(x, b'$');
    let breaks = !ident & HIGH;
    if breaks == 0 {
        None
    } else {
        Some(breaks.trailing_zeros() >> 3)
    }
}

/// x86-64 wide form of [`first_non_identifier_continue`].
///
/// # Safety
/// `ptr` must point to at least 16 readable bytes.
#[cfg(target_arch = "x86_64")]
#[inline(always)]
pub unsafe fn first_non_identifier_continue_16(ptr: *const u8) -> Option<u32> {
    use std::arch::x86_64::*;

    // SAFETY: guaranteed by the caller.
    let bytes = unsafe { _mm_loadu_si128(ptr.cast()) };
    let folded = _mm_or_si128(bytes, _mm_set1_epi8(0x20));
    // Rotate each inclusive byte range to start at signed -128. One signed
    // comparison then checks both bounds; bytes outside the range, including
    // non-ASCII bytes, rotate above its upper bound. SSE2 byte addition wraps.
    let letters = _mm_cmpgt_epi8(
        _mm_set1_epi8(-128 + 26),
        _mm_add_epi8(folded, _mm_set1_epi8((128 - b'a') as i8)),
    );
    let digits = _mm_cmpgt_epi8(
        _mm_set1_epi8(-128 + 10),
        _mm_add_epi8(bytes, _mm_set1_epi8((128 - b'0') as i8)),
    );
    let punctuation = _mm_or_si128(
        _mm_cmpeq_epi8(bytes, _mm_set1_epi8(b'_' as i8)),
        _mm_cmpeq_epi8(bytes, _mm_set1_epi8(b'$' as i8)),
    );
    let valid = _mm_movemask_epi8(_mm_or_si128(_mm_or_si128(letters, digits), punctuation)) as u32;
    let breaks = !valid & 0xffff;
    if breaks == 0 {
        None
    } else {
        Some(breaks.trailing_zeros())
    }
}

/// Returns `true` if `ch` is a line terminator.
///
/// Covers `\n`, `\r`, and (at the byte level) the start of Unicode
/// line/paragraph separators (U+2028, U+2029) which are handled
/// separately during UTF-8 scanning.
#[inline]
pub fn is_line_break(ch: u8) -> bool {
    ch == b'\n' || ch == b'\r'
}

/// Byte-oriented line-terminator check including UTF-8 U+2028/U+2029.
#[inline]
pub fn is_line_break_at(source: &[u8], position: usize) -> bool {
    is_line_break(source[position])
        || source[position] == 0xe2
            && source.get(position + 1) == Some(&0x80)
            && matches!(source.get(position + 2), Some(0xa8 | 0xa9))
}

/// Returns `true` if `ch` is whitespace (but not a line terminator).
///
/// Covers space, tab, vertical tab, form feed, and BOM (0xFE/0xFF as
/// first bytes of UTF-16 BOM -- though in UTF-8 sources the BOM is
/// EF BB BF, which is handled separately).
#[inline]
pub fn is_white_space(ch: u8) -> bool {
    matches!(ch, b' ' | b'\t' | 0x0B | 0x0C)
}

/// Returns `true` if `ch` is whitespace or a line terminator.
#[inline]
pub fn is_white_space_or_line_break(ch: u8) -> bool {
    is_white_space(ch) || is_line_break(ch)
}

/// Check if a multi-byte character (decoded code point) is a Unicode
/// identifier start character. This is the slow path for non-ASCII.
pub fn is_unicode_identifier_start(ch: char) -> bool {
    is_xid_start(ch) || ch == '_' || ch == '$'
}

/// Check if a multi-byte character (decoded code point) is a Unicode
/// identifier continue character.
pub fn is_unicode_identifier_continue(ch: char) -> bool {
    is_xid_continue(ch) || ch == '_' || ch == '$' || ch == '\u{200C}' || ch == '\u{200D}'
}
