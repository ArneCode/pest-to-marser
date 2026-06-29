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

#[derive(Debug, Clone, PartialEq)]
pub enum Parsed<'src> {
    WHITESPACE { value: &'src str },
    main {
        select: &'src str,
        from: &'src str,
        table: Box<Parsed<'src>>,
    },
    ident { value: &'src str },
}

pub fn grammar<'src>() -> impl Parser<'src, &'src str, Output = Parsed<'src>> + Clone {
    let ASCII_ALPHA = one_of(('a'..='z', 'A'..='Z'));

    let ASCII_ALPHANUMERIC = one_of(('a'..='z', 'A'..='Z', '0'..='9'));

    // ident = @{ ("_" | ASCII_ALPHA) ~ ("_" | ASCII_ALPHANUMERIC)* }
    let ident = capture!(
bind_slice!(
            (
                one_of(('_', ASCII_ALPHA.clone())),
                many(one_of(('_', ASCII_ALPHANUMERIC.clone()))),
            ),
        value as &'src str
    ) => Parsed::ident { value }
    );

    // WHITESPACE = _{ " " }
    let WHITESPACE = capture!(
bind_slice!(
            ' ',
        value as &'src str
    ) => Parsed::WHITESPACE { value }
    );

    let ws = many(
        WHITESPACE.clone().ignore_result()
    );

    // main = { SOI ~ #select = ^"select" ~ #from = ^"from" ~ #table = ident ~ EOI }
    let main = capture!(
        (
            start_of_input(),
            ws.clone(),
            bind_slice!((ci_ch('s'), ci_ch('e'), ci_ch('l'), ci_ch('e'), ci_ch('c'), ci_ch('t')), select as &'src str),
            ws.clone(),
            bind_slice!((ci_ch('f'), ci_ch('r'), ci_ch('o'), ci_ch('m')), from as &'src str),
            ws.clone(),
            bind!(ident.clone(), table),
            ws.clone(),
            end_of_input(),
        ) => Parsed::main { select: select, from: from, table: Box::new(table) }
    );

    main.clone()
}
