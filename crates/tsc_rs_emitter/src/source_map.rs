//! Source map generation following the Source Map v3 specification.
//!
//! Provides VLQ base64 encoding and a `SourceMapGenerator` that collects
//! mappings and serialises them as JSON.

// ---------------------------------------------------------------------------
// VLQ Base64 encoding
// ---------------------------------------------------------------------------

const BASE64_CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encode a signed integer as a VLQ base64 string, appending to `out`.
pub fn vlq_encode(value: i64, out: &mut String) {
    // Convert to unsigned with sign in the least significant bit.
    let mut vlq = if value < 0 {
        ((-value) as u64) << 1 | 1
    } else {
        (value as u64) << 1
    };

    loop {
        let mut digit = (vlq & 0b11111) as u8; // 5-bit chunk
        vlq >>= 5;
        if vlq > 0 {
            digit |= 0b100000; // continuation bit
        }
        out.push(BASE64_CHARS[digit as usize] as char);
        if vlq == 0 {
            break;
        }
    }
}

/// Encode a signed integer as a VLQ base64 string, returning a new `String`.
pub fn vlq_encode_string(value: i64) -> String {
    let mut s = String::new();
    vlq_encode(value, &mut s);
    s
}

// ---------------------------------------------------------------------------
// Mapping
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Mapping {
    pub generated_line: u32,
    pub generated_column: u32,
    pub source_index: u32,
    pub original_line: u32,
    pub original_column: u32,
    pub name_index: Option<u32>,
}

// ---------------------------------------------------------------------------
// SourceMapGenerator
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SourceMapGenerator {
    file: String,
    source_root: String,
    sources: Vec<String>,
    names: Vec<String>,
    mappings: Vec<Mapping>,
}

impl SourceMapGenerator {
    pub fn new(file: &str, source_root: &str) -> Self {
        Self {
            file: file.to_string(),
            source_root: source_root.to_string(),
            sources: Vec::new(),
            names: Vec::new(),
            mappings: Vec::new(),
        }
    }

    /// Register a source file. Returns the index.
    pub fn add_source(&mut self, source: &str) -> u32 {
        if let Some(idx) = self.sources.iter().position(|s| s == source) {
            return idx as u32;
        }
        let idx = self.sources.len() as u32;
        self.sources.push(source.to_string());
        idx
    }

    /// Register a name. Returns the index.
    pub fn add_name(&mut self, name: &str) -> u32 {
        if let Some(idx) = self.names.iter().position(|n| n == name) {
            return idx as u32;
        }
        let idx = self.names.len() as u32;
        self.names.push(name.to_string());
        idx
    }

    /// Add a mapping.
    pub fn add_mapping(&mut self, mapping: Mapping) {
        self.mappings.push(mapping);
    }

    /// Add mappings after the recorded ones; at one generated position the
    /// recorded ones come first (coalescing keeps the later, forward one).
    pub fn merge_mappings(&mut self, mappings: Vec<Mapping>) {
        self.mappings.extend(mappings);
    }

    /// Return a checkpoint that can later be used to remove mappings produced
    /// by a speculative buffered emission. Registered names intentionally stay
    /// interned because relocated mappings retain their existing name indices.
    pub(crate) fn mapping_checkpoint(&self) -> usize {
        self.mappings.len()
    }

    /// Remove and return mappings recorded after `checkpoint`.
    pub(crate) fn take_mappings_since(&mut self, checkpoint: usize) -> Vec<Mapping> {
        if checkpoint >= self.mappings.len() {
            return Vec::new();
        }
        self.mappings.split_off(checkpoint)
    }

    /// Relocate mappings after text is inserted into the generated output.
    /// `generated_line`/`generated_column` identify the insertion point before
    /// the insertion. Columns are byte-based, matching the emitter's cursor.
    pub fn shift_after_insertion(
        &mut self,
        generated_line: u32,
        generated_column: u32,
        inserted: &str,
    ) {
        if inserted.is_empty() {
            return;
        }
        let inserted_bytes = inserted.as_bytes();
        let inserted_lines = inserted_bytes.iter().filter(|&&b| b == b'\n').count() as u32;
        let trailing_columns = inserted_bytes
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(inserted_bytes.len(), |last| inserted_bytes.len() - last - 1)
            as u32;

        for mapping in &mut self.mappings {
            if mapping.generated_line < generated_line
                || (mapping.generated_line == generated_line
                    && mapping.generated_column < generated_column)
            {
                continue;
            }
            if inserted_lines == 0 {
                if mapping.generated_line == generated_line {
                    mapping.generated_column += inserted_bytes.len() as u32;
                }
            } else if mapping.generated_line == generated_line {
                mapping.generated_line += inserted_lines;
                mapping.generated_column =
                    trailing_columns + mapping.generated_column.saturating_sub(generated_column);
            } else {
                mapping.generated_line += inserted_lines;
            }
        }
    }

    /// Relocate mappings after replacing a generated single-line token with a
    /// token of a different byte length. Generated placeholder identifiers are
    /// not themselves mapped, so only positions at or after the old token end
    /// need to move.
    pub fn shift_after_single_line_replacement(
        &mut self,
        generated_line: u32,
        generated_column: u32,
        old_len: usize,
        new_len: usize,
    ) {
        let old_end = generated_column.saturating_add(old_len as u32);
        let delta = new_len as i64 - old_len as i64;
        if delta == 0 {
            return;
        }
        for mapping in &mut self.mappings {
            if mapping.generated_line == generated_line && mapping.generated_column >= old_end {
                mapping.generated_column = (mapping.generated_column as i64 + delta).max(0) as u32;
            }
        }
    }

    /// Encode all mappings into the VLQ "mappings" field string.
    fn encode_mappings(&self) -> String {
        let mut result = String::new();
        let mut prev_gen_line: u32 = 0;
        let mut prev_gen_col: u32 = 0;
        let mut prev_source: u32 = 0;
        let mut prev_orig_line: u32 = 0;
        let mut prev_orig_col: u32 = 0;
        let mut prev_name: u32 = 0;

        // Sort mappings by generated line, then column (stable: recording
        // order decides between mappings at one generated position).
        let mut ordered: Vec<&Mapping> = self.mappings.iter().collect();
        ordered.sort_by_key(|m| (m.generated_line, m.generated_column));
        let sorted = tsc_coalesce_mappings(&ordered);

        let mut first_in_line = true;

        for m in &sorted {
            // Insert semicolons for skipped lines.
            while prev_gen_line < m.generated_line {
                result.push(';');
                prev_gen_line += 1;
                prev_gen_col = 0;
                first_in_line = true;
            }

            if !first_in_line {
                result.push(',');
            }
            first_in_line = false;

            // Field 1: generated column (relative)
            vlq_encode(m.generated_column as i64 - prev_gen_col as i64, &mut result);
            prev_gen_col = m.generated_column;

            // Field 2: source index (relative)
            vlq_encode(m.source_index as i64 - prev_source as i64, &mut result);
            prev_source = m.source_index;

            // Field 3: original line (relative)
            vlq_encode(m.original_line as i64 - prev_orig_line as i64, &mut result);
            prev_orig_line = m.original_line;

            // Field 4: original column (relative)
            vlq_encode(m.original_column as i64 - prev_orig_col as i64, &mut result);
            prev_orig_col = m.original_column;

            // Field 5: name index (relative, optional)
            if let Some(name_idx) = m.name_index {
                vlq_encode(name_idx as i64 - prev_name as i64, &mut result);
                prev_name = name_idx;
            }
        }

        result
    }

    /// Generate the JSON source map string (Source Map v3).
    ///
    /// Emits compact (minified) JSON – no extra whitespace – matching
    /// TypeScript's own sourcemap serialiser output.
    pub fn to_json(&self) -> String {
        let mut json = String::new();
        json.push_str("{\"version\":3,\"file\":\"");
        json.push_str(&escape_json(&self.file));
        json.push_str("\",\"sourceRoot\":\"");
        json.push_str(&escape_json(&self.source_root));
        json.push_str("\",\"sources\":[");
        for (i, src) in self.sources.iter().enumerate() {
            if i > 0 {
                json.push(',');
            }
            json.push('"');
            json.push_str(&escape_json(src));
            json.push('"');
        }
        json.push_str("],\"names\":[");
        for (i, name) in self.names.iter().enumerate() {
            if i > 0 {
                json.push(',');
            }
            json.push('"');
            json.push_str(&escape_json(name));
            json.push('"');
        }
        json.push_str("],\"mappings\":\"");
        json.push_str(&self.encode_mappings());
        json.push_str("\"}");
        json
    }

    /// Generate a base64-encoded data URI for an inline source map.
    pub fn to_data_uri(&self) -> String {
        let json = self.to_json();
        let encoded = base64_encode(json.as_bytes());
        format!("data:application/json;base64,{}", encoded)
    }
}

// ---------------------------------------------------------------------------
// Base64 encoding (for inline source maps)
// ---------------------------------------------------------------------------

fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity(data.len().div_ceil(3) * 4);
    let chunks = data.chunks(3);
    for chunk in chunks {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;

        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);

        if chunk.len() > 1 {
            result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }

        if chunk.len() > 2 {
            result.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

// ---------------------------------------------------------------------------
// JSON string escaping
// ---------------------------------------------------------------------------

fn escape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < '\x20' => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Helper: compute line/column from byte offset in source text
// ---------------------------------------------------------------------------

/// Pre-computed line start offsets for O(log n) offset-to-line-col lookups.
#[derive(Clone)]
pub struct LineIndex {
    /// Byte offsets of the start of each line. `line_starts[0]` is always 0.
    line_starts: Vec<u32>,
}

impl LineIndex {
    /// Build a line index for the given source text.
    pub fn new(source: &str) -> Self {
        let bytes = source.as_bytes();
        let mut line_starts = Vec::with_capacity(bytes.len() / 40 + 1); // heuristic
        line_starts.push(0);
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'\n' {
                line_starts.push((i + 1) as u32);
            }
        }
        LineIndex { line_starts }
    }

    /// Compute (0-based line, 0-based column) from a byte offset.
    #[inline]
    pub fn offset_to_line_col(&self, offset: u32) -> (u32, u32) {
        let line = match self.line_starts.binary_search(&offset) {
            Ok(exact) => exact as u32,
            Err(insert) => (insert - 1) as u32,
        };
        let col = offset - self.line_starts[line as usize];
        (line, col)
    }
}

/// Compute (0-based line, 0-based column) from a byte offset into source text.
/// This is the O(n) fallback; prefer `LineIndex` for repeated lookups.
pub fn offset_to_line_col(source: &str, offset: u32) -> (u32, u32) {
    let offset = offset as usize;
    let bytes = source.as_bytes();
    let mut line = 0u32;
    let mut col = 0u32;
    for &b in &bytes[..offset.min(bytes.len())] {
        if b == b'\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    (line, col)
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // VLQ encoding tests
    // -----------------------------------------------------------------------

    #[test]
    fn vlq_encode_zero() {
        assert_eq!(vlq_encode_string(0), "A");
    }

    #[test]
    fn vlq_encode_positive_small() {
        // 1 -> unsigned = 2 -> 5-bit = 2, no continuation -> 'C'
        assert_eq!(vlq_encode_string(1), "C");
    }

    #[test]
    fn vlq_encode_negative_one() {
        // -1 -> unsigned = 3 -> 5-bit = 3, no continuation -> 'D'
        assert_eq!(vlq_encode_string(-1), "D");
    }

    #[test]
    fn vlq_encode_positive_large() {
        // 16 -> unsigned = 32 -> needs continuation
        // first 5 bits = 0 with cont = 32 -> 'g' (index 32)
        // remaining = 1 -> 'B'
        assert_eq!(vlq_encode_string(16), "gB");
    }

    #[test]
    fn vlq_encode_negative_large() {
        // -16 -> unsigned = 33 -> needs continuation
        // first 5 bits = 1 with cont = 33 -> 'h' (index 33)
        // remaining = 1 -> 'B'
        assert_eq!(vlq_encode_string(-16), "hB");
    }

    #[test]
    fn vlq_encode_larger_value() {
        // 100 -> unsigned = 200
        // 200 in binary: 11001000
        // 5-bit groups: 01000 (8) with cont, 00110 (6)
        // first group: 8 | 32 = 40 -> 'o' (index 40)? Let's verify:
        // 200 & 0x1f = 8, continuation -> 8 | 32 = 40 -> index 40 = 'o'
        // 200 >> 5 = 6, no continuation -> 6 -> index 6 = 'G'
        assert_eq!(vlq_encode_string(100), "oG");
    }

    #[test]
    fn vlq_encode_negative_larger_value() {
        // -100 -> unsigned = 201
        // 201 & 0x1f = 9, continuation -> 9 | 32 = 41 -> index 41 = 'p'
        // 201 >> 5 = 6, no continuation -> 6 -> index 6 = 'G'
        assert_eq!(vlq_encode_string(-100), "pG");
    }

    #[test]
    fn vlq_encode_very_large() {
        // 500 -> unsigned = 1000
        // 1000 & 0x1f = 8, cont -> 40 -> 'o'
        // 1000 >> 5 = 31, cont -> 31 | 32 = 63 -> '/' (index 63)
        // Wait, 31 < 32, no continuation needed.
        // Actually, let's compute: 1000 >> 5 = 31, 31 & 0x1f = 31, no cont -> index 31 = 'f'
        // Hmm, but 31 fits in 5 bits so no further continuation.
        // Actually 1000 = 0b1111101000
        // Groups of 5 from LSB: 01000 (8), 11110 (30)... wait:
        // 1000 & 31 = 8, rest = 1000 >> 5 = 31
        // 31 & 31 = 31, rest = 31 >> 5 = 0
        // So: 8|32=40 -> 'o', 31 -> 'f'
        assert_eq!(vlq_encode_string(500), "of");
    }

    // -----------------------------------------------------------------------
    // offset_to_line_col tests
    // -----------------------------------------------------------------------

    #[test]
    fn line_col_start() {
        assert_eq!(offset_to_line_col("hello\nworld", 0), (0, 0));
    }

    #[test]
    fn line_col_mid_first_line() {
        assert_eq!(offset_to_line_col("hello\nworld", 3), (0, 3));
    }

    #[test]
    fn line_col_second_line() {
        assert_eq!(offset_to_line_col("hello\nworld", 6), (1, 0));
    }

    #[test]
    fn line_col_second_line_mid() {
        assert_eq!(offset_to_line_col("hello\nworld", 9), (1, 3));
    }

    // -----------------------------------------------------------------------
    // Base64 encoding tests
    // -----------------------------------------------------------------------

    #[test]
    fn base64_simple() {
        assert_eq!(base64_encode(b"hello"), "aGVsbG8=");
    }

    #[test]
    fn base64_empty() {
        assert_eq!(base64_encode(b""), "");
    }

    // -----------------------------------------------------------------------
    // SourceMapGenerator tests
    // -----------------------------------------------------------------------

    #[test]
    fn empty_source_map() {
        let gen = SourceMapGenerator::new("out.js", "");
        let json = gen.to_json();
        assert!(json.contains("\"version\":3"));
        assert!(json.contains("\"file\":\"out.js\""));
        assert!(json.contains("\"sources\":[]"));
        assert!(json.contains("\"names\":[]"));
        assert!(json.contains("\"mappings\":\"\""));
    }

    #[test]
    fn single_line_mapping() {
        let mut gen = SourceMapGenerator::new("out.js", "");
        gen.add_source("input.ts");
        gen.add_mapping(Mapping {
            generated_line: 0,
            generated_column: 0,
            source_index: 0,
            original_line: 0,
            original_column: 0,
            name_index: None,
        });
        let json = gen.to_json();
        assert!(json.contains("\"sources\":[\"input.ts\"]"));
        // Generated col 0, source 0, orig line 0, orig col 0 -> all zeros -> "AAAA"
        assert!(json.contains("\"mappings\":\"AAAA\""));
    }

    #[test]
    fn multi_line_mapping() {
        let mut gen = SourceMapGenerator::new("out.js", "");
        gen.add_source("input.ts");
        gen.add_mapping(Mapping {
            generated_line: 0,
            generated_column: 0,
            source_index: 0,
            original_line: 0,
            original_column: 0,
            name_index: None,
        });
        gen.add_mapping(Mapping {
            generated_line: 1,
            generated_column: 0,
            source_index: 0,
            original_line: 1,
            original_column: 0,
            name_index: None,
        });
        let json = gen.to_json();
        // Line 0: AAAA, Line 1: AACA (col 0 rel, source 0 rel, line +1, col 0 rel)
        assert!(json.contains("\"mappings\":\"AAAA;AACA\""));
    }

    #[test]
    fn mapping_with_names() {
        let mut gen = SourceMapGenerator::new("out.js", "");
        let src = gen.add_source("input.ts");
        let name = gen.add_name("myVar");
        gen.add_mapping(Mapping {
            generated_line: 0,
            generated_column: 4,
            source_index: src,
            original_line: 0,
            original_column: 4,
            name_index: Some(name),
        });
        let json = gen.to_json();
        assert!(json.contains("\"names\":[\"myVar\"]"));
        // gen_col 4, source 0, orig_line 0, orig_col 4, name 0 -> IAAIA
        assert!(json.contains("\"mappings\":\"IAAIA\""));
    }

    #[test]
    fn multiple_sources() {
        let mut gen = SourceMapGenerator::new("bundle.js", "");
        let s0 = gen.add_source("a.ts");
        let s1 = gen.add_source("b.ts");
        gen.add_mapping(Mapping {
            generated_line: 0,
            generated_column: 0,
            source_index: s0,
            original_line: 0,
            original_column: 0,
            name_index: None,
        });
        gen.add_mapping(Mapping {
            generated_line: 1,
            generated_column: 0,
            source_index: s1,
            original_line: 0,
            original_column: 0,
            name_index: None,
        });
        let json = gen.to_json();
        assert!(json.contains("\"sources\":[\"a.ts\",\"b.ts\"]"));
        // Line 0: gen_col=0, src=0, orig_line=0, orig_col=0 -> AAAA
        // Line 1: gen_col=0, src=+1(=C), orig_line=0(=A relative), orig_col=0(=A)
        assert!(json.contains("\"mappings\":\"AAAA;ACAA\""));
    }

    #[test]
    fn source_map_json_valid_structure() {
        let mut gen = SourceMapGenerator::new("test.js", "/src");
        gen.add_source("test.ts");
        gen.add_mapping(Mapping {
            generated_line: 0,
            generated_column: 0,
            source_index: 0,
            original_line: 0,
            original_column: 0,
            name_index: None,
        });
        let json = gen.to_json();
        assert!(json.starts_with('{'));
        assert!(json.ends_with('}'));
        assert!(json.contains("\"version\":3"));
        assert!(json.contains("\"file\":\"test.js\""));
        assert!(json.contains("\"sourceRoot\":\"/src\""));
    }

    #[test]
    fn inline_source_map_data_uri() {
        let mut gen = SourceMapGenerator::new("out.js", "");
        gen.add_source("in.ts");
        gen.add_mapping(Mapping {
            generated_line: 0,
            generated_column: 0,
            source_index: 0,
            original_line: 0,
            original_column: 0,
            name_index: None,
        });
        let data_uri = gen.to_data_uri();
        assert!(data_uri.starts_with("data:application/json;base64,"));
        // Verify the base64 part decodes to valid JSON-like content
        let b64_part = &data_uri["data:application/json;base64,".len()..];
        assert!(!b64_part.is_empty());
    }

    #[test]
    fn multiple_mappings_same_line() {
        let mut gen = SourceMapGenerator::new("out.js", "");
        gen.add_source("in.ts");
        gen.add_mapping(Mapping {
            generated_line: 0,
            generated_column: 0,
            source_index: 0,
            original_line: 0,
            original_column: 0,
            name_index: None,
        });
        gen.add_mapping(Mapping {
            generated_line: 0,
            generated_column: 10,
            source_index: 0,
            original_line: 0,
            original_column: 10,
            name_index: None,
        });
        let json = gen.to_json();
        let mappings_str = extract_mappings(&json);
        // Should have a comma separating two segments on line 0
        assert!(mappings_str.contains(','));
        assert!(!mappings_str.contains(';'));
    }

    #[test]
    fn type_stripping_shifts_positions() {
        // Simulates: `let x: number = 5;` becomes `let x = 5;`
        // The `=` moves from column 16 to column 6 in the output,
        // but the source map should map output col 4 (x) to original col 4
        let mut gen = SourceMapGenerator::new("out.js", "");
        gen.add_source("input.ts");
        // Map 'let' keyword
        gen.add_mapping(Mapping {
            generated_line: 0,
            generated_column: 0,
            source_index: 0,
            original_line: 0,
            original_column: 0,
            name_index: None,
        });
        // Map 'x' variable name
        gen.add_mapping(Mapping {
            generated_line: 0,
            generated_column: 4,
            source_index: 0,
            original_line: 0,
            original_column: 4,
            name_index: None,
        });
        // Map '=' sign (shifted left due to type removal)
        gen.add_mapping(Mapping {
            generated_line: 0,
            generated_column: 6,
            source_index: 0,
            original_line: 0,
            original_column: 16,
            name_index: None,
        });
        // Map '5' literal
        gen.add_mapping(Mapping {
            generated_line: 0,
            generated_column: 8,
            source_index: 0,
            original_line: 0,
            original_column: 18,
            name_index: None,
        });
        let json = gen.to_json();
        assert!(json.contains("\"version\":3"));
        // Verify mappings field exists and has segments separated by commas
        let mappings_str = extract_mappings(&json);
        let segments: Vec<&str> = mappings_str.split(',').collect();
        assert_eq!(segments.len(), 4);
    }

    /// Helper to extract the mappings string from the JSON.
    fn extract_mappings(json: &str) -> String {
        let key = "\"mappings\":\"";
        let start = json.find(key).unwrap() + key.len();
        let end = json[start..].find('"').unwrap() + start;
        json[start..end].to_string()
    }
}

/// tsc's SourceMapGenerator.addMapping: a mapping at the pending generated
/// position overwrites the pending source position unless it moves the
/// source backwards, which commits the pending one; a mapping identical to
/// the last committed one is dropped.
fn tsc_coalesce_mappings<'a>(ordered: &[&'a Mapping]) -> Vec<&'a Mapping> {
    let mut out: Vec<&'a Mapping> = Vec::with_capacity(ordered.len());
    let mut pending: Option<&'a Mapping> = None;
    let same_target = |a: &Mapping, b: &Mapping| {
        a.generated_line == b.generated_line
            && a.generated_column == b.generated_column
            && a.source_index == b.source_index
            && a.original_line == b.original_line
            && a.original_column == b.original_column
            && a.name_index == b.name_index
    };
    let mut commit = |pending: Option<&'a Mapping>, out: &mut Vec<&'a Mapping>| {
        if let Some(mapping) = pending {
            if !out.last().is_some_and(|last| same_target(last, mapping)) {
                out.push(mapping);
            }
        }
    };
    for &mapping in ordered {
        let new_position = pending.is_none_or(|p| {
            p.generated_line != mapping.generated_line
                || p.generated_column != mapping.generated_column
        });
        let backtracking = pending.is_some_and(|p| {
            p.source_index == mapping.source_index
                && (p.original_line > mapping.original_line
                    || (p.original_line == mapping.original_line
                        && p.original_column > mapping.original_column))
        });
        if new_position || backtracking {
            commit(pending, &mut out);
        }
        pending = Some(mapping);
    }
    commit(pending, &mut out);
    out
}
