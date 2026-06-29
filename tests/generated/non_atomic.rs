use marser::capture;
use marser::matcher::{
    Matcher,
    many,
    start_of_input,
    end_of_input,
};
use marser::one_of::one_of;
use marser::parser::{
    Parser,
    ParserCombinator,
};

// Pest `X+` requires a first item, then implicit whitespace only between
// later repetitions. This helper preserves that shape without duplicating
// the generated matcher body for `X`.
fn repeat_one_or_more_ws<'src, MRes, Item, Ws>(
    item: Item,
    ws: Ws,
) -> impl Matcher<'src, &'src str, MRes> + Clone
where
    Item: Matcher<'src, &'src str, MRes> + Clone,
    Ws: Matcher<'src, &'src str, MRes> + Clone,
{
    (item.clone(), many((ws, item)))
}

#[derive(Debug, Clone, PartialEq)]
pub enum Parsed<'src> {
    WHITESPACE { value: &'src str },
    main {
        spaced: Box<Parsed<'src>>,
    },
    spaced {
        first: Box<Parsed<'src>>,
        rest: Vec<Box<Parsed<'src>>>,
    },
    letter { value: &'src str },
}

pub fn grammar<'src>() -> impl Parser<'src, &'src str, Output = Parsed<'src>> + Clone {
    let ASCII_ALPHA = one_of(('a'..='z', 'A'..='Z'));

    // letter = @{ ASCII_ALPHA }
    let letter = capture!(
bind_slice!(
            ASCII_ALPHA.clone(),
        value as &'src str
    ) => Parsed::letter { value }
    );

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

    // spaced = !{ #first = letter ~ ((" " ~ #rest = letter)+) }
    let spaced = capture!(
        (
            bind!(letter.clone(), first),
            ws.clone(),
            repeat_one_or_more_ws((' ', ws.clone(), bind!(letter.clone(), *rest)), ws.clone()),
        ) => Parsed::spaced { first: Box::new(first), rest: rest.into_iter().map(Box::new).collect() }
    );

    // main = { SOI ~ #spaced = spaced ~ EOI }
    let main = capture!(
        (start_of_input(), ws.clone(), bind!(spaced.clone(), spaced_val), ws.clone(), end_of_input()) => Parsed::main { spaced: Box::new(spaced_val) }
    );

    main.clone()
}
