//! Parser for the supported generic-PEG subset.
//!
//! v1 syntax:
//! - Rules: `name <- expression`
//! - Ordered choice: `a / b`
//! - Sequence: juxtaposition `a b`
//! - Prefix: `&`, `!`
//! - Postfix: `?`, `*`, `+`
//! - Grouping: `( expression )`
//! - Literals: `"..."` and `'...'`
//! - Character classes: `[a-z]`, `[abc]`
//! - Any character: `.`
//! - Rule references: identifiers
//! - Optional bind labels: `label = term`
//! - Comments: `//` to end of line
//!
//! PEG mode does not use Pest implicit whitespace or rule modifiers.

use marser::capture;
use marser::{
    label::WithLabel,
    matcher::{
        AnyToken, Matcher, MatcherCombinator, commit_on, end_of_input, many, negative_lookahead,
        optional, start_of_input,
    },
    one_of::one_of,
    parser::{DeferredWeak, Parser, ParserCombinator, recursive},
};

use crate::ast::*;

fn newline<'src, MRes>() -> impl Matcher<'src, &'src str, MRes> {
    one_of(("\n", "\r\n"))
}

fn peg_comment<'src, MRes>() -> impl Matcher<'src, &'src str, MRes> + Clone {
    (
        "//",
        many((negative_lookahead(newline()), AnyToken)),
        optional(newline()),
    )
}

fn peg_ws<'src, MRes>() -> impl Matcher<'src, &'src str, MRes> + Clone {
    many(one_of((" ", "\t", "\r", "\n", peg_comment())))
}

fn peg_identifier<'src>() -> impl Parser<'src, &'src str, Output = String> + Clone {
    capture!((
        bind_slice!((
            one_of(('_', 'a'..='z', 'A'..='Z')),
            many(one_of(('_', 'a'..='z', 'A'..='Z', '0'..='9'))),
        ), id as &'src str),
        peg_ws(),
    ) => id.to_string())
    .with_label("identifier")
}

fn peg_identifier_syntax<'src, MRes>() -> impl Matcher<'src, &'src str, MRes> + Clone {
    (
        one_of(('_', 'a'..='z', 'A'..='Z')),
        many(one_of(('_', 'a'..='z', 'A'..='Z', '0'..='9'))),
    )
}

fn peg_escape_seq<'src>() -> impl Parser<'src, &'src str, Output = char> + Clone {
    one_of((
        'n'.to('\n'),
        'r'.to('\r'),
        't'.to('\t'),
        '\\'.to('\\'),
        '"'.to('"'),
        '\''.to('\''),
        capture!((
            'x',
            bind_slice!((
                one_of(('0'..='9', 'a'..='f', 'A'..='F')),
                one_of(('0'..='9', 'a'..='f', 'A'..='F')),
            ), hex as &str),
        ) => char::from_u32(u32::from_str_radix(hex, 16).unwrap()).unwrap()),
    ))
}

fn peg_escaped_char<'src>() -> impl Parser<'src, &'src str, Output = char> + Clone {
    capture!((
        '\\',
        bind!(peg_escape_seq(), ch),
    ) => ch)
    .erase_types()
}

fn peg_any_char<'src>() -> impl Parser<'src, &'src str, Output = char> + Clone {
    capture!(bind_slice!((AnyToken,), s as &str) => s.chars().next().unwrap())
}

fn peg_string_content<'src>() -> impl Parser<'src, &'src str, Output = String> + Clone {
    recursive(|inner: DeferredWeak<'_, '_, &str, String>| {
        let tail = capture!((
            bind!(peg_escaped_char(), esc),
            bind!(inner.clone(), rest as String),
        ) => {
            let mut s = String::new();
            s.push(esc);
            s.push_str(&rest);
            s
        });

        capture!((
            bind_slice!(
                many((negative_lookahead(one_of(('"', '\\'))), AnyToken)),
                prefix as &'src str
            ),
            optional(bind!(tail, ?suffix_part as String)),
        ) => {
            let mut s = prefix.to_string();
            if let Some(suffix_part) = suffix_part {
                s.push_str(&suffix_part);
            }
            s
        })
    })
    .erase_types()
}

fn peg_string<'src>() -> impl Parser<'src, &'src str, Output = String> + Clone {
    capture!((
        '"',
        bind!(peg_string_content(), content),
        '"',
        peg_ws(),
    ) => content)
    .with_label("string literal")
}

fn peg_char<'src>() -> impl Parser<'src, &'src str, Output = char> + Clone {
    capture!((
        '\'',
        bind!(one_of((peg_escaped_char(), peg_any_char())), ch),
        '\'',
        peg_ws(),
    ) => ch)
    .with_label("char literal")
}

fn peg_class_char<'src>() -> impl Parser<'src, &'src str, Output = char> + Clone {
    one_of((
        peg_escaped_char(),
        capture!((
            negative_lookahead(one_of((']', '-'))),
            bind!(peg_any_char(), ch),
        ) => ch),
    ))
    .erase_types()
}

fn peg_class_item_to_expr(start: char, end: char) -> Expression {
    if start == end {
        Expression {
            leading_choice: false,
            terms: vec![Term {
                tag: None,
                prefix_ops: vec![],
                node: Node::Terminal(Terminal::String(start.to_string())),
                postfix_ops: vec![],
            }],
            infix_ops: vec![],
        }
    } else {
        Expression {
            leading_choice: false,
            terms: vec![Term {
                tag: None,
                prefix_ops: vec![],
                node: Node::Terminal(Terminal::Range { start, end }),
                postfix_ops: vec![],
            }],
            infix_ops: vec![],
        }
    }
}

fn peg_class_item<'src>() -> impl Parser<'src, &'src str, Output = (char, Option<char>)> + Clone {
    one_of((
        capture!((
            bind!(peg_class_char(), start),
            optional(bind!(
                capture!(('-', bind!(peg_class_char(), end)) => end),
                ?range_end
            )),
        ) => (start, range_end)),
        capture!((
            bind!(peg_class_char(), only),
        ) => (only, None::<char>)),
    ))
    .erase_types()
}

fn peg_char_class<'src>() -> impl Parser<'src, &'src str, Output = Expression> + Clone {
    capture!((
        '[',
        peg_ws(),
        many(bind!(peg_class_item(), *items)),
        ']',
        peg_ws(),
    ) => {
        if items.is_empty() {
            return Expression {
                leading_choice: false,
                terms: vec![],
                infix_ops: vec![],
            };
        }
        let mut terms = Vec::new();
        for (start, end) in items {
            let expr = match end {
                Some(end) => peg_class_item_to_expr(start, end),
                None => peg_class_item_to_expr(start, start),
            };
            terms.push(expr.terms.into_iter().next().unwrap());
        }
        let len = terms.len();
        if len == 1 {
            Expression {
                leading_choice: false,
                terms,
                infix_ops: vec![],
            }
        } else {
            Expression {
                leading_choice: false,
                terms,
                infix_ops: vec![InfixOp::Choice; len - 1],
            }
        }
    })
    .erase_types()
}

fn peg_prefix_op<'src>() -> impl Parser<'src, &'src str, Output = PrefixOp> + Clone {
    one_of((
        '&'.map_output(|_| PrefixOp::PositivePredicate),
        '!'.map_output(|_| PrefixOp::NegativePredicate),
    ))
}

fn peg_postfix_op<'src>() -> impl Parser<'src, &'src str, Output = PostfixOp> + Clone {
    one_of((
        '?'.map_output(|_| PostfixOp::Optional),
        '*'.map_output(|_| PostfixOp::Repeat),
        '+'.map_output(|_| PostfixOp::RepeatOnce),
    ))
}

fn peg_tag_name<'src>() -> impl Parser<'src, &'src str, Output = String> + Clone {
    capture!((
        optional('#'),
        bind!(peg_identifier(), tag),
        '=',
        peg_ws(),
    ) => tag)
}

fn peg_fold_sequence(terms: Vec<Term>) -> Expression {
    let len = terms.len();
    if len == 1 {
        Expression {
            leading_choice: false,
            terms,
            infix_ops: vec![],
        }
    } else {
        Expression {
            leading_choice: false,
            terms,
            infix_ops: vec![InfixOp::Sequence; len - 1],
        }
    }
}

fn peg_sequence_to_term(seq: Expression) -> Term {
    if seq.terms.len() == 1 && seq.infix_ops.is_empty() {
        seq.terms.into_iter().next().unwrap()
    } else {
        Term {
            tag: None,
            prefix_ops: vec![],
            postfix_ops: vec![],
            node: Node::Grouped(Box::new(seq)),
        }
    }
}

fn peg_fold_choice(sequences: Vec<Expression>) -> Expression {
    let terms: Vec<Term> = sequences.into_iter().map(peg_sequence_to_term).collect();
    let len = terms.len();
    Expression {
        leading_choice: false,
        terms,
        infix_ops: vec![InfixOp::Choice; len.saturating_sub(1)],
    }
}

fn peg_expression<'src>() -> impl Parser<'src, &'src str, Output = Expression> + Clone {
    recursive(|expr_weak: DeferredWeak<'_, '_, &str, Expression>| {
        let rule_start = (
            peg_identifier_syntax(),
            peg_ws(),
            '<',
            '-',
            peg_ws(),
        );
        let atom = one_of((
            peg_string()
                .map_output(|s| Node::Terminal(Terminal::String(s)))
                .erase_types(),
            peg_char()
                .map_output(|c: char| Node::Terminal(Terminal::String(c.to_string())))
                .erase_types(),
            peg_char_class().map_output(|e| Node::Grouped(Box::new(e))).erase_types(),
            '.'.map_output(|_| Node::Terminal(Terminal::Identifier("ANY".to_string())))
                .erase_types(),
            peg_identifier()
                .map_output(|id| Node::Terminal(Terminal::Identifier(id)))
                .erase_types(),
            capture!((
                '(',
                peg_ws(),
                bind!(expr_weak.clone(), inner),
                peg_ws(),
                ')',
                peg_ws(),
            ) => Node::Grouped(Box::new(inner))),
        ))
        .with_label("term");

        let term = capture!((
            optional(bind!(
                peg_tag_name(),
                ?tag
            )),
            many(bind!(peg_prefix_op(), *prefix_ops)),
            bind!(atom, n),
            many(bind!(peg_postfix_op(), *postfix_ops)),
            peg_ws(),
        ) => Term {
            tag,
            prefix_ops,
            node: n,
            postfix_ops,
        })
        .with_label("expression");

        let peg_sequence = capture!((
            bind!(term.clone(), first),
            many((
                negative_lookahead(rule_start.clone()),
                bind!(term, *rest),
            )),
        ) => {
            let mut all_terms = vec![first];
            all_terms.extend(rest);
            peg_fold_sequence(all_terms)
        });

        capture!((
            bind!(peg_sequence.clone(), first),
            many((
                '/',
                peg_ws(),
                bind!(peg_sequence, *rest),
            )),
        ) => {
            let mut sequences = vec![first];
            sequences.extend(rest);
            peg_fold_choice(sequences)
        })
    })
    .erase_types()
}

fn peg_rule<'src>() -> impl Parser<'src, &'src str, Output = GrammarItem> + Clone {
    capture!((
        commit_on(
            (
                bind!(peg_identifier(), name),
                '<',
                '-',
                peg_ws(),
            ),
            bind!(peg_expression(), expr),
        ),
    ) => GrammarItem::Rule(GrammarRule::Valid {
        name,
        modifier: None,
        expression: expr,
    }))
    .erase_types()
}

fn next_peg_rule_start<'src, MRes>() -> impl Matcher<'src, &'src str, MRes> + Clone {
    let rule_start = (peg_identifier_syntax(), peg_ws(), '<', '-', peg_ws());
    one_of((end_of_input(), (newline(), peg_ws(), rule_start)))
}

fn recover_peg_rule<'src>() -> impl Parser<'src, &'src str, Output = GrammarItem> + Clone {
    capture!((
        bind!(peg_identifier(), name),
        '<',
        '-',
        peg_ws(),
        bind_slice!(
            many((negative_lookahead(next_peg_rule_start()), AnyToken)),
            text as &'src str
        ),
        peg_ws(),
    ) => GrammarItem::Rule(GrammarRule::Invalid {
        name,
        text: text.to_string(),
    }))
    .erase_types()
}

fn peg_item<'src>() -> impl Parser<'src, &'src str, Output = GrammarItem> + Clone {
    peg_rule()
        .recover_with(recover_peg_rule())
        .with_label("PEG rule")
        .erase_types()
}

pub fn parse_peg_grammar<'src>() -> impl Parser<'src, &'src str, Output = Grammar> + Clone {
    capture!((
        start_of_input(),
        peg_ws(),
        many(bind!(peg_item(), *rules)),
        peg_ws(),
        end_of_input(),
    ) => Grammar {
        items: rules,
    })
    .erase_types()
}

#[cfg(test)]
mod tests {
    use super::*;
    use marser::parser::Parser;

    #[test]
    fn parses_simple_peg_rule() {
        let src = r#"main <- "hello""#;
        let grammar = parse_peg_grammar().parse_str(src).unwrap().0;
        assert_eq!(grammar.items.len(), 1);
        match &grammar.items[0] {
            GrammarItem::Rule(GrammarRule::Valid { name, modifier, .. }) => {
                assert_eq!(name, "main");
                assert_eq!(*modifier, None);
            }
            other => panic!("expected rule, got {other:?}"),
        }
    }

    #[test]
    fn parses_peg_choice_and_sequence() {
        let src = r#"expr <- term / factor
term <- ident "+" factor"#;
        let grammar = parse_peg_grammar().parse_str(src).unwrap().0;
        assert_eq!(grammar.items.len(), 2);
    }

    #[test]
    fn parses_peg_char_class() {
        let src = r#"digit <- [0-9]"#;
        let grammar = parse_peg_grammar().parse_str(src).unwrap().0;
        assert_eq!(grammar.items.len(), 1);
    }

    #[test]
    fn parses_peg_hash_tag_syntax() {
        let src = r#"item <- #name=ident
ident <- [A-Za-z_][A-Za-z0-9_]*
number <- [0-9]+"#;
        let grammar = parse_peg_grammar().parse_str(src).unwrap().0;
        assert_eq!(grammar.items.len(), 3);
        match &grammar.items[0] {
            GrammarItem::Rule(GrammarRule::Valid { expression, .. }) => {
                assert!(expression.terms.iter().any(|t| t.tag.as_deref() == Some("name")));
            }
            other => panic!("expected valid rule, got {other:?}"),
        }
    }

    #[test]
    fn recovers_invalid_rule_and_parses_following_rule() {
        let src = r#"bad <- (
good <- "ok""#;
        let (grammar, errors) = parse_peg_grammar().parse_str(src).unwrap();
        assert!(!errors.is_empty());
        assert_eq!(grammar.items.len(), 2);
        match &grammar.items[0] {
            GrammarItem::Rule(rule) => assert_eq!(rule.name(), "bad"),
            other => panic!("expected recovered rule, got {other:?}"),
        }
        match &grammar.items[1] {
            GrammarItem::Rule(GrammarRule::Valid { name, .. }) => assert_eq!(name, "good"),
            other => panic!("expected valid rule, got {other:?}"),
        }
    }

    #[test]
    fn recovers_multiple_invalid_rules() {
        let src = r#"bad <- (
worse <- (
good <- "ok""#;
        let (grammar, errors) = parse_peg_grammar().parse_str(src).unwrap();
        assert!(
            errors.len() >= 2,
            "expected at least two recovered errors, got {}",
            errors.len()
        );
        assert_eq!(grammar.items.len(), 3);
        match &grammar.items[0] {
            GrammarItem::Rule(rule) => assert_eq!(rule.name(), "bad"),
            other => panic!("expected recovered rule, got {other:?}"),
        }
        match &grammar.items[1] {
            GrammarItem::Rule(rule) => assert_eq!(rule.name(), "worse"),
            other => panic!("expected recovered rule, got {other:?}"),
        }
        match &grammar.items[2] {
            GrammarItem::Rule(GrammarRule::Valid { name, .. }) => assert_eq!(name, "good"),
            other => panic!("expected valid rule, got {other:?}"),
        }
    }

    #[test]
    fn recovery_rule_consumes_until_next_rule_start() {
        let src = r#"bad <- "unclosed"#;
        let (item, errors) = recover_peg_rule().parse_str(src).unwrap();
        assert!(errors.is_empty());
        match item {
            GrammarItem::Rule(GrammarRule::Invalid { name, .. }) => assert_eq!(name, "bad"),
            other => panic!("expected invalid rule, got {other:?}"),
        }
    }
}
