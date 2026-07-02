use std::fmt;

use marser::error::{FurthestFailError, InlineError, ParserError};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConvertError {
    DuplicateRule {
        name: String,
    },
    UnknownEntryRule {
        name: String,
    },
    UndefinedRule {
        name: String,
    },
    UnsupportedFeature {
        feature: String,
        detail: String,
    },
    UnknownBuiltin {
        name: String,
    },
    NonProgressingRepetition {
        rule: String,
        detail: String,
    },
    NonFailingRepetition {
        rule: String,
        detail: String,
    },
    NonProgressingWhitespace {
        rule: String,
    },
    NonFailingWhitespace {
        rule: String,
    },
    LeftRecursion {
        chain: String,
    },
    SccTooLarge {
        size: usize,
    },
    UnreachableRule {
        name: String,
    },
    ParseError {
        message: String,
        span: Option<(usize, usize)>,
    },
    InvalidRule {
        name: String,
        text: String,
    },
    CodegenFormatError {
        detail: String,
    },
}

impl ConvertError {
    pub fn span(&self) -> Option<(usize, usize)> {
        match self {
            Self::ParseError { span, .. } => *span,
            _ => None,
        }
    }
}

/// 1-based line and column for a byte offset in `source`.
pub fn offset_to_line_col(source: &str, offset: usize) -> (usize, usize) {
    let mut line = 1usize;
    let mut col = 1usize;
    for (i, ch) in source.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

pub fn format_span_location(source: &str, span: (usize, usize)) -> String {
    let (start_line, start_col) = offset_to_line_col(source, span.0);
    if span.0 == span.1 {
        return format!("line {start_line}, column {start_col}");
    }
    let (end_line, end_col) = offset_to_line_col(source, span.1);
    if start_line == end_line {
        format!("line {start_line}, columns {start_col}-{end_col}")
    } else {
        format!("line {start_line}, column {start_col} through line {end_line}, column {end_col}")
    }
}

pub fn format_furthest_fail(source: &str, err: &FurthestFailError) -> String {
    let expected_msg = match err.expected.len() {
        0 => "unexpected token".to_string(),
        1 => format!("expected {}", err.expected[0]),
        _ => {
            let mut sorted = err.expected.clone();
            sorted.sort();
            format!("expected one of {}", sorted.join(", "))
        }
    };
    let location = format_span_location(source, err.span);
    let mut out = format!("{expected_msg} at {location}");
    for ann in &err.annotations {
        let loc = format_span_location(source, ann.span);
        out.push_str(&format!("\n  {loc}: {}", ann.message));
    }
    for note in &err.notes {
        out.push_str(&format!("\nnote: {note}"));
    }
    for help in &err.helps {
        out.push_str(&format!("\nhelp: {help}"));
    }
    out
}

pub fn format_inline_error(source: &str, err: &InlineError) -> String {
    let mut out = err.message.clone();
    if let Some(span) = err.span {
        out.push_str(&format!(" at {}", format_span_location(source, span)));
    }
    for ann in &err.annotations {
        let loc = format_span_location(source, ann.span);
        out.push_str(&format!("\n  {loc}: {}", ann.message));
    }
    for note in &err.notes {
        out.push_str(&format!("\nnote: {note}"));
    }
    for help in &err.helps {
        out.push_str(&format!("\nhelp: {help}"));
    }
    out
}

pub fn parse_error_from_furthest_fail(source: &str, err: FurthestFailError) -> ConvertError {
    let span = err.span;
    ConvertError::ParseError {
        message: format_furthest_fail(source, &err),
        span: Some(span),
    }
}

pub fn parse_error_from_parser_error(source: &str, err: &ParserError) -> ConvertError {
    match err {
        ParserError::FurthestFail(e) => ConvertError::ParseError {
            message: format_furthest_fail(source, e),
            span: Some(e.span),
        },
        ParserError::Inline(e) => ConvertError::ParseError {
            message: format_inline_error(source, e),
            span: Some(e.reporting_span()),
        },
    }
}

impl fmt::Display for ConvertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateRule { name } => write!(f, "rule {name} is already defined"),
            Self::UnknownEntryRule { name } => write!(f, "entry rule {name} is not defined"),
            Self::UndefinedRule { name } => write!(f, "rule {name} is undefined"),
            Self::UnsupportedFeature { feature, detail } => {
                write!(f, "unsupported {feature}: {detail}")
            }
            Self::UnknownBuiltin { name } => write!(f, "unknown builtin {name}"),
            Self::NonProgressingRepetition { rule, detail } => {
                write!(f, "rule {rule}: {detail}")
            }
            Self::NonFailingRepetition { rule, detail } => {
                write!(f, "rule {rule}: {detail}")
            }
            Self::NonProgressingWhitespace { rule } => {
                write!(f, "{rule} is non-progressing and will repeat infinitely")
            }
            Self::NonFailingWhitespace { rule } => {
                write!(f, "{rule} cannot fail and will repeat infinitely")
            }
            Self::LeftRecursion { chain } => write!(f, "left-recursive cycle: {chain}"),
            Self::SccTooLarge { size } => {
                write!(
                    f,
                    "mutual recursion group of size {size} exceeds v1 limit of 12"
                )
            }
            Self::UnreachableRule { name } => write!(f, "rule {name} is unreachable from entry"),
            Self::ParseError { message, .. } => write!(f, "{message}"),
            Self::InvalidRule { name, text } => {
                write!(f, "rule {name} could not be parsed: {text}")
            }
            Self::CodegenFormatError { detail } => {
                write!(f, "failed to format generated code: {detail}")
            }
        }
    }
}

impl std::error::Error for ConvertError {}

pub type ConvertResult<T> = Result<T, Vec<ConvertError>>;

#[cfg(test)]
mod tests {
    use super::*;
    use marser::error::FurthestFailError;

    #[test]
    fn format_span_location_single_point() {
        let src = "a\nbc\ndef";
        assert_eq!(format_span_location(src, (3, 3)), "line 2, column 2");
    }

    #[test]
    fn format_furthest_fail_uses_line_numbers() {
        let src = "rule = { \"x\" ";
        let err = FurthestFailError {
            span: (src.len(), src.len()),
            expected: vec!["'}'".into()],
            annotations: vec![],
            notes: vec![],
            helps: vec![],
        };
        let msg = format_furthest_fail(src, &err);
        assert!(msg.contains("line 1,"));
        assert!(!msg.contains(".."));
    }
}
