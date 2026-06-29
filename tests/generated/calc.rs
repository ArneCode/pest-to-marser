use marser::capture;
use marser::matcher::{
    AnyToken, Matcher, MatcherCombinator, many, negative_lookahead, one_or_more,
    optional, positive_lookahead, start_of_input, end_of_input,
};
use marser::one_of::one_of;
use marser::parser::{
    DeferredWeak, Parser, ParserCombinator, recursive};

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
    let number = capture!(
        one_or_more('0'..='9') => ()
    ).erase_types();

    let WHITESPACE = capture!(
        one_of((' ', '\t')) => ()
    ).erase_types();

    let ws = many(
        WHITESPACE.clone().ignore_result()
    );

    let expr = recursive(|expr_weak| {
        let factor = capture!(
            one_of((
                number.clone().ignore_result(),
                ('(', ws.clone(), expr_weak.clone().ignore_result(), ws.clone(), ')'),
            )) => ()
        ).erase_types();

        let term = capture!(
            (
                factor.clone().ignore_result(),
                ws.clone(),
                repeat_ws(
                    (one_of(('*', '/')), ws.clone(), factor.clone().ignore_result()),
                    ws.clone(),
                ),
            ) => ()
        ).erase_types();

        capture!(
            (
                term.clone().ignore_result(),
                ws.clone(),
                repeat_ws(
                    (one_of(('+', '-')), ws.clone(), term.clone().ignore_result()),
                    ws.clone(),
                ),
            ) => ()
        ).erase_types()
    });

    expr.clone()
}
