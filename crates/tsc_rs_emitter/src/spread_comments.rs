//! Reject spans without a possible comment adjacent to a spread marker.
//! This indexes text, including strings/comments, just like the existing checks.
use tsc_rs_ast::Span;

#[derive(Clone)]
pub(super) struct SpreadCommentIndex {
    // Last three dots before a comment, and the end of its two-byte prefix.
    // Both coordinates increase monotonically with source order.
    candidates: Vec<(usize, usize)>,
}

impl SpreadCommentIndex {
    pub(super) fn new(source: &str) -> Self {
        let bytes = source.as_bytes();
        let mut candidates = Vec::new();
        for (slash, _) in source.match_indices('/') {
            if !matches!(bytes.get(slash + 1), Some(b'/' | b'*')) {
                continue;
            }
            let mut before = slash;
            while before > 0 && matches!(bytes[before - 1], b' ' | b'\t') {
                before -= 1;
            }
            if before >= 3 && &bytes[before - 3..before] == b"..." {
                candidates.push((before - 3, slash + 2));
            }
        }
        Self { candidates }
    }

    pub(super) fn may_contain(&self, span: Span) -> bool {
        let first = self
            .candidates
            .partition_point(|&(dots, _)| dots < span.start as usize);
        self.candidates
            .get(first)
            .is_some_and(|&(_, prefix_end)| prefix_end <= span.end as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Emitter;
    use std::collections::HashMap;
    use tsc_rs_ast::CompilerOptions;

    // Keep the slow span-local text search as an independent reference. The
    // index is only a negative filter: long dot runs can admit false positives
    // depending on the span's starting position and non-overlapping matches.
    fn reference(text: &str, linebreak: bool) -> bool {
        text.match_indices("...").any(|(dots, _)| {
            let rest = text[dots + 3..].trim_start_matches([' ', '\t']);
            if !linebreak {
                return rest.starts_with("/*") || rest.starts_with("//");
            }
            if rest.starts_with("/*") {
                rest.find("*/").is_some_and(|end| {
                    rest[end + 2..]
                        .chars()
                        .take_while(|c| c.is_whitespace())
                        .any(|c| c == '\n' || c == '\r')
                })
            } else {
                rest.starts_with("//") && rest.contains(['\n', '\r'])
            }
        })
    }

    fn check_all_spans(source: &str) {
        let options = CompilerOptions::default();
        let emitter = Emitter::new(source, &options, &[], HashMap::new());
        let boundaries: Vec<_> = source
            .char_indices()
            .map(|(i, _)| i)
            .chain([source.len()])
            .collect();
        for &start in &boundaries {
            for &end in boundaries.iter().filter(|&&end| end >= start) {
                let span = Span::new(start as u32, end as u32);
                let text = &source[start..end];
                assert_eq!(
                    emitter.span_has_adjacent_spread_comment(span),
                    reference(text, false),
                    "{source:?} {span:?}"
                );
                assert_eq!(
                    emitter.span_has_adjacent_spread_comment_linebreak(span),
                    reference(text, true),
                    "{source:?} {span:?}"
                );
            }
        }
        assert!(!emitter.span_has_adjacent_spread_comment(Span::new(0, source.len() as u32 + 1)));
    }

    #[test]
    fn spread_comment_index_preserves_truncated_and_overlapping_spans() {
        for dots in 1..=9 {
            for gap in ["", " ", "\t ", "\n", "\u{2000}"] {
                for comment in [
                    "/*x*/\n",
                    "//x\r\n",
                    "/*x",
                    "/*x*/\u{2000}\r",
                    "/x",
                    "/*...*/x\n",
                ] {
                    check_all_spans(&format!("λ[{}{gap}{comment}].../*z*/\n", ".".repeat(dots)));
                }
            }
        }
    }

    #[test]
    fn spread_comment_index_preserves_textual_matches_in_strings_and_comments() {
        for source in [
            "const text = '... /*fake*/';",
            "// example ... // another comment\n",
            "`... /*x*/\r\n`",
            ".... /*x*/\n.....//y\n....../*z*/\r",
            "x / y; let spread = [...values];",
        ] {
            check_all_spans(source);
        }
        let options = CompilerOptions {
            fast_emit: Some(true),
            ..Default::default()
        };
        let emitter = Emitter::new("... /*x*/\n", &options, &[], HashMap::new());
        assert!(!emitter.span_has_adjacent_spread_comment(Span::new(0, 10)));
        assert!(!emitter.span_has_adjacent_spread_comment_linebreak(Span::new(0, 10)));
        assert!(emitter.adjacent_spread_comments.get().is_none());
    }
}
