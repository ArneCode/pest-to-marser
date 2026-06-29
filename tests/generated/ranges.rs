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

pub fn grammar<'src>() -> impl Parser<'src, &'src str, Output = ()> + Clone {
    // WHITESPACE = _{ " " }
    let WHITESPACE = capture!(
        ' ' => ()
    ).erase_types();

    let ws = many(
        WHITESPACE.clone().ignore_result()
    );

    // hex_digit = _{ '0'..'9' | 'a'..'f' | 'A'..'F' }
    let hex_digit = capture!(
        one_of(('0'..='9', 'a'..='f', 'A'..='F')) => ()
    ).erase_types();

    // hex_color = { "#" ~ hex_digit{6} }
    let hex_color = capture!(
        (
            '#',
            ws.clone(),
            (bind!(hex_digit.clone(), *hex_digit_val), repeat((ws.clone(), bind!(hex_digit.clone(), *hex_digit_val)), 5..=5)),
        ) => ()
    ).erase_types();

    // main = { SOI ~ hex_color ~ EOI }
    let main = capture!(
        (start_of_input(), ws.clone(), bind!(hex_color.clone(), hex_color_val), ws.clone(), end_of_input()) => ()
    ).erase_types();

    main.clone()
}
