use marser::capture;
use marser::matcher::{
    many,
    one_or_more,
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
        item_val: Vec<Box<Parsed<'src>>>,
    },
    item {
        name: Box<Parsed<'src>>,
        value: Box<Parsed<'src>>,
    },
    ident { value: &'src str },
    number { value: &'src str },
}

pub fn grammar<'src>() -> impl Parser<'src, &'src str, Output = Parsed<'src>> + Clone {
    // number <- [0-9]+
    let number = capture!(
        bind_slice!(one_or_more('0'..='9'), value as &'src str) => Parsed::number { value }
    );

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

    // item <- #name = ident "=" #value = number
    let item = capture!(
        (
            bind!(ident.clone(), name),
            '=',
            bind!(number.clone(), value),
        ) => Parsed::item {
            name: Box::new(name),
            value: Box::new(value),
        }
    );

    // main <- item ("," item)*
    let main = capture!(
        (
            bind!(item.clone(), *item_val),
            many((',', bind!(item.clone(), *item_val))),
        ) => Parsed::main {
            item_val: item_val.into_iter().map(Box::new).collect(),
        }
    );

    main.clone()
}
