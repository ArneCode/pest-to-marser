use marser::capture;
use marser::matcher::{
    Matcher,
    many,
    repeat,
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
        hex_color_val: Box<Parsed<'src>>,
    },
    hex_color {
        body: Vec<Box<Parsed<'src>>>,
    },
    hex_digit { value: &'src str },
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

    // hex_digit = _{ '0'..'9' | 'a'..'f' | 'A'..'F' }
    let hex_digit = capture!(
bind_slice!(
            one_of(('0'..='9', 'a'..='f', 'A'..='F')),
        value as &'src str
    ) => Parsed::hex_digit { value }
    );

    // hex_color = { "#" ~ #body = hex_digit{6} }
    let hex_color = capture!(
        (
            '#',
            ws.clone(),
            (bind!(hex_digit.clone(), *body), repeat((ws.clone(), bind!(hex_digit.clone(), *body)), 5..=5)),
        ) => Parsed::hex_color { body: body.into_iter().map(Box::new).collect() }
    );

    // main = { SOI ~ hex_color ~ EOI }
    let main = capture!(
        (start_of_input(), ws.clone(), bind!(hex_color.clone(), hex_color_val), ws.clone(), end_of_input()) => Parsed::main { hex_color_val: Box::new(hex_color_val) }
    );

    main.clone()
}
