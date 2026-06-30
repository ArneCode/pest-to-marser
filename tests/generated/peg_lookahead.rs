use marser::capture;
use marser::matcher::{
    AnyToken,
    many,
    negative_lookahead,
};
use marser::one_of::one_of;
use marser::parser::{
    Parser,
};

// Typed parse tree returned by `grammar()`. Each grammar rule becomes a variant;
// labeled bindings become struct fields, and leaf rules store their matched slice
// as `value`.
#[derive(Debug, Clone, PartialEq)]
pub enum Parsed<'src> {
    main {
        id: Box<Parsed<'src>>,
        prefix: Vec<&'src str>,
    },
    ident { value: &'src str },
}

pub fn grammar<'src>() -> impl Parser<'src, &'src str, Output = Parsed<'src>> + Clone {
    // ident <- ("_" / [a-zA-Z]) [a-zA-Z0-9_]*
    let ident = capture!(
        bind_slice!(
            (
                one_of(('_', one_of(('a'..='z', 'A'..='Z')))),
                many(one_of(('a'..='z', 'A'..='Z', '0'..='9', '_'))),
            ),
            value as &'src str
        ) => Parsed::ident { value }
    );

    // main <- #id = ident #prefix = (!"end" .)* "end"
    let main = capture!(
        (
            bind!(ident.clone(), id),
            many(
                bind_slice!(
                    (negative_lookahead("end"), AnyToken),
                    *prefix as &'src str
                ),
            ),
            "end",
        ) => Parsed::main {
            id: Box::new(id),
            prefix: prefix,
        }
    );

    main.clone()
}
