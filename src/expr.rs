use crate::ast::{PostfixOp, PrefixOp};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MatchingContext {
    NormalWs,
    AtomicNoWs,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Builtin {
    Soi,
    Eoi,
    Any,
    Newline,
    AsciiDigit,
    AsciiNonzeroDigit,
    AsciiBinDigit,
    AsciiOctDigit,
    AsciiHexDigit,
    AsciiAlphaLower,
    AsciiAlphaUpper,
    AsciiAlpha,
    AsciiAlphanumeric,
}

impl Builtin {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "SOI" => Some(Self::Soi),
            "EOI" => Some(Self::Eoi),
            "ANY" => Some(Self::Any),
            "NEWLINE" => Some(Self::Newline),
            "ASCII_DIGIT" => Some(Self::AsciiDigit),
            "ASCII_NONZERO_DIGIT" => Some(Self::AsciiNonzeroDigit),
            "ASCII_BIN_DIGIT" => Some(Self::AsciiBinDigit),
            "ASCII_OCT_DIGIT" => Some(Self::AsciiOctDigit),
            "ASCII_HEX_DIGIT" => Some(Self::AsciiHexDigit),
            "ASCII_ALPHA_LOWER" => Some(Self::AsciiAlphaLower),
            "ASCII_ALPHA_UPPER" => Some(Self::AsciiAlphaUpper),
            "ASCII_ALPHA" => Some(Self::AsciiAlpha),
            "ASCII_ALPHANUMERIC" => Some(Self::AsciiAlphanumeric),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Soi => "SOI",
            Self::Eoi => "EOI",
            Self::Any => "ANY",
            Self::Newline => "NEWLINE",
            Self::AsciiDigit => "ASCII_DIGIT",
            Self::AsciiNonzeroDigit => "ASCII_NONZERO_DIGIT",
            Self::AsciiBinDigit => "ASCII_BIN_DIGIT",
            Self::AsciiOctDigit => "ASCII_OCT_DIGIT",
            Self::AsciiHexDigit => "ASCII_HEX_DIGIT",
            Self::AsciiAlphaLower => "ASCII_ALPHA_LOWER",
            Self::AsciiAlphaUpper => "ASCII_ALPHA_UPPER",
            Self::AsciiAlpha => "ASCII_ALPHA",
            Self::AsciiAlphanumeric => "ASCII_ALPHANUMERIC",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Empty,
    Builtin(Builtin),
    RuleRef(String),
    Literal(String),
    InsensitiveLiteral(String),
    Range { start: char, end: char },
    Sequence(Vec<Expr>),
    Choice(Vec<Expr>),
    Prefix { op: PrefixOp, expr: Box<Expr> },
    Postfix { expr: Box<Expr>, op: PostfixOp },
}

impl Expr {
    pub fn rule_refs(&self) -> Vec<&str> {
        let mut refs = Vec::new();
        self.collect_rule_refs(&mut refs);
        refs
    }

    fn collect_rule_refs<'a>(&'a self, refs: &mut Vec<&'a str>) {
        match self {
            Self::RuleRef(name) => refs.push(name),
            Self::Sequence(items) | Self::Choice(items) => {
                for item in items {
                    item.collect_rule_refs(refs);
                }
            }
            Self::Prefix { expr, .. } | Self::Postfix { expr, .. } => {
                expr.collect_rule_refs(refs);
            }
            Self::Empty
            | Self::Builtin(_)
            | Self::Literal(_)
            | Self::InsensitiveLiteral(_)
            | Self::Range { .. } => {}
        }
    }

    pub fn has_unsupported(&self) -> Option<&'static str> {
        match self {
            Self::Empty => None,
            Self::Builtin(_) => None,
            Self::RuleRef(name) => {
                if matches!(
                    name.as_str(),
                    "PUSH" | "POP" | "POP_ALL" | "DROP" | "PEEK" | "PEEK_ALL"
                ) {
                    Some("stack construct")
                } else {
                    None
                }
            }
            Self::Literal(_) | Self::InsensitiveLiteral(_) | Self::Range { .. } => None,
            Self::Sequence(items) | Self::Choice(items) => {
                items.iter().find_map(|item| item.has_unsupported())
            }
            Self::Prefix { expr, .. } | Self::Postfix { expr, .. } => expr.has_unsupported(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SymKey {
    pub rule: String,
    pub context: MatchingContext,
}
