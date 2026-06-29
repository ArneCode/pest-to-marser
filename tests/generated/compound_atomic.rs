use marser::capture;
use marser::matcher::{
    Matcher,
    many,
    one_or_more,
    start_of_input,
    end_of_input,
};
use marser::one_of::one_of;
use marser::parser::{
    Parser,
    ParserCombinator,
};

pub fn grammar<'src>() -> impl Parser<'src, &'src str, Output = ()> + Clone {
    let ASCII_ALPHA = one_of(('a'..='z', 'A'..='Z'));

    // letter = { ASCII_ALPHA }
    let letter = capture!(
        ASCII_ALPHA.clone() => ()
    ).erase_types();

    // word = ${ letter+ }
    let word = capture!(
        one_or_more(bind!(letter.clone(), *letter_val)) => ()
    ).erase_types();

    // WHITESPACE = _{ " " }
    let WHITESPACE = capture!(
        ' ' => ()
    ).erase_types();

    let ws = many(
        WHITESPACE.clone().ignore_result()
    );

    // main = { SOI ~ word ~ EOI }
    let main = capture!(
        (start_of_input(), ws.clone(), bind!(word.clone(), word_val), ws.clone(), end_of_input()) => ()
    ).erase_types();

    main.clone()
}
