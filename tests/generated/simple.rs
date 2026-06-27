use marser::capture;
use marser::matcher::{
    AnyToken, MatcherCombinator, many, negative_lookahead, one_or_more,
    optional, positive_lookahead, start_of_input, end_of_input,
};
use marser::one_of::one_of;
use marser::parser::{
    DeferredWeak, Parser, ParserCombinator, recursive};

pub fn grammar<'src>() -> impl Parser<'src, &'src str, Output = ()> + Clone {
    let number = capture!(
        one_or_more('0'..='9') => ()
    ).erase_types();

    let ident = capture!(
        (
            one_of(('_', one_of(('a'..='z', 'A'..='Z')))),
            many(one_of(('_', one_of(('a'..='z', 'A'..='Z', '0'..='9'))))),
        ) => ()
    ).erase_types();

    let newline = capture!(
        one_of(('\n', "\r\n")) => ()
    ).erase_types();

    let WHITESPACE = capture!(
        one_of((' ', '\t', newline.clone().ignore_result())) => ()
    ).erase_types();

    let line_comment = capture!(
        ("//", many((negative_lookahead(newline.clone().ignore_result()), AnyToken))) => ()
    ).erase_types();

    let COMMENT = capture!(
        line_comment.clone().ignore_result() => ()
    ).erase_types();

    let ws = many(
        one_of((WHITESPACE.clone().ignore_result(), COMMENT.clone().ignore_result()))
    );

    let item = capture!(
        (
            ident.clone().ignore_result(),
            ws.clone(),
            '=',
            ws.clone(),
            number.clone().ignore_result(),
        ) => ()
    ).erase_types();

    let main = capture!(
        (
            start_of_input(),
            ws.clone(),
            item.clone().ignore_result(),
            ws.clone(),
            optional((
                (',', ws.clone(), item.clone().ignore_result()),
                many((ws.clone(), (',', ws.clone(), item.clone().ignore_result()))),
            )),
            ws.clone(),
            end_of_input(),
        ) => ()
    ).erase_types();

    main.clone()
}
