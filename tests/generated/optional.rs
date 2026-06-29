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

pub fn grammar<'src>() -> impl Parser<'src, &'src str, Output = ()> + Clone {
    let ASCII_DIGIT = '0'..='9';

    // digits = @{ ASCII_DIGIT+ }
    let digits = capture!(
        one_or_more(ASCII_DIGIT.clone()) => ()
    ).erase_types();

    // WHITESPACE = _{ " " }
    let WHITESPACE = capture!(
        ' ' => ()
    ).erase_types();

    let ws = many(
        WHITESPACE.clone().ignore_result()
    );

    // sign = { "+" | "-" }
    let sign = capture!(
        one_of(('+', '-')) => ()
    ).erase_types();

    // main = { SOI ~ sign? ~ digits ~ EOI }
    let main = capture!(
        (
            start_of_input(),
            ws.clone(),
            optional(bind!(sign.clone(), ?sign_val)),
            ws.clone(),
            bind!(digits.clone(), digits_val),
            ws.clone(),
            end_of_input(),
        ) => ()
    ).erase_types();

    main.clone()
}
