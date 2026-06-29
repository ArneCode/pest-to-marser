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

#[derive(Debug, Clone, PartialEq)]
pub enum Parsed<'src> {
    expr {
        term_val: Vec<Box<Parsed<'src>>>,
        op: Vec<&'src str>,
    },
    term {
        factor_val: Vec<Box<Parsed<'src>>>,
        op: Vec<&'src str>,
    },
    factor {
        inner: Vec<Box<Parsed<'src>>>,
    },
    number { value: &'src str },
    WHITESPACE { value: &'src str },
}

pub fn grammar<'src>() -> impl Parser<'src, &'src str, Output = Parsed<'src>> + Clone {
    let ASCII_DIGIT = '0'..='9';

    // number = @{ ASCII_DIGIT+ }
    let number = capture!(
bind_slice!(
            one_or_more(ASCII_DIGIT.clone()),
        value as &'src str
    ) => Parsed::number { value }
    );

    // WHITESPACE = _{ " " | "\t" }
    let WHITESPACE = capture!(
bind_slice!(
            one_of((' ', '\t')),
        value as &'src str
    ) => Parsed::WHITESPACE { value }
    );

    let ws = many(
        WHITESPACE.clone().ignore_result()
    );

    // This rule cluster is cyclic: some rules refer back to others in the same
    // group (directly or indirectly). marser's `recursive` breaks that cycle
    // by giving the closure a deferred handle to clone inside the body. See:
    // https://docs.rs/marser/latest/marser/parser/deferred/fn.recursive.html
    let expr = recursive(|expr| {
        // factor = { #inner = number | "(" ~ #inner = expr ~ ")" }
        let factor = capture!(
            one_of((
                bind!(number.clone(), *inner),
                ('(', ws.clone(), bind!(expr.clone(), *inner), ws.clone(), ')'),
            )) => Parsed::factor { inner: inner.into_iter().map(Box::new).collect() }
        );

        // term = { factor ~ ( #op = ("*" | "/") ~ factor )* }
        let term = capture!(
            (
                bind!(factor.clone(), *factor_val),
                ws.clone(),
                repeat_ws((bind_slice!(one_of(('*', '/')), *op as &'src str), ws.clone(), bind!(factor.clone(), *factor_val)), ws.clone()),
            ) => Parsed::term { factor_val: factor_val.into_iter().map(Box::new).collect(), op: op }
        );

        // expr = { term ~ ( #op = ("+" | "-") ~ term )* }
        capture!(
            (
                bind!(term.clone(), *term_val),
                ws.clone(),
                repeat_ws((bind_slice!(one_of(('+', '-')), *op as &'src str), ws.clone(), bind!(term.clone(), *term_val)), ws.clone()),
            ) => Parsed::expr { term_val: term_val.into_iter().map(Box::new).collect(), op: op }
        )
    });

    expr.clone()
}
