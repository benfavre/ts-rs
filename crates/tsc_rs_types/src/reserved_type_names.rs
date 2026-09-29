use tsc_rs_ast::{Diagnostic, DiagnosticCategory, Span};
use tsc_rs_scanner::{TokenKind, TsScanner};

use crate::TypeChecker;
use std::borrow::Cow;

fn is_reserved_type_name(name: &str) -> bool {
    matches!(
        name,
        "any"
            | "unknown"
            | "never"
            | "number"
            | "bigint"
            | "boolean"
            | "string"
            | "symbol"
            | "void"
            | "object"
            | "undefined"
    )
}

fn reserved_type_name(name: &str) -> Option<Cow<'_, str>> {
    if is_reserved_type_name(name) {
        return Some(Cow::Borrowed(name));
    }
    if name.contains('\\') {
        let mut scanner = TsScanner::new(name);
        scanner.scan();
        if is_reserved_type_name(scanner.token_value()) {
            return Some(Cow::Owned(scanner.token_value().to_owned()));
        }
    }
    None
}

impl TypeChecker {
    pub(crate) fn check_reserved_type_name(
        &mut self,
        name: &str,
        span: Option<Span>,
        code: u32,
        declaration: &str,
    ) {
        let Some(name) = reserved_type_name(name) else {
            return;
        };
        let Some(span) = span else { return };
        // A declaration can be visited by both contextual and ordinary checking.
        if self
            .diagnostics
            .iter()
            .any(|d| d.code == code && d.span == Some(span))
        {
            return;
        }
        self.diagnostics.push(Diagnostic {
            code,
            message: format!("{declaration} name cannot be '{name}'."),
            category: DiagnosticCategory::Error,
            file_name: None,
            span: Some(span),
            related: None,
        });
    }

    pub(crate) fn check_reserved_type_parameter_name(&mut self, name: &str, span: Span) {
        let Some(name) = reserved_type_name(name) else {
            return;
        };
        // TypeParam spans also contain modifiers/constraints/defaults. Scan only
        // the invalid-name path to underline the name, including Unicode escapes,
        // rather than its modifiers or a later occurrence in the constraint.
        let Some(source) = self
            .current_source
            .as_deref()
            .and_then(|source| source.get(span.start as usize..span.end as usize))
        else {
            return;
        };
        let mut scanner = TsScanner::new(source);
        while scanner.scan() != TokenKind::EndOfFile {
            if scanner.token_value() == name.as_ref() {
                let name_span = Span::new(
                    span.start + scanner.token_pos() as u32,
                    span.start + scanner.text_pos() as u32,
                );
                self.check_reserved_type_name(&name, Some(name_span), 2368, "Type parameter");
                break;
            }
        }
    }
}
