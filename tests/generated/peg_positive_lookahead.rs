use marser::capture;
use marser::matcher::{
    positive_lookahead,
};
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
    // main <- "a" &"b" "b"
    let main = capture!(
        bind_slice!(('a', positive_lookahead('b'), 'b'), value as &'src str) => Parsed::main { value }
    );

    main.clone()
}
