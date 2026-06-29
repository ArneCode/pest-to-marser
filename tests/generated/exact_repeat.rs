use marser::capture;
use marser::matcher::{
    Matcher,
    many,
    repeat,
    optional,
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
        chars: Vec<&'src str>,
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

    // main = { SOI ~ #chars = "a"{3} ~ EOI }
    let main = capture!(
        (start_of_input(), ws.clone(), bind_slice!(('a', repeat((ws.clone(), 'a'), 2..=2)), *chars as &'src str), ws.clone(), end_of_input()) => Parsed::main { chars: chars }
    );

    main.clone()
}
