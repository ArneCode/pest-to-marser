use marser::capture;
use marser::matcher::{
    AnyToken,
    Matcher,
    many,
    negative_lookahead,
    optional,
    start_of_input,
    end_of_input,
};
use marser::one_of::one_of;
use marser::parser::{
    Parser,
    ParserCombinator,
};

// Pest inserts implicit whitespace between repetitions, but not before the
// first item. This keeps `X*` equivalent to Pest while avoiding duplicated
// generated matcher bodies.
fn repeat_ws<'src, MRes, Item, Ws>(
    item: Item,
    ws: Ws,
) -> impl Matcher<'src, &'src str, MRes> + Clone
where
    Item: Matcher<'src, &'src str, MRes> + Clone,
    Ws: Matcher<'src, &'src str, MRes> + Clone,
{
    optional((item.clone(), many((ws, item))))
}

#[derive(Debug, Clone, PartialEq)]
pub enum Parsed<'src> {
    WHITESPACE { value: &'src str },
    main {
        id: Box<Parsed<'src>>,
        prefix: Vec<&'src str>,
    },
    ident { value: &'src str },
}

pub fn grammar<'src>() -> impl Parser<'src, &'src str, Output = Parsed<'src>> + Clone {
    let ASCII_ALPHA = one_of(('a'..='z', 'A'..='Z'));

    let ASCII_ALPHANUMERIC = one_of(('a'..='z', 'A'..='Z', '0'..='9'));

    // ident = @{ ("_" | ASCII_ALPHA) ~ ("_" | ASCII_ALPHANUMERIC)* }
    let ident = capture!(
bind_slice!(
            (
                one_of(('_', ASCII_ALPHA.clone())),
                many(one_of(('_', ASCII_ALPHANUMERIC.clone()))),
            ),
        value as &'src str
    ) => Parsed::ident { value }
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

    // main = { SOI ~ #id = ident ~ #prefix = (!"end" ~ ANY)* ~ "end" ~ EOI }
    let main = capture!(
        (
            start_of_input(),
            ws.clone(),
            bind!(ident.clone(), id),
            ws.clone(),
            bind_slice!(repeat_ws((negative_lookahead("end"), ws.clone(), AnyToken), ws.clone()), *prefix as &'src str),
            ws.clone(),
            "end",
            ws.clone(),
            end_of_input(),
        ) => Parsed::main { id: Box::new(id), prefix: prefix }
    );

    main.clone()
}
