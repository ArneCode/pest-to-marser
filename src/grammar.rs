use marser::capture;
use marser::{
    label::WithLabel,
    matcher::{
        AnyToken, Matcher, MatcherCombinator, commit_on, end_of_input, if_error::if_error, many,
        negative_lookahead, one_or_more, optional, start_of_input,
    },
    one_of::one_of,
    parser::{DeferredWeak, Parser, ParserCombinator, recursive},
    trace::WithTrace,
};

use crate::ast::*;

fn newline<'src, MRes>() -> impl Matcher<'src, &'src str, MRes> {
    one_of(("\n", "\r\n"))
}

fn block_comment<'src>() -> impl Parser<'src, &'src str, Output = ()> + Clone {
    recursive(|bc: DeferredWeak<'_, '_, &str, ()>| {
        capture!((
            commit_on(
                "/*",
                (
                    many(one_of((
                        bc.clone().ignore_result(),
                        (negative_lookahead("*/"), AnyToken),
                    ))),
                    "*/".try_insert_if_missing("missing block comment closing '*/'"),
                ),
            ),
        ) => ())
    })
    .erase_types()
}

fn line_comment<'src>() -> impl Parser<'src, &'src str, Output = ()> + Clone {
    capture!((
        "//",
        negative_lookahead(one_of(("/", "!"))),
        many((negative_lookahead(newline()), AnyToken)),
    ) => ())
}

fn comment<'src>() -> impl Parser<'src, &'src str, Output = ()> + Clone {
    one_of((block_comment(), line_comment()))
}

fn ws<'src, MRes>() -> impl Matcher<'src, &'src str, MRes> + Clone {
    many(one_of((
        one_of((" ", "\t")),
        newline(),
        comment().ignore_result(),
    )))
}

fn number<'src>() -> impl Parser<'src, &'src str, Output = u32> + Clone {
    capture!((
        bind_slice!(one_or_more('0'..='9'), digits as &'src str),
        ws(),
    ) => digits.parse().unwrap())
}

fn integer<'src>() -> impl Parser<'src, &'src str, Output = i64> + Clone {
    one_of((
        capture!((
            bind_slice!(one_or_more('0'..='9'), digits as &'src str),
            ws(),
        ) => digits.parse::<i64>().unwrap()),
        capture!((
            '-',
            bind_slice!(many('0'), zeros as &'src str),
            bind!('1'..='9', lead as char),
            optional(bind_slice!(one_or_more('0'..='9'), ?tail as &'src str)),
            ws(),
        ) => {
            let mut digits = String::from("-");
            digits.push_str(zeros);
            digits.push(lead);
            if let Some(tail) = tail {
                digits.push_str(tail);
            }
            digits.parse::<i64>().unwrap()
        }),
    ))
}

fn hex_chars<'src, MRes>() -> impl Matcher<'src, &'src str, MRes> {
    one_of(('0'..='9', 'a'..='f', 'A'..='F'))
}

fn decode_hex_byte(hex: &str) -> char {
    char::from_u32(u32::from_str_radix(hex, 16).unwrap()).unwrap()
}

fn decode_unicode(hex: &str) -> char {
    char::from_u32(u32::from_str_radix(hex, 16).unwrap()).unwrap()
}

fn escape<'src>() -> impl Parser<'src, &'src str, Output = char> + Clone {
    capture!((
        commit_on(
            '\\',
            bind!(
                one_of((
                    '"'.to('"'),
                    '\\'.to('\\'),
                    'r'.to('\r'),
                    'n'.to('\n'),
                    't'.to('\t'),
                    '0'.to('\0'),
                    '\''.to('\''),
                    capture!((
                        'x',
                        bind_slice!((
                            hex_chars(),
                            hex_chars(),
                        ), hex as &str),
                    ) => decode_hex_byte(hex)),
                    capture!((
                        'u',
                        '{',
                        bind_slice!(one_or_more(hex_chars()), hex as &str),
                        '}',
                    ) => decode_unicode(hex)),
                ))
                .with_label("escape sequence (n, t, r, 0, \\, ', \", \\xNN, \\u{..})"),
                escaped
            ),
        ),
    ) => escaped)
    .erase_types()
}

fn inner_str<'src>() -> impl Parser<'src, &'src str, Output = String> + Clone {
    recursive(|inner: DeferredWeak<'_, '_, &str, String>| {
        let tail = capture!((
            bind!(escape(), esc),
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

fn string<'src>() -> impl Parser<'src, &'src str, Output = String> + Clone {
    capture!((
        commit_on(
            '"',
            (
                bind!(inner_str(), content),
                '"'.try_insert_if_missing("missing closing '\"'"),
                ws(),
            ),
        ),
    ) => content)
}

fn any_char<'src>() -> impl Parser<'src, &'src str, Output = char> + Clone {
    capture!(bind_slice!((AnyToken,), s as &str) => s.chars().next().unwrap())
}

fn character<'src>() -> impl Parser<'src, &'src str, Output = char> + Clone {
    capture!((
        commit_on(
            '\'',
            (
                bind!(one_of((escape(), any_char())), ch),
                '\''.try_insert_if_missing("missing closing '\''"),
                ws(),
            ),
        ),
    ) => ch)
}

fn identifier<'src>() -> impl Parser<'src, &'src str, Output = String> + Clone {
    capture!((
        negative_lookahead("PUSH"),
        bind_slice!((
            one_of(('_', 'a'..='z', 'A'..='Z')),
            many(one_of(('_', 'a'..='z', 'A'..='Z', '0'..='9'))),
        ), id as &'src str),
        ws(),
    ) => id.to_string())
}

fn identifier_syntax<'src, MRes>() -> impl Matcher<'src, &'src str, MRes> + Clone {
    (
        one_of(('_', 'a'..='z', 'A'..='Z')),
        many(one_of(('_', 'a'..='z', 'A'..='Z', '0'..='9'))),
    )
}

fn tag_id<'src>() -> impl Parser<'src, &'src str, Output = String> + Clone {
    capture!((
        '#',
        bind_slice!((
            one_of(('_', 'a'..='z', 'A'..='Z')),
            many(one_of(('_', 'a'..='z', 'A'..='Z', '0'..='9'))),
        ), id as &'src str),
        ws(),
    ) => id.to_string())
}

fn node_tag<'src>() -> impl Parser<'src, &'src str, Output = String> + Clone {
    capture!((
        bind!(tag_id(), id),
        '=',
        ws(),
    ) => id)
}

fn parse_modifier<'src>() -> impl Parser<'src, &'src str, Output = Modifier> + Clone {
    one_of((
        '_'.map_output(|_| Modifier::Silent),
        '@'.map_output(|_| Modifier::Atomic),
        '$'.map_output(|_| Modifier::CompoundAtomic),
        '!'.map_output(|_| Modifier::NonAtomic),
    ))
}

fn prefix_op<'src>() -> impl Parser<'src, &'src str, Output = PrefixOp> + Clone {
    one_of((
        '&'.map_output(|_| PrefixOp::PositivePredicate),
        '!'.map_output(|_| PrefixOp::NegativePredicate),
    ))
}

fn infix_op<'src>() -> impl Parser<'src, &'src str, Output = InfixOp> + Clone {
    one_of((
        '~'.map_output(|_| InfixOp::Sequence),
        '|'.map_output(|_| InfixOp::Choice),
    ))
}

fn brace_repetition<'src>() -> impl Parser<'src, &'src str, Output = PostfixOp> + Clone {
    capture!((
        commit_on(
            '{',
            (
                ws(),
                bind!(
                    one_of((
                        capture!((
                            bind!(number(), min),
                            ws(),
                            ',',
                            ws(),
                            bind!(number(), max),
                        ) => PostfixOp::RepeatMinMax(min, max)),
                        capture!((
                            bind!(number(), n),
                            ws(),
                            ',',
                        ) => PostfixOp::RepeatMin(n)),
                        capture!((
                            ',',
                            ws(),
                            bind!(number(), n),
                        ) => PostfixOp::RepeatMax(n)),
                        capture!((
                            bind!(number(), n),
                        ) => PostfixOp::RepeatExact(n)),
                    ))
                    .with_label("repetition count"),
                    op
                ),
                ws(),
                '}'.try_insert_if_missing("missing '}'"),
                ws(),
            ),
        ),
    ) => op)
    .erase_types()
}

fn postfix_op<'src>() -> impl Parser<'src, &'src str, Output = PostfixOp> + Clone {
    one_of((
        '?'.map_output(|_| PostfixOp::Optional),
        '*'.map_output(|_| PostfixOp::Repeat),
        '+'.map_output(|_| PostfixOp::RepeatOnce),
        brace_repetition(),
    ))
    .erase_types()
}

fn inner_doc<'src>() -> impl Parser<'src, &'src str, Output = String> + Clone {
    capture!(bind_slice!(
        many((negative_lookahead(newline()), AnyToken)),
        doc as &'src str
    ) => doc.to_string())
}

fn grammar_doc<'src>() -> impl Parser<'src, &'src str, Output = GrammarItem> + Clone {
    capture!((
        "//!",
        optional(one_of((" ", "\t"))),
        bind!(inner_doc(), doc),
        ws(),
    ) => GrammarItem::Doc(doc))
}

fn line_doc<'src>() -> impl Parser<'src, &'src str, Output = GrammarItem> + Clone {
    capture!((
        "///",
        optional(one_of((" ", "\t"))),
        bind!(inner_doc(), doc),
        ws(),
    ) => GrammarItem::LineDoc(doc))
}

fn expression_grammar<'src>() -> impl Parser<'src, &'src str, Output = Expression> + Clone {
    recursive(|expr_weak: DeferredWeak<'_, '_, &str, Expression>| {
        let peek_slice = capture!((
            commit_on(
                ("PEEK", ws(), '[', ws()),
                (
                    optional(bind!(integer(), ?start)),
                    ws(),
                    "..",
                    ws(),
                    optional(bind!(integer(), ?end)),
                    ws(),
                    ']'.try_insert_if_missing("missing ']'"),
                    ws(),
                ),
            ),
        ) => Terminal::PeekSlice { start, end });

        let push_literal = capture!((
            commit_on(
                ("PUSH_LITERAL", ws(), '(', ws()),
                (
                    bind!(string(), lit),
                    ws(),
                    ')'.try_insert_if_missing("missing ')'"),
                    ws(),
                ),
            ),
        ) => Terminal::PushLiteral(lit));

        let push = capture!((
            commit_on(
                ("PUSH", ws(), '(', ws()),
                (
                    bind!(expr_weak.clone(), inner),
                    ws(),
                    ')'.try_insert_if_missing("missing ')'"),
                    ws(),
                ),
            ),
        ) => Terminal::Push(Box::new(inner)));

        let insensitive_string = capture!((
            '^',
            bind!(string(), lit),
        ) => Terminal::InsensitiveString(lit));

        let range = capture!((
            bind!(character(), start),
            "..",
            ws(),
            bind!(character(), end),
        ) => Terminal::Range { start, end });

        let terminal = one_of((
            push_literal,
            push,
            peek_slice,
            identifier().map_output(Terminal::Identifier),
            string().map_output(Terminal::String),
            insensitive_string,
            range,
        ))
        .map_output(Node::Terminal);

        let node = one_of((
            capture!((
                commit_on(
                    ('(', ws()),
                    (
                        bind!(expr_weak.clone(), inner),
                        ws(),
                        ')'.try_insert_if_missing("missing ')'"),
                        ws(),
                    ),
                ),
            ) => Node::Grouped(Box::new(inner))),
            terminal,
        ));

        let term = capture!((
            optional((bind!(node_tag(), ?tag), ws())),
            many((bind!(prefix_op(), *prefix_ops), ws())),
            bind!(node, n),
            many((bind!(postfix_op(), *postfix_ops), ws())),
        ) => Term {
            tag,
            prefix_ops,
            node: n,
            postfix_ops,
        });

        capture!((
            optional((bind!('|', ?leading_pipe), ws())),
            bind!(term.clone(), first),
            many((
                bind!(infix_op(), *ops),
                ws(),
                bind!(term, *terms),
            )),
        ) => {
            let mut all_terms = vec![first];
            all_terms.extend(terms);
            Expression {
                leading_choice: leading_pipe.is_some(),
                terms: all_terms,
                infix_ops: ops,
            }
        })
    })
    .erase_types()
}

fn next_grammar_item_start<'src, MRes>() -> impl Matcher<'src, &'src str, MRes> + Clone {
    let rule_start = (identifier_syntax(), ws(), '=');
    let item_start = one_of(("///", rule_start));

    one_of((
        end_of_input(),
        ('}', ws(), item_start.clone()),
        (newline(), ws(), item_start),
    ))
}

fn recover_grammar_rule<'src>() -> impl Parser<'src, &'src str, Output = GrammarItem> + Clone {
    capture!((
        bind!(identifier(), name),
        ws(),
        '=',
        ws(),
        bind_slice!(
            many((
                negative_lookahead(next_grammar_item_start()),
                AnyToken,
            )),
            text as &'src str
        ),
        optional(('}', ws())),
        ws(),
    ) => GrammarItem::Rule(GrammarRule::Invalid {
        name,
        text: text.to_string(),
    }))
    .trace_with_label("recover_grammar_rule")
    .erase_types()
}

fn unexpected_rule_body_tail<'src, MRes>() -> impl Matcher<'src, &'src str, MRes> + Clone
where
    MRes: marser::parser::capture::MatchResult,
{
    optional((
        one_or_more((
            negative_lookahead(one_of(('}', next_grammar_item_start()))),
            AnyToken,
        ))
        .unwanted("unexpected text after expression"),
        ws(),
    ))
}

fn grammar_rule<'src>() -> impl Parser<'src, &'src str, Output = GrammarItem> + Clone {
    let expression = expression_grammar();
    capture!((
        commit_on(
            (bind!(identifier(), name), ws(), '='),
            (
                ws(),
                optional((bind!(parse_modifier(), ?rule_mod), ws())),
                commit_on(
                    ('{', ws()),
                    (
                        bind!(expression.with_label("expression"), expr),
                        ws(),
                        if_error(unexpected_rule_body_tail()),
                        '}'.try_insert_if_missing("missing '}'"),
                        ws(),
                    ),
                ),
            ),
        ),
    ) => GrammarItem::Rule(GrammarRule::Valid {
        name,
        modifier: rule_mod,
        expression: expr,
    }))
    .trace_with_label("grammar_rule")
    .erase_types()
}

fn grammar_item<'src>() -> impl Parser<'src, &'src str, Output = GrammarItem> + Clone {
    one_of((grammar_rule(), line_doc()))
        .recover_with(recover_grammar_rule())
        .trace_with_label("grammar_item")
        .erase_types()
}

pub fn parse_pest_grammar<'src>() -> impl Parser<'src, &'src str, Output = Grammar> + Clone {
    capture!((
        start_of_input(),
        ws(),
        many(bind!(grammar_doc(), *docs)),
        many(bind!(grammar_item(), *rules)),
        ws(),
        end_of_input(),
    ) => {
        let mut items = docs;
        items.extend(rules);
        Grammar { items }
    })
    .erase_types()
}

#[cfg(test)]
mod tests {
    use super::*;
    use marser::parser::Parser;

    #[test]
    fn parses_simple_rule() {
        let src = r#"rule = { "hello" }"#;
        let grammar = parse_pest_grammar().parse_str(src).unwrap().0;
        assert_eq!(grammar.items.len(), 1);
        match &grammar.items[0] {
            GrammarItem::Rule(GrammarRule::Valid { name, modifier, .. }) => {
                assert_eq!(name, "rule");
                assert_eq!(*modifier, None);
            }
            other => panic!("expected rule, got {other:?}"),
        }
    }

    #[test]
    fn recovers_invalid_rule_and_parses_following_rule() {
        let src = r#"bad = broken
good = { "ok" }"#;
        let (grammar, errors) = parse_pest_grammar().parse_str(src).unwrap();
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
    fn recovers_invalid_braced_rule_and_parses_following_rule() {
        let src = r#"bad = { "a" ~ }
good = { "ok" }"#;
        let (grammar, errors) = parse_pest_grammar().parse_str(src).unwrap();
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
    fn recovery_rule_consumes_closing_brace_sync() {
        let src = r#"bad = { "a" ~ }"#;
        let (item, errors) = recover_grammar_rule().parse_str(src).unwrap();
        assert!(errors.is_empty());
        match item {
            GrammarItem::Rule(GrammarRule::Invalid { name, .. }) => assert_eq!(name, "bad"),
            other => panic!("expected invalid rule, got {other:?}"),
        }
    }

    #[cfg(feature = "parser-trace")]
    #[test]
    #[ignore = "debug helper for parser recovery traces"]
    fn trace_invalid_braced_rule_recovery() {
        use marser::trace::TraceFormat;

        let src = r#"bad = { "a" ~ }
good = { "ok" }"#;
        let path = std::env::temp_dir().join("grammar_to_marser_recovery_trace.json");
        let result =
            parse_pest_grammar().parse_str_with_trace_to_file(src, &path, TraceFormat::Json);
        eprintln!("trace: {}", path.display());
        eprintln!("result: {}", if result.is_ok() { "ok" } else { "err" });
        let _ = result;
    }

    #[test]
    fn recovers_multiple_invalid_braced_rules() {
        let src = r#"bad = { "a" ~ }
worse = { | }
good = { "ok" }"#;
        let (grammar, errors) = parse_pest_grammar().parse_str(src).unwrap();
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
    fn recovers_invalid_rule_before_line_doc() {
        let src = r#"bad = { "a" ~ }
/// parsed after recovery
good = { "ok" }"#;
        let (grammar, errors) = parse_pest_grammar().parse_str(src).unwrap();
        assert!(!errors.is_empty());
        assert_eq!(grammar.items.len(), 3);
        match &grammar.items[0] {
            GrammarItem::Rule(rule) => assert_eq!(rule.name(), "bad"),
            other => panic!("expected recovered rule, got {other:?}"),
        }
        match &grammar.items[1] {
            GrammarItem::LineDoc(doc) => assert_eq!(doc, "parsed after recovery"),
            other => panic!("expected line doc, got {other:?}"),
        }
        match &grammar.items[2] {
            GrammarItem::Rule(GrammarRule::Valid { name, .. }) => assert_eq!(name, "good"),
            other => panic!("expected valid rule, got {other:?}"),
        }
    }

    #[test]
    fn parses_pest_meta_grammar() {
        let src = include_str!("../tests/fixtures/grammar.pest");
        parse_pest_grammar().parse_str(src).unwrap();
    }

    #[test]
    fn integer_matches_pest_grammar() {
        use marser::matcher::end_of_input;

        let positive = capture!((bind!(integer(), n), end_of_input()) => n);
        assert_eq!(positive.parse_str("0").unwrap().0, 0);
        assert_eq!(positive.parse_str("123").unwrap().0, 123);
        assert_eq!(positive.parse_str("007").unwrap().0, 7);

        let negative = capture!((bind!(integer(), n), end_of_input()) => n);
        assert_eq!(negative.parse_str("-7").unwrap().0, -7);
        assert_eq!(negative.parse_str("-007").unwrap().0, -7);
        assert_eq!(negative.parse_str("-123").unwrap().0, -123);

        assert!(integer().parse_str("-0").is_err());
        assert!(integer().parse_str("-").is_err());
    }
}
