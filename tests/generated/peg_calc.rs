use marser::capture;
use marser::matcher::{
    many,
    one_or_more,
};
use marser::one_of::one_of;
use marser::parser::{
    Parser,
    recursive,
};

// Typed parse tree returned by `grammar()`. Each grammar rule becomes a variant;
// labeled bindings become struct fields, and leaf rules store their matched slice
// as `value`.
#[derive(Debug, Clone, PartialEq)]
pub enum Parsed<'src> {
    expr {
        term_val: Vec<Box<Parsed<'src>>>,
    },
    term {
        factor_val: Vec<Box<Parsed<'src>>>,
    },
    factor {
        inner: Box<Parsed<'src>>,
    },
    number { value: &'src str },
}

pub fn grammar<'src>() -> impl Parser<'src, &'src str, Output = Parsed<'src>> + Clone {
    // number <- [0-9]+
    let number = capture!(
        bind_slice!(one_or_more('0'..='9'), value as &'src str) => Parsed::number { value }
    );

    // This rule cluster is cyclic: some rules refer back to others in the same
    // group (directly or indirectly). marser's `recursive` breaks that cycle
    // by giving the closure a deferred handle to clone inside the body. See:
    // https://docs.rs/marser/latest/marser/parser/deferred/fn.recursive.html
    let expr = recursive(|expr| {
        // factor <- #inner = number / ("(" #inner = expr ")")
        let factor = capture!(
            one_of((
                bind!(number.clone(), inner),
                ('(', bind!(expr.clone(), inner), ')'),
            )) => Parsed::factor {
                inner: Box::new(inner),
            }
        );

        // term <- factor (("*" / "/") factor)*
        let term = capture!(
            (
                bind!(factor.clone(), *factor_val),
                many((one_of(('*', '/')), bind!(factor.clone(), *factor_val))),
            ) => Parsed::term {
                factor_val: factor_val.into_iter().map(Box::new).collect(),
            }
        );

        // expr <- term (("+" / "-") term)*
        capture!(
            (
                bind!(term.clone(), *term_val),
                many((one_of(('+', '-')), bind!(term.clone(), *term_val))),
            ) => Parsed::expr {
                term_val: term_val.into_iter().map(Box::new).collect(),
            }
        )
    });

    expr.clone()
}
