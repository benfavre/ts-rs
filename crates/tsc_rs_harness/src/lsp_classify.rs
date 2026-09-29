//! Classify fourslash test failures into semantic LSP buckets.

use std::fmt;

/// Failure classification bucket for LSP test results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LspBucket {
    /// Test caused a panic in parser/binder/checker.
    Panic,
    /// Baseline file not found — test cannot be compared.
    NoBaseline,
    /// Verify command not yet implemented in the harness.
    Unsupported,
    /// Parser could not parse the fourslash test file.
    ParseError,
    /// Operation returned None where we expected a result.
    NoResult,
    /// Type portion of the display text is wrong.
    WrongType,
    /// Symbol kind prefix is wrong (e.g. "const" vs "let").
    WrongKind,
    /// Display text differs only in minor formatting.
    DisplayDiff,
    /// Failure involves cross-file resolution.
    CrossFile,
    /// An expected completion entry was not found.
    MissingCompletion,
    /// Unexpected entries appeared (only for exact match).
    ExtraCompletion,
    /// GoToDefinition: returned no definition when one was expected.
    NoDefinition,
    /// GoToDefinition: pointed to the wrong file.
    WrongFile,
    /// GoToDefinition: pointed to wrong position within the correct file.
    WrongLocation,
}

impl fmt::Display for LspBucket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Panic => write!(f, "PANIC"),
            Self::NoBaseline => write!(f, "NO-BASELINE"),
            Self::Unsupported => write!(f, "UNSUPPORTED"),
            Self::ParseError => write!(f, "PARSE-ERROR"),
            Self::NoResult => write!(f, "NO-RESULT"),
            Self::WrongType => write!(f, "WRONG-TYPE"),
            Self::WrongKind => write!(f, "WRONG-KIND"),
            Self::DisplayDiff => write!(f, "DISPLAY-DIFF"),
            Self::CrossFile => write!(f, "CROSS-FILE"),
            Self::MissingCompletion => write!(f, "MISSING-COMPLETION"),
            Self::ExtraCompletion => write!(f, "EXTRA-COMPLETION"),
            Self::NoDefinition => write!(f, "NO-DEFINITION"),
            Self::WrongFile => write!(f, "WRONG-FILE"),
            Self::WrongLocation => write!(f, "WRONG-LOCATION"),
        }
    }
}

impl LspBucket {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "PANIC" => Some(Self::Panic),
            "NO-BASELINE" => Some(Self::NoBaseline),
            "UNSUPPORTED" => Some(Self::Unsupported),
            "PARSE-ERROR" => Some(Self::ParseError),
            "NO-RESULT" => Some(Self::NoResult),
            "WRONG-TYPE" => Some(Self::WrongType),
            "WRONG-KIND" => Some(Self::WrongKind),
            "DISPLAY-DIFF" => Some(Self::DisplayDiff),
            "CROSS-FILE" => Some(Self::CrossFile),
            "MISSING-COMPLETION" => Some(Self::MissingCompletion),
            "EXTRA-COMPLETION" => Some(Self::ExtraCompletion),
            "NO-DEFINITION" => Some(Self::NoDefinition),
            "WRONG-FILE" => Some(Self::WrongFile),
            "WRONG-LOCATION" => Some(Self::WrongLocation),
            _ => None,
        }
    }
}

/// Classify a quickInfo marker failure.
///
/// `expected_display` is the baseline displayParts text.
/// `actual` is what our hover returned (None if no result).
/// `expected_kind` is the baseline kind (e.g. "class", "var").
/// `is_cross_file` indicates the test has multiple files.
pub fn classify_quick_info(
    expected_display: &str,
    actual: Option<&str>,
    _expected_kind: &str,
    is_cross_file: bool,
) -> LspBucket {
    let Some(actual) = actual else {
        if is_cross_file {
            return LspBucket::CrossFile;
        }
        return LspBucket::NoResult;
    };

    let actual_norm = normalize(actual);
    let expected_norm = normalize(expected_display);

    if actual_norm == expected_norm {
        // Shouldn't happen — caller should only classify failures
        return LspBucket::DisplayDiff;
    }

    // Check for kind prefix mismatch
    let expected_prefix = extract_kind_prefix(&expected_norm);
    let actual_prefix = extract_kind_prefix(&actual_norm);
    if expected_prefix != actual_prefix && !expected_prefix.is_empty() && !actual_prefix.is_empty()
    {
        return LspBucket::WrongKind;
    }

    // If it's a cross-file test and we got wrong result, likely cross-file issue
    if is_cross_file {
        return LspBucket::CrossFile;
    }

    // Default: the type is wrong
    LspBucket::WrongType
}

/// Classify a goToDefinition marker failure.
///
/// `expected_file` is the baseline target file (None if no definition expected).
/// `actual` is what our definition_at returned (None if no result).
/// `is_cross_file` indicates the test has multiple files.
pub fn classify_goto_definition(
    expected_file: Option<&str>,
    actual: Option<(&str, u32)>,
    is_cross_file: bool,
) -> LspBucket {
    match (expected_file, actual) {
        (None, None) => LspBucket::DisplayDiff, // both agree: no definition
        (Some(_), None) => {
            if is_cross_file {
                return LspBucket::CrossFile;
            }
            LspBucket::NoDefinition
        }
        (None, Some(_)) => LspBucket::NoDefinition, // returned def when none expected
        (Some(expected), Some((actual_file, _))) => {
            // Compare by filename (last path component) since baseline uses
            // full fourslash paths while executor uses short paths.
            let exp_name = expected.rsplit('/').next().unwrap_or(expected);
            let act_name = actual_file.rsplit('/').next().unwrap_or(actual_file);
            if exp_name != act_name {
                if is_cross_file {
                    return LspBucket::CrossFile;
                }
                return LspBucket::WrongFile;
            }
            LspBucket::WrongLocation
        }
    }
}

fn normalize(s: &str) -> String {
    let s = s.trim();
    let s = s.strip_prefix("```typescript\n").unwrap_or(s);
    let s = s.strip_prefix("```ts\n").unwrap_or(s);
    let s = s.strip_suffix("\n```").unwrap_or(s);
    let s = if let Some(pos) = s.find("\n\n---\n\n") {
        &s[..pos]
    } else {
        s
    };
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn extract_kind_prefix(display: &str) -> &str {
    // Extract the first word: "class", "const", "var", "function", etc.
    // Also handles "(method)", "(property)", "(parameter)"
    if display.starts_with('(') {
        if let Some(end) = display.find(')') {
            return &display[..end + 1];
        }
    }
    display.split_whitespace().next().unwrap_or("")
}
