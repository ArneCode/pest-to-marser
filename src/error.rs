use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConvertError {
    DuplicateRule { name: String },
    UnknownEntryRule { name: String },
    UndefinedRule { name: String },
    UnsupportedFeature { feature: String, detail: String },
    UnknownBuiltin { name: String },
    NonProgressingRepetition { rule: String, detail: String },
    NonFailingRepetition { rule: String, detail: String },
    NonProgressingWhitespace { rule: String },
    NonFailingWhitespace { rule: String },
    LeftRecursion { chain: String },
    SccTooLarge { size: usize },
    UnreachableRule { name: String },
    TrailingInput { remaining: usize },
    CodegenFormatError { detail: String },
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
            Self::TrailingInput { remaining } => {
                write!(f, "trailing input: {remaining} byte(s) remain unparsed")
            }
            Self::CodegenFormatError { detail } => {
                write!(f, "failed to format generated code: {detail}")
            }
        }
    }
}

impl std::error::Error for ConvertError {}

pub type ConvertResult<T> = Result<T, Vec<ConvertError>>;

pub fn ok_or_errors<T>(errors: Vec<ConvertError>, value: T) -> ConvertResult<T> {
    if errors.is_empty() {
        Ok(value)
    } else {
        Err(errors)
    }
}
