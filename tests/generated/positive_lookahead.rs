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

pub fn grammar<'src>() -> impl Parser<'src, &'src str, Output = ()> + Clone {
    // WHITESPACE = _{ " " }
    let WHITESPACE = capture!(
        ' ' => ()
    ).erase_types();

    let ws = many(
        WHITESPACE.clone().ignore_result()
    );

    // main = { SOI ~ &"ab" ~ "ab" ~ EOI }
    let main = capture!(
        (
            start_of_input(),
            ws.clone(),
            positive_lookahead("ab"),
            ws.clone(),
            "ab",
            ws.clone(),
            end_of_input(),
        ) => ()
    ).erase_types();

    main.clone()
}
