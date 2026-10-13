//! Template segment matching adapted from pingdotgg/ts-rust
//! `checker/relater_p3.rs`, inferFromLiteralPartsToTemplateLiteral.
//! See NOTICE.txt for the pinned revision and upstream attribution.

use super::*;

impl TypeChecker {
    /// Match fixed target delimiters across source text segments, retaining
    /// the source placeholders between them. A target placeholder may cover
    /// several source segments, as in `1.${T}.3` -> `${string}.${string}.${string}`.
    pub(super) fn template_parts_assignable(
        &self,
        source_texts: &[String],
        source_types: &[Type],
        target_texts: &[String],
        target_types: &[Type],
    ) -> bool {
        if source_texts.len() != source_types.len() + 1
            || target_texts.len() != target_types.len() + 1
            || target_types.is_empty()
        {
            return false;
        }
        let last = source_texts.len() - 1;
        let start = &target_texts[0];
        let end = target_texts.last().unwrap();
        if !source_texts[0].starts_with(start)
            || !source_texts[last].ends_with(end)
            || (last == 0 && source_texts[0].len() < start.len() + end.len())
        {
            return false;
        }
        let text_at = |index: usize| {
            let text = source_texts[index].as_str();
            if index == last {
                &text[..text.len() - end.len()]
            } else {
                text
            }
        };
        let (mut segment, mut position) = (0, start.len());
        for (index, target) in target_types.iter().enumerate() {
            let delimiter = &target_texts[index + 1];
            let (mut next_segment, mut next_position) = (segment, position);
            if index + 1 == target_types.len() {
                next_segment = last;
                next_position = text_at(last).len();
            } else if !delimiter.is_empty() {
                loop {
                    if let Some(offset) = text_at(next_segment)[next_position..].find(delimiter) {
                        next_position += offset;
                        break;
                    }
                    next_segment += 1;
                    if next_segment > last {
                        return false;
                    }
                    next_position = 0;
                }
            } else if let Some(character) = text_at(segment)[position..].chars().next() {
                next_position += character.len_utf8();
            } else if segment < last {
                next_segment += 1;
                next_position = 0;
            } else {
                return false;
            }
            let captured = if segment == next_segment {
                Type::StringLiteral(source_texts[segment][position..next_position].to_string())
            } else {
                let mut texts = vec![source_texts[segment][position..].to_string()];
                texts.extend_from_slice(&source_texts[segment + 1..next_segment]);
                texts.push(source_texts[next_segment][..next_position].to_string());
                Self::evaluate_template_literal_type(
                    texts,
                    source_types[segment..next_segment].to_vec(),
                )
            };
            if !self.template_placeholder_assignable(&captured, target) {
                return false;
            }
            segment = next_segment;
            position = next_position + delimiter.len();
        }
        true
    }

    fn template_placeholder_assignable(&self, source: &Type, target: &Type) -> bool {
        if matches!(target, Type::String) || self.is_assignable_to(source, target) {
            return true;
        }
        if let Type::Union(members) = target {
            return members
                .iter()
                .any(|member| self.template_placeholder_assignable(source, member));
        }
        match source {
            Type::TemplateLiteral { quasis, types }
                if types.len() == 1 && quasis.iter().all(String::is_empty) =>
            {
                self.is_assignable_to(&types[0], target)
            }
            Type::StringLiteral(text)
                if matches!(
                    target,
                    Type::Number
                        | Type::Boolean
                        | Type::NumberLiteral(_)
                        | Type::BooleanLiteral(_)
                        | Type::StringLiteral(_)
                ) =>
            {
                Self::matches_template_literal(
                    text,
                    &[String::new(), String::new()],
                    std::slice::from_ref(target),
                )
            }
            _ => false,
        }
    }
}
