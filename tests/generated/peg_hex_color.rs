use marser::capture;
use marser::one_of::one_of;
use marser::parser::{
    Parser,
};

// Typed parse tree returned by `grammar()`. Each grammar rule becomes a variant;
// labeled bindings become struct fields, and leaf rules store their matched slice
// as `value`.
#[derive(Debug, Clone, PartialEq)]
pub enum Parsed<'src> {
    main { value: &'src str },
}

pub fn grammar<'src>() -> impl Parser<'src, &'src str, Output = Parsed<'src>> + Clone {
    // main <- "#" [0-9a-fA-F] [0-9a-fA-F] [0-9a-fA-F] [0-9a-fA-F] [0-9a-fA-F] [0-9a-fA-F]
    let main = capture!(
        bind_slice!(
            (
                '#',
                one_of(('0'..='9', 'a'..='f', 'A'..='F')),
                one_of(('0'..='9', 'a'..='f', 'A'..='F')),
                one_of(('0'..='9', 'a'..='f', 'A'..='F')),
                one_of(('0'..='9', 'a'..='f', 'A'..='F')),
                one_of(('0'..='9', 'a'..='f', 'A'..='F')),
                one_of(('0'..='9', 'a'..='f', 'A'..='F')),
            ),
            value as &'src str
        ) => Parsed::main { value }
    );

    main.clone()
}
