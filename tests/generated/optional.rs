use marser::capture;
use marser::matcher::{
    Matcher,
    many,
    one_or_more,
    optional,
    start_of_input,
    end_of_input,
};
use marser::one_of::one_of;
use marser::parser::{
    Parser,
    ParserCombinator,
};

#[derive(Debug, Clone, PartialEq)]
pub enum Parsed<'src> {
    WHITESPACE { value: &'src str },
    main {
        sign: Option<Box<Parsed<'src>>>,
        digits: Box<Parsed<'src>>,
    },
    sign { value: &'src str },
    digits { value: &'src str },
}

pub fn grammar<'src>() -> impl Parser<'src, &'src str, Output = Parsed<'src>> + Clone {
    let ASCII_DIGIT = '0'..='9';

    // WHITESPACE = _{ " " }
    let WHITESPACE = capture!(
bind_slice!(
            ' ',
        value as &'src str
    ) => Parsed::WHITESPACE { value }
    );

    let ws = many(
        WHITESPACE.clone().ignore_result()
    );

    // sign = { "+" | "-" }
    let sign = capture!(
bind_slice!(
            one_of(('+', '-')),
        value as &'src str
    ) => Parsed::sign { value }
    );

    // digits = @{ ASCII_DIGIT+ }
    let digits = capture!(
bind_slice!(
            one_or_more(ASCII_DIGIT.clone()),
        value as &'src str
    ) => Parsed::digits { value }
    );

    // main = { SOI ~ #sign = sign? ~ #digits = digits ~ EOI }
    let main = capture!(
        (
            start_of_input(),
            ws.clone(),
            optional(bind!(sign.clone(), ?sign_val)),
            ws.clone(),
            bind!(digits.clone(), digits_val),
            ws.clone(),
            end_of_input(),
        ) => Parsed::main { sign: sign_val.map(Box::new), digits: Box::new(digits_val) }
    );

    main.clone()
}
