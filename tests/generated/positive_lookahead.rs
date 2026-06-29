use marser::capture;
use marser::matcher::{
    Matcher,
    many,
    positive_lookahead,
    start_of_input,
    end_of_input,
};
use marser::parser::{
    Parser,
    ParserCombinator,
};

#[derive(Debug, Clone, PartialEq)]
pub enum Parsed<'src> {
    WHITESPACE { value: &'src str },
    main {
        matched: &'src str,
    },
}

pub fn grammar<'src>() -> impl Parser<'src, &'src str, Output = Parsed<'src>> + Clone {
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

    // main = { SOI ~ &"ab" ~ #matched = "ab" ~ EOI }
    let main = capture!(
        (
            start_of_input(),
            ws.clone(),
            positive_lookahead("ab"),
            ws.clone(),
            bind_slice!("ab", matched as &'src str),
            ws.clone(),
            end_of_input(),
        ) => Parsed::main { matched: matched }
    );

    main.clone()
}
