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

// Typed parse tree returned by `grammar()`. Each Pest rule becomes a variant;
// `#field = ...` bindings become struct fields, and atomic (`@`) leaves store
// their matched slice as `value`.
#[derive(Debug, Clone, PartialEq)]
pub enum Parsed<'src> {
    main {
        chars: Vec<&'src str>,
    },
}

pub fn grammar<'src>() -> impl Parser<'src, &'src str, Output = Parsed<'src>> + Clone {
    // WHITESPACE = _{ " " }
    let WHITESPACE = ' ';

    let ws = many(
        WHITESPACE.clone()
    );

    // main = { SOI ~ #chars = "a"{3} ~ EOI }
    let main = capture!(
        (
            start_of_input(),
            ws.clone(),
            (
                bind_slice!('a', *chars as &'src str),
                repeat(
                    (ws.clone(), bind_slice!('a', *chars as &'src str)),
                    2..=2,
                ),
            ),
            ws.clone(),
            end_of_input(),
        ) => Parsed::main { chars: chars }
    );

    main.clone()
}
