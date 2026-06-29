use marser::capture;
use marser::matcher::{
    one_or_more,
};
use marser::parser::{
    Parser,
    ParserCombinator,
};

pub fn grammar<'src>() -> impl Parser<'src, &'src str, Output = ()> + Clone {
    let ASCII_DIGIT = '0'..='9';

    // number = @{ ASCII_DIGIT+ }
    let number = capture!(
        one_or_more(ASCII_DIGIT.clone()) => ()
    ).erase_types();

    number.clone()
}
