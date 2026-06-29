use marser::capture;
use marser::matcher::{
    Matcher,
    many,
    one_or_more,
    optional,
};
use marser::one_of::one_of;
use marser::parser::{
    Parser,
    ParserCombinator,
    recursive,
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
    let ASCII_DIGIT = '0'..='9';

    // number = @{ ASCII_DIGIT+ }
    let number = capture!(
        one_or_more(ASCII_DIGIT.clone()) => ()
    ).erase_types();

    // WHITESPACE = _{ " " | "\t" }
    let WHITESPACE = capture!(
        one_of((' ', '\t')) => ()
    ).erase_types();

    let ws = many(
        WHITESPACE.clone().ignore_result()
    );

    // This rule cluster is cyclic: some rules refer back to others in the same
    // group (directly or indirectly). marser's `recursive` breaks that cycle
    // by giving the closure a deferred handle to clone inside the body. See:
    // https://docs.rs/marser/latest/marser/parser/deferred/fn.recursive.html
    let expr = recursive(|expr| {
        // factor = { number | "(" ~ expr ~ ")" }
        let factor = capture!(
            one_of((
                bind!(number.clone(), ?number_val),
                ('(', ws.clone(), bind!(expr.clone(), ?expr_val), ws.clone(), ')'),
            )) => ()
        ).erase_types();

        // term = { factor ~ (("*" | "/") ~ factor)* }
        let term = capture!(
            (
                bind!(factor.clone(), *factor_val),
                ws.clone(),
                repeat_ws((one_of(('*', '/')), ws.clone(), bind!(factor.clone(), *factor_val)), ws.clone()),
            ) => ()
        ).erase_types();

        // expr = { term ~ (("+" | "-") ~ term)* }
        capture!(
            (
                bind!(term.clone(), *term_val),
                ws.clone(),
                repeat_ws((one_of(('+', '-')), ws.clone(), bind!(term.clone(), *term_val)), ws.clone()),
            ) => ()
        ).erase_types()
    });

    expr.clone()
}
