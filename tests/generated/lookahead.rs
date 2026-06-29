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

pub fn grammar<'src>() -> impl Parser<'src, &'src str, Output = ()> + Clone {
    let ASCII_ALPHA = one_of(('a'..='z', 'A'..='Z'));

    let ASCII_ALPHANUMERIC = one_of(('a'..='z', 'A'..='Z', '0'..='9'));

    // ident = @{ ("_" | ASCII_ALPHA) ~ ("_" | ASCII_ALPHANUMERIC)* }
    let ident = capture!(
        (
            one_of(('_', ASCII_ALPHA.clone())),
            many(one_of(('_', ASCII_ALPHANUMERIC.clone()))),
        ) => ()
    ).erase_types();

    // WHITESPACE = _{ " " }
    let WHITESPACE = capture!(
        ' ' => ()
    ).erase_types();

    let ws = many(
        WHITESPACE.clone().ignore_result()
    );

    // main = { SOI ~ ident ~ (!"end" ~ ANY)* ~ "end" ~ EOI }
    let main = capture!(
        (
            start_of_input(),
            ws.clone(),
            bind!(ident.clone(), ident_val),
            ws.clone(),
            repeat_ws((negative_lookahead("end"), ws.clone(), AnyToken), ws.clone()),
            ws.clone(),
            "end",
            ws.clone(),
            end_of_input(),
        ) => ()
    ).erase_types();

    main.clone()
}
