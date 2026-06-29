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

// Pest `^"..."` literals match ASCII letters case-insensitively.
fn ci_ch<'src, MRes>(c: char) -> impl Matcher<'src, &'src str, MRes> + Clone {
    one_of((c.to_ascii_lowercase(), c.to_ascii_uppercase()))
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

    // main = { SOI ~ ^"select" ~ ^"from" ~ ident ~ EOI }
    let main = capture!(
        (
            start_of_input(),
            ws.clone(),
            (ci_ch('s'), ci_ch('e'), ci_ch('l'), ci_ch('e'), ci_ch('c'), ci_ch('t')),
            ws.clone(),
            (ci_ch('f'), ci_ch('r'), ci_ch('o'), ci_ch('m')),
            ws.clone(),
            bind!(ident.clone(), ident_val),
            ws.clone(),
            end_of_input(),
        ) => ()
    ).erase_types();

    main.clone()
}
