/// Input grammar syntax supported by the converter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum InputSyntax {
    #[default]
    Pest,
    Peg,
}

impl InputSyntax {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "pest" => Some(Self::Pest),
            "peg" => Some(Self::Peg),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pest => "pest",
            Self::Peg => "peg",
        }
    }
}
