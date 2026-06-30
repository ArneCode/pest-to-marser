use marser::capture;
use marser::matcher::{
    one_or_more,
    optional,
};
use marser::one_of::one_of;
use marser::parser::{
    Parser,
};

// Typed parse tree returned by `grammar()`. Each grammar rule becomes a variant;
// labeled bindings become struct fields, and leaf rules store their matched slice
// as `value`.
#[derive(Debug, Clone, PartialEq)]
pub enum Parsed<'src> {
    main {
        sign: Option<Box<Parsed<'src>>>,
        digits: Box<Parsed<'src>>,
    },
    sign { value: &'src str },
    digits { value: &'src str },
}

pub fn grammar<'src>() -> impl Parser<'src, &'src str, Output = Parsed<'src>> + Clone {
    // sign <- "+" / "-"
    let sign = capture!(
        bind_slice!(one_of(('+', '-')), value as &'src str) => Parsed::sign { value }
    );

    // digits <- [0-9]+
    let digits = capture!(
        bind_slice!(one_or_more('0'..='9'), value as &'src str) => Parsed::digits { value }
    );

    // main <- #sign = sign? #digits = digits
    let main = capture!(
        (
            optional(bind!(sign.clone(), ?sign_val)),
            bind!(digits.clone(), digits_val),
        ) => Parsed::main {
            sign: sign_val.map(Box::new),
            digits: Box::new(digits_val),
        }
    );

    main.clone()
}
