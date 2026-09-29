use super::*;

fn trim_block_comment_inner_line_trailing_spaces(text: &str) -> String {
    if !text.contains('\n') && !text.contains('\r') {
        return text.to_string();
    }
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b' ' || bytes[i] == b'\t' {
            let mut j = i;
            while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
                j += 1;
            }
            if j < bytes.len() && (bytes[j] == b'\n' || bytes[j] == b'\r') {
                let mut line_start = i;
                while line_start > 0
                    && bytes[line_start - 1] != b'\n'
                    && bytes[line_start - 1] != b'\r'
                {
                    line_start -= 1;
                }
                let line_before_ws = &text[line_start..i];
                if line_before_ws.trim_end().ends_with("*/") {
                    while i < j {
                        i = crate::push_utf8_aware(&mut out, text, i);
                    }
                } else {
                    i = j;
                }
                continue;
            }
        }
        i = crate::push_utf8_aware(&mut out, text, i);
    }
    out
}

impl<'a> Emitter<'a> {
    /// Get trailing comment text after a span (on the same line).
    pub(super) fn get_trailing_comment(&self, span: Span) -> Option<String> {
        if self.options.remove_comments == Some(true) {
            return None;
        }
        let end = span.end as usize;
        if end >= self.source.len() {
            return None;
        }
        let rest = &self.source[end..];
        let eol = rest.find('\n').unwrap_or(rest.len());
        let tail = &rest[..eol];
        if let Some(comment_pos) = find_trailing_comment_start(tail) {
            // If there is a semicolon or closing brace between span.end and the
            // comment, the comment belongs to a subsequent statement or outer scope.
            let before_comment = &tail[..comment_pos];
            if before_comment.contains(';')
                || before_comment.contains('{')
                || before_comment.contains('}')
            {
                return None;
            }
            // Preserve trailing whitespace in comments - TypeScript keeps it
            let comment = tail[comment_pos..].trim_end_matches('\r');
            // For multi-line block comments (`/* ... \n ... */`), include
            // all lines up to and including the closing `*/`.
            if comment.starts_with("/*") && !comment.contains("*/") {
                let comment_abs_start = end + comment_pos;
                let remaining = &self.source[comment_abs_start..];
                if let Some(close_pos) = remaining.find("*/") {
                    let full = &remaining[..close_pos + 2];
                    return Some(trim_block_comment_inner_line_trailing_spaces(full));
                }
                return None;
            }
            // Single-line block comment (`/* ... */`) that closes on this line.
            // Collect ALL consecutive trailing comments on the line — multiple
            // block comments (`/* a */ /* b */`) or a block comment followed by
            // a line comment (`/* a */ // b`) — separated only by whitespace.
            // Stop at the first non-comment token: anything after the last `*/`
            // that isn't another comment (e.g. the enclosing block's closing
            // `}`) is NOT part of the trailing comment — capturing it would
            // duplicate that token into the output (a stray `}` → unbalanced
            // braces → invalid JS).
            if comment.starts_with("/*") {
                let bytes = comment.as_bytes();
                let mut i = 0;
                let mut last_end = 0;
                loop {
                    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
                        i += 1;
                    }
                    if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
                        match comment[i..].find("*/") {
                            Some(rel) => {
                                i += rel + 2;
                                last_end = i;
                            }
                            None => break,
                        }
                    } else if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'/' {
                        // Line comment runs to end of line (end of `comment`).
                        last_end = comment.len();
                        break;
                    } else {
                        break;
                    }
                }
                if last_end > 0 {
                    return Some(comment[..last_end].to_string());
                }
            }
            if !comment.is_empty() {
                return Some(comment.to_string());
            }
        }
        None
    }

    /// Emit file-prologue ("detached") comments.
    /// These are comments at the very beginning of the source file, before
    /// any declaration, that are separated from the first statement by at
    /// least one blank line.  TypeScript preserves these even when the first
    /// declaration is erased (interface / type alias / declare).
    ///
    /// This should only be called ONCE, for the first statement in the file.
    pub(super) fn emit_file_prologue_comments(&mut self, first_stmt_start: u32) {
        let remove_comments = self.options.remove_comments == Some(true);
        // Only consider comments that are at the very beginning of the source
        // (before any non-comment, non-whitespace content).
        while self.next_comment_idx < self.comments.len() {
            let c = &self.comments[self.next_comment_idx];
            if c.pos >= first_stmt_start {
                break;
            }
            if c.pos < self.comment_emit_pos {
                self.next_comment_idx += 1;
                continue;
            }
            let start = c.pos as usize;
            let prefix = &self.source[..start];
            let is_at_file_start = {
                let mut residual = prefix.to_string();
                for earlier in &self.comments[..self.next_comment_idx] {
                    let es = earlier.pos as usize;
                    let ee = (earlier.end as usize).min(start);
                    if es < ee && ee <= residual.len() {
                        residual.replace_range(es..ee, &" ".repeat(ee - es));
                    }
                }
                residual.trim().is_empty()
            };

            if !is_at_file_start {
                break;
            }

            let c_end = c.end as usize;
            let s_start = first_stmt_start as usize;
            let mut emitted = false;
            if c_end < s_start && s_start <= self.source.len() {
                let between = &self.source[c_end..s_start];
                let newline_segments: Vec<&str> = between.split('\n').collect();
                let has_blank_line = newline_segments.len() > 2
                    && newline_segments[1..newline_segments.len() - 1]
                        .iter()
                        .any(|line| line.trim_matches('\r').trim().is_empty());
                if has_blank_line {
                    let end = c.end as usize;
                    let text = &self.source[start..end.min(self.source.len())];
                    let text = text.trim_end_matches('\r');
                    if remove_comments && !text.starts_with("/*!") {
                        self.next_comment_idx += 1;
                        continue;
                    }
                    // In CJS mode, emit triple-slash reference directives as
                    // prologue comments so they appear BEFORE the __esModule
                    // preamble (matching TypeScript's output ordering).
                    let is_ref_directive =
                        text.starts_with("///<reference") || text.starts_with("/// <reference");
                    if is_ref_directive && self.is_commonjs() {
                        self.writeln(text);
                        self.next_comment_idx += 1;
                        // After emitting a reference directive, check if there
                        // is a blank-line gap to the next comment.  If so, stop
                        // processing prologue comments — TypeScript only treats
                        // the first contiguous group as detached.
                        if self.next_comment_idx < self.comments.len() {
                            let nc = &self.comments[self.next_comment_idx];
                            let nc_pos = nc.pos as usize;
                            let c_end_ref = c.end as usize;
                            if c_end_ref < nc_pos && nc_pos <= self.source.len() {
                                let gap = &self.source[c_end_ref..nc_pos];
                                let gap_segs: Vec<&str> = gap.split('\n').collect();
                                let gap_has_blank = gap_segs.len() > 2
                                    && gap_segs[1..gap_segs.len() - 1]
                                        .iter()
                                        .any(|l| l.trim_matches('\r').trim().is_empty());
                                if gap_has_blank {
                                    break;
                                }
                            }
                        }
                        continue;
                    }
                    if text.contains('\n') {
                        let line_start = if start == 0 {
                            0
                        } else {
                            self.source[..start].rfind('\n').map(|p| p + 1).unwrap_or(0)
                        };
                        let source_col = start - line_start;
                        self.emit_multiline_comment_with_col(text, source_col);
                        self.newline();
                    } else {
                        self.writeln(text);
                    }
                    emitted = true;
                }
            }
            if emitted {
                self.next_comment_idx += 1;
                if self.next_comment_idx < self.comments.len() {
                    let nc = &self.comments[self.next_comment_idx];
                    let nc_pos = nc.pos as usize;
                    if c_end < nc_pos && nc_pos <= self.source.len() {
                        let gap = &self.source[c_end..nc_pos];
                        let gap_segs: Vec<&str> = gap.split('\n').collect();
                        let gap_has_blank = gap_segs.len() > 2
                            && gap_segs[1..gap_segs.len() - 1]
                                .iter()
                                .any(|l| l.trim_matches('\r').trim().is_empty());
                        if gap_has_blank {
                            break;
                        }
                    }
                }
            } else {
                break;
            }
        }
    }

    /// Emit leading `/// <reference path="..."/>` directives from the file
    /// preamble.
    pub(super) fn emit_file_reference_path_directives(&mut self, first_stmt_start: u32) {
        if self.options.remove_comments == Some(true) {
            return;
        }
        while self.next_comment_idx < self.comments.len() {
            let c = &self.comments[self.next_comment_idx];
            if c.pos >= first_stmt_start {
                break;
            }
            if c.pos < self.comment_emit_pos {
                self.next_comment_idx += 1;
                continue;
            }
            let start = c.pos as usize;
            let end = (c.end as usize).min(self.source.len());
            let text = self.source[start..end].trim_end_matches('\r');
            let is_path_ref =
                text.starts_with("///<reference path") || text.starts_with("/// <reference path");
            let is_preserve_types_ref = (text.starts_with("///<reference types")
                || text.starts_with("/// <reference types"))
                && text.contains("preserve=\"true\"");
            if !is_path_ref && !is_preserve_types_ref {
                break;
            }
            // Strip .d.ts reference directives from JS output in ALL module
            // formats. TypeScript never preserves .d.ts references in JS emit.
            {
                let ref_path = text
                    .split('"')
                    .nth(1)
                    .or_else(|| text.split('\'').nth(1))
                    .unwrap_or("");
                if ref_path.ends_with(".d.ts") {
                    self.comment_emit_pos = self.comment_emit_pos.max(c.end);
                    self.next_comment_idx += 1;
                    continue;
                }
            }
            self.writeln(text);
            self.comment_emit_pos = self.comment_emit_pos.max(c.end);
            self.next_comment_idx += 1;
        }
    }

    /// Like `emit_file_reference_path_directives`, but only emits directives
    /// whose referenced file base name doesn't match any import source.
    pub(super) fn emit_filtered_reference_directives(
        &mut self,
        first_stmt_start: u32,
        import_sources: &[String],
    ) {
        if self.options.remove_comments == Some(true) {
            return;
        }
        while self.next_comment_idx < self.comments.len() {
            let c = &self.comments[self.next_comment_idx];
            if c.pos >= first_stmt_start {
                break;
            }
            if c.pos < self.comment_emit_pos {
                self.next_comment_idx += 1;
                continue;
            }
            let start = c.pos as usize;
            let end = (c.end as usize).min(self.source.len());
            let text = self.source[start..end].trim_end_matches('\r');
            let is_path_ref =
                text.starts_with("///<reference path") || text.starts_with("/// <reference path");
            let is_preserve_types_ref = (text.starts_with("///<reference types")
                || text.starts_with("/// <reference types"))
                && text.contains("preserve=\"true\"");
            if !is_path_ref && !is_preserve_types_ref {
                break;
            }
            let ref_path = text
                .split('"')
                .nth(1)
                .or_else(|| text.split('\'').nth(1))
                .unwrap_or("");
            let base = ref_path.trim_end_matches(".d.ts").trim_end_matches(".ts");
            let covered_by_import = import_sources.iter().any(|src| {
                let src_base = src
                    .trim_start_matches("./")
                    .trim_end_matches(".d.ts")
                    .trim_end_matches(".ts");
                src_base == base
            });
            if covered_by_import {
                self.comment_emit_pos = self.comment_emit_pos.max(c.end);
                self.next_comment_idx += 1;
                continue;
            }
            self.writeln(text);
            self.comment_emit_pos = self.comment_emit_pos.max(c.end);
            self.next_comment_idx += 1;
        }
    }

    /// Emit "detached" comments that appear between `=>` and the arrow body
    /// expression. TypeScript preserves `///` doc comments in this position.
    /// This is called from structured arrow emit when type annotations were stripped.
    pub(super) fn emit_detached_arrow_body_comments(&mut self, body_start: u32) {
        if self.options.remove_comments == Some(true) && !self.preserve_comments {
            return;
        }
        let mut emitted_any = false;
        while self.next_comment_idx < self.comments.len() {
            let c = &self.comments[self.next_comment_idx];
            if c.pos >= body_start {
                break;
            }
            if c.pos < self.comment_emit_pos {
                self.next_comment_idx += 1;
                continue;
            }
            let start = c.pos as usize;
            let end = c.end as usize;
            let text = &self.source[start..end.min(self.source.len())];
            let text = text.trim_end_matches('\r');
            // Emit newline before the comment (TypeScript puts detached
            // comments on their own line after `=> `)
            self.writeln("");
            self.write(text);
            self.comment_emit_pos = c.end;
            self.next_comment_idx += 1;
            emitted_any = true;
        }
        if emitted_any {
            self.writeln("");
        }
    }

    pub(super) fn emit_leading_comments(&mut self, before_pos: u32) {
        if self.options.remove_comments == Some(true) && !self.preserve_comments {
            return;
        }
        while self.next_comment_idx < self.comments.len() {
            let c = &self.comments[self.next_comment_idx];
            if c.pos >= before_pos {
                break;
            }
            if c.pos < self.comment_emit_pos {
                self.next_comment_idx += 1;
                continue;
            }
            // Skip JSX pragma comments marked for stripping
            if self.jsx_pragma_strip_positions.contains(&c.pos) {
                self.comment_emit_pos = self.comment_emit_pos.max(c.end);
                self.next_comment_idx += 1;
                continue;
            }
            let start = c.pos as usize;
            let line_start = self.source[..start].rfind('\n').map(|p| p + 1).unwrap_or(0);
            let line_prefix = &self.source[line_start..start];
            // A comment immediately following a scanner-skipped backslash is
            // trivia for the invalid token, not for the next recovered node.
            if c.is_multiline && line_prefix.trim_end().ends_with('\\') {
                self.comment_emit_pos = self.comment_emit_pos.max(c.end);
                self.next_comment_idx += 1;
                continue;
            }
            let is_line_start = if start == 0 {
                true
            } else {
                self.source[line_start..start].trim().is_empty()
            };
            if is_line_start {
                let end = c.end as usize;
                let text = &self.source[start..end.min(self.source.len())];
                let text = text.trim_end_matches('\r');
                // Triple-slash reference directives are stripped from AMD/UMD
                // JS output.  CJS (and non-module) output preserves them as
                // regular leading comments.
                if (self.is_amd() || self.is_umd())
                    && (text.starts_with("///<reference path")
                        || text.starts_with("/// <reference path"))
                {
                    self.next_comment_idx += 1;
                    continue;
                }
                let before = (before_pos as usize).min(self.source.len());
                let same_line_with_stmt =
                    c.is_multiline && before_pos != u32::MAX && end <= before && {
                        let between = &self.source[end..before];
                        !between.contains('\n')
                            && !between.contains('\r')
                            && between.trim().is_empty()
                    };

                if same_line_with_stmt {
                    self.write(text);
                    self.write(" ");
                } else if !self.at_line_start {
                    // The comment starts on its own line in the source, but
                    // the output cursor is mid-line (e.g., after `export default `).
                    // Emit a newline first so the comment appears on its own line.
                    self.newline();
                    self.write(text);
                    self.emit_same_line_following_comments(c.end, before_pos);
                    self.newline();
                } else if text.contains('\n') {
                    let line_start = if start == 0 {
                        0
                    } else {
                        self.source[..start].rfind('\n').map(|p| p + 1).unwrap_or(0)
                    };
                    let source_col = start - line_start;
                    let needs_trailing_space =
                        text.contains('\n') && text.trim_end_matches('\r').ends_with("*/") && {
                            let end_pos = c.end as usize;
                            end_pos >= self.source.len()
                                || self.source[end_pos..].trim().is_empty()
                                    && !self.source[end_pos..].contains('\n')
                        };
                    self.emit_multiline_comment_with_col(text, source_col);
                    if needs_trailing_space {
                        self.write(" ");
                    }
                    self.emit_same_line_following_comments(c.end, before_pos);
                    self.newline();
                } else {
                    self.write(text);
                    // TypeScript adds a trailing space after JSDoc comments
                    // (/** ... */) when they are at the very end of the file
                    // with no trailing content (not even a newline).
                    if text.starts_with("/**")
                        && before_pos == u32::MAX
                        && c.end as usize >= self.source.len()
                    {
                        self.write(" ");
                    }
                    self.emit_same_line_following_comments(c.end, before_pos);
                    self.newline();
                }
            } else if !is_line_start && c.is_multiline {
                let end = c.end as usize;
                let before = (before_pos as usize).min(self.source.len());
                if before_pos == u32::MAX
                    && end >= self.source.len()
                    && !self.source[start..end.min(self.source.len())].contains("*/")
                {
                    // Unterminated block comment at very end of file:
                    // append on previous output line (e.g. `a.public; /*`)
                    let text = &self.source[start..end.min(self.source.len())];
                    let text = text.trim_end_matches('\r');
                    if self.output.ends_with('\n') {
                        // Only if not already present on last output line
                        let last_line_start = self.output[..self.output.len() - 1]
                            .rfind('\n')
                            .map(|p| p + 1)
                            .unwrap_or(0);
                        let last_line = &self.output[last_line_start..self.output.len() - 1];
                        if !last_line.contains(text) {
                            self.output.pop();
                            self.output.push(' ');
                            self.output.push_str(text);
                            self.output.push('\n');
                        }
                    }
                } else if before_pos != u32::MAX && end <= before {
                    let between = &self.source[end..before];
                    if !between.contains('\n') && !between.contains('\r') {
                        let text = &self.source[start..end.min(self.source.len())];
                        let text = text.trim_end_matches('\r');
                        self.write(text);
                        self.write(" ");
                    }
                }
            }
            self.next_comment_idx += 1;
        }
    }

    pub(super) fn emit_same_line_following_comments(&mut self, cur_end: u32, before_pos: u32) {
        let mut cur_end = cur_end as usize;
        loop {
            let next_ci = self.next_comment_idx + 1;
            if next_ci >= self.comments.len() {
                break;
            }
            let nc = &self.comments[next_ci];
            if nc.pos >= before_pos {
                break;
            }
            let nc_start = nc.pos as usize;
            if nc_start > self.source.len() || nc_start < cur_end {
                break;
            }
            let gap = &self.source[cur_end..nc_start];
            if gap.contains('\n') || gap.contains('\r') {
                break;
            }
            let nc_end = (nc.end as usize).min(self.source.len());
            let nc_text = &self.source[nc_start..nc_end];
            let nc_text = nc_text.trim_end_matches('\r');
            let is_line_comment = nc_text.starts_with("//");
            if !nc_text.starts_with("/*") && !is_line_comment {
                break;
            }
            let nc_text = nc_text.to_string();
            self.write(" ");
            if !is_line_comment && nc_text.contains('\n') {
                // Multi-line block comment on same line as preceding comment:
                // re-indent continuation lines to match the current indent level.
                let target_col = (self.indent as usize) * 4;
                let text_lines: Vec<&str> = nc_text.split('\n').collect();
                self.write(text_lines[0].trim_end());
                for line in &text_lines[1..] {
                    self.newline();
                    let trimmed = line.trim_start();
                    if trimmed.is_empty() {
                        continue;
                    }
                    for _ in 0..target_col {
                        self.output.push(' ');
                    }
                    self.out_col = target_col as u32;
                    self.at_line_start = false;
                    self.output.push_str(trimmed);
                    self.out_col += trimmed.len() as u32;
                }
            } else {
                self.write(&nc_text);
            }
            cur_end = nc_end;
            self.comment_emit_pos = self.comment_emit_pos.max(nc.end);
            self.next_comment_idx += 1;
            if is_line_comment {
                break; // Line comments end at end of line
            }
        }
    }

    #[allow(dead_code)]
    pub(super) fn emit_leading_block_comments(&mut self, before_pos: u32) {
        if self.options.remove_comments == Some(true) {
            return;
        }
        while self.next_comment_idx < self.comments.len() {
            let c = &self.comments[self.next_comment_idx];
            if c.pos >= before_pos {
                break;
            }
            if c.pos < self.comment_emit_pos {
                self.next_comment_idx += 1;
                continue;
            }
            let start = c.pos as usize;
            let end = c.end as usize;
            let text = &self.source[start..end.min(self.source.len())];
            let text = text.trim_end_matches('\r');

            let is_block_comment = text.starts_with("/*");
            let is_amd_dep =
                text.starts_with("///<amd-dependency") || text.starts_with("/// <amd-dependency");

            if is_block_comment || is_amd_dep {
                let is_line_start = if start == 0 {
                    true
                } else {
                    let prefix = &self.source[..start];
                    let line_start = prefix.rfind('\n').map(|p| p + 1).unwrap_or(0);
                    self.source[line_start..start].trim().is_empty()
                };
                if is_line_start {
                    if text.contains('\n') {
                        let line_start = if start == 0 {
                            0
                        } else {
                            self.source[..start].rfind('\n').map(|p| p + 1).unwrap_or(0)
                        };
                        let source_col = start - line_start;
                        self.emit_multiline_comment_with_col(text, source_col);
                        self.newline();
                    } else {
                        self.writeln(text);
                    }
                }
                self.next_comment_idx += 1;
            } else {
                break;
            }
        }
    }

    pub(super) fn emit_multiline_comment_with_col(&mut self, text: &str, source_col: usize) {
        let lines: Vec<&str> = text.split('\n').collect();
        if lines.is_empty() {
            return;
        }
        self.write(lines[0].trim_end());
        let output_first_col = if self.at_line_start {
            (self.indent as usize) * 4
        } else {
            (self.out_col as usize).saturating_sub(lines[0].trim_end().len())
        };

        if lines.len() == 1 {
            return;
        }

        for line in &lines[1..] {
            self.newline();
            let raw_line = line.trim_end_matches('\r');
            let trimmed = raw_line.trim_end();
            if trimmed.trim().is_empty() {
                if !raw_line.is_empty() && raw_line.trim().is_empty() {
                    self.at_line_start = false;
                    for _ in 0..output_first_col {
                        self.output.push(' ');
                    }
                    self.out_col = output_first_col as u32;
                }
                continue;
            }
            let mut line_cols = 0usize;
            let mut byte_offset = 0usize;
            for ch in trimmed.chars() {
                match ch {
                    ' ' => {
                        line_cols += 1;
                        byte_offset += 1;
                    }
                    '\t' => {
                        line_cols = ((line_cols / 4) + 1) * 4;
                        byte_offset += 1;
                    }
                    _ => break,
                }
            }
            let content = trimmed[byte_offset..].trim_end();
            let desired_col = if line_cols >= source_col {
                (line_cols - source_col) + output_first_col
            } else {
                output_first_col.saturating_sub(source_col - line_cols)
            };
            self.at_line_start = false;
            for _ in 0..desired_col {
                self.output.push(' ');
            }
            self.out_col = desired_col as u32;
            self.output.push_str(content);
            self.out_col += content.len() as u32;
        }
    }

    pub(super) fn has_comments_in_range(&self, start: u32, end: u32) -> bool {
        self.comments.iter().any(|c| c.pos >= start && c.pos < end)
    }

    /// Emit inline block comments (`/* ... */`) that fall within the given
    /// source range.  Used to preserve comments like `this.a /*undefined*/`
    /// when structurally emitting initializer expressions.
    pub(super) fn emit_inline_comments_in_range(&mut self, start: u32, end: u32) {
        if self.options.remove_comments == Some(true) && !self.preserve_comments {
            return;
        }
        while self.next_comment_idx < self.comments.len() {
            let c = &self.comments[self.next_comment_idx];
            if c.pos >= end {
                break;
            }
            if c.pos < start || c.pos < self.comment_emit_pos {
                self.next_comment_idx += 1;
                continue;
            }
            let cs = c.pos as usize;
            let ce = (c.end as usize).min(self.source.len());
            let text = self.source[cs..ce].trim();
            if text.starts_with("/*") && text.ends_with("*/") {
                self.write(" ");
                self.write(text);
                self.comment_emit_pos = self.comment_emit_pos.max(c.end);
            }
            self.next_comment_idx += 1;
        }
    }

    /// Re-emit block comments from a transformed header range even when the
    /// ordinary comment cursor has advanced past that source position. This is
    /// used when a for-of binding moves from the header into the loop body.
    pub(super) fn emit_moved_block_comments_in_range(&mut self, start: u32, end: u32) {
        if self.options.remove_comments == Some(true) || start >= end {
            return;
        }
        let comments: Vec<(u32, String)> = self
            .comments
            .iter()
            .filter(|comment| comment.is_multiline && comment.pos >= start && comment.pos < end)
            .filter_map(|comment| {
                let comment_end = (comment.end as usize).min(self.source.len());
                (comment.pos < comment.end && (comment.pos as usize) < comment_end).then(|| {
                    (
                        comment.end,
                        self.source[comment.pos as usize..comment_end].to_string(),
                    )
                })
            })
            .collect();
        for (comment_end, comment) in comments {
            self.write(" ");
            self.write(&comment);
            self.comment_emit_pos = self.comment_emit_pos.max(comment_end);
        }
    }

    /// Move line comments out of a transformed for-of header before the body
    /// declaration. Keeping each comment on its own line prevents synthesized
    /// initializer tokens from being swallowed by the comment.
    pub(super) fn emit_moved_line_comments_in_range(&mut self, start: u32, end: u32) {
        if self.options.remove_comments == Some(true) || start >= end {
            return;
        }
        let comments: Vec<(u32, String)> = self
            .comments
            .iter()
            .filter(|comment| !comment.is_multiline && comment.pos >= start && comment.pos < end)
            .filter_map(|comment| {
                let comment_end = (comment.end as usize).min(self.source.len());
                (comment.pos < comment.end && (comment.pos as usize) < comment_end).then(|| {
                    (
                        comment.end,
                        self.source[comment.pos as usize..comment_end]
                            .trim_end()
                            .to_string(),
                    )
                })
            })
            .collect();
        for (comment_end, comment) in comments {
            self.writeln(&comment);
            self.comment_emit_pos = self.comment_emit_pos.max(comment_end);
        }
    }

    /// Re-emit leading comments immediately before an operand position.
    /// Used for expression restructuring (e.g. `**` to `Math.pow()`) where
    /// TypeScript duplicates leading comments inside the transformed call.
    /// Finds comments whose end position is <= `before_pos` and whose text
    /// sits in the trivia gap immediately before the operand.
    pub(super) fn emit_exp_operand_comments(&mut self, before_pos: u32) {
        if self.options.remove_comments == Some(true) && !self.preserve_comments {
            return;
        }
        // Find comments that end at or just before `before_pos` (with only
        // whitespace between the comment end and the operand start).
        for c in self.comments.iter() {
            if c.end > before_pos {
                break;
            }
            // Check that between comment end and operand start there is only whitespace
            let gap_start = c.end as usize;
            let gap_end = before_pos as usize;
            if gap_start <= gap_end
                && gap_end <= self.source.len()
                && self.source[gap_start..gap_end].trim().is_empty()
            {
                let cs = c.pos as usize;
                let ce = (c.end as usize).min(self.source.len());
                let text = &self.source[cs..ce];
                if text.starts_with("//") {
                    self.write(text);
                    self.write("\n");
                } else if text.starts_with("/*") {
                    self.write(text);
                    self.write(" ");
                }
            }
        }
    }

    pub(super) fn advance_comment_pos(&mut self, past: u32) {
        if past > self.comment_emit_pos {
            self.comment_emit_pos = past;
        }
        while self.next_comment_idx < self.comments.len()
            && self.comments[self.next_comment_idx].pos < past
        {
            self.next_comment_idx += 1;
        }
    }
}
