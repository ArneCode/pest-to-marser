use crate::ast::{
    Expression, Grammar, GrammarItem, GrammarRule, InfixOp, Node, PostfixOp, PrefixOp, Term,
    Terminal,
};
use crate::error::{ConvertError, ConvertResult};
use crate::expr::{Builtin, Expr};

#[derive(Clone, Debug)]
pub struct RuleDef {
    pub name: String,
    pub modifier: Option<crate::ast::Modifier>,
    pub expr: Expr,
    pub docs: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct RuleTable {
    pub rules: Vec<RuleDef>,
    pub grammar_docs: Vec<String>,
    pub has_whitespace: bool,
    pub has_comment: bool,
}

pub fn build_rule_table(grammar: &Grammar) -> ConvertResult<RuleTable> {
    let mut errors = Vec::new();
    let mut grammar_docs = Vec::new();
    let mut rules = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut has_whitespace = false;
    let mut has_comment = false;

    for item in &grammar.items {
        match item {
            GrammarItem::Doc(doc) => grammar_docs.push(doc.clone()),
            GrammarItem::LineDoc(_) => {}
            GrammarItem::Rule(rule) => {
                if !seen.insert(rule.name.clone()) {
                    errors.push(ConvertError::DuplicateRule {
                        name: rule.name.clone(),
                    });
                    continue;
                }
                if rule.name == "WHITESPACE" {
                    has_whitespace = true;
                }
                if rule.name == "COMMENT" {
                    has_comment = true;
                }
                match normalize_expression(&rule.expression) {
                    Ok(expr) => rules.push(RuleDef {
                        name: rule.name.clone(),
                        modifier: rule.modifier.clone(),
                        expr,
                        docs: Vec::new(),
                    }),
                    Err(mut normalize_errors) => errors.append(&mut normalize_errors),
                }
            }
        }
    }

    // Attach line docs to the following rule.
    let mut pending_docs = Vec::new();
    let mut rules_with_docs = Vec::new();
    for item in &grammar.items {
        match item {
            GrammarItem::LineDoc(doc) => pending_docs.push(doc.clone()),
            GrammarItem::Rule(rule) => {
                if let Some(def) = rules.iter().find(|r| r.name == rule.name) {
                    let mut def = def.clone();
                    def.docs = std::mem::take(&mut pending_docs);
                    rules_with_docs.push(def);
                }
            }
            GrammarItem::Doc(_) => {}
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    let defined: std::collections::HashSet<_> =
        rules_with_docs.iter().map(|r| r.name.clone()).collect();
    let mut resolved_rules = rules_with_docs;
    for rule in &mut resolved_rules {
        resolve_builtins(&mut rule.expr, &defined);
    }

    Ok(RuleTable {
        rules: resolved_rules,
        grammar_docs,
        has_whitespace,
        has_comment,
    })
}

pub fn normalize_expression(expr: &Expression) -> ConvertResult<Expr> {
    let mut errors = Vec::new();
    let mut normalized_terms = Vec::new();

    for term in &expr.terms {
        match normalize_term(term) {
            Ok(term_expr) => normalized_terms.push(term_expr),
            Err(mut term_errors) => errors.append(&mut term_errors),
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    let folded = if normalized_terms.len() == 1 {
        normalized_terms.into_iter().next().unwrap()
    } else {
        fold_infix(normalized_terms, &expr.infix_ops)
    };

    let result = if expr.leading_choice {
        match folded {
            Expr::Choice(mut items) => {
                items.insert(0, Expr::Empty);
                Expr::Choice(items)
            }
            other => Expr::Choice(vec![Expr::Empty, other]),
        }
    } else {
        folded
    };

    if let Some(feature) = result.has_unsupported() {
        return Err(vec![ConvertError::UnsupportedFeature {
            feature: feature.to_string(),
            detail: "PUSH/POP/DROP/PEEK are not supported in v1".to_string(),
        }]);
    }

    Ok(result)
}

fn fold_infix(terms: Vec<Expr>, ops: &[InfixOp]) -> Expr {
    assert_eq!(terms.len(), ops.len() + 1);

    let mut choices = Vec::new();
    let mut seq = vec![terms[0].clone()];

    for (term, op) in terms[1..].iter().zip(ops.iter()) {
        match op {
            InfixOp::Sequence => seq.push(term.clone()),
            InfixOp::Choice => {
                choices.push(finish_sequence(seq));
                seq = vec![term.clone()];
            }
        }
    }
    choices.push(finish_sequence(seq));

    if choices.len() == 1 {
        choices.pop().unwrap()
    } else {
        Expr::Choice(choices)
    }
}

fn finish_sequence(seq: Vec<Expr>) -> Expr {
    if seq.len() == 1 {
        seq.into_iter().next().unwrap()
    } else {
        Expr::Sequence(seq)
    }
}

fn normalize_term(term: &Term) -> ConvertResult<Expr> {
    let mut errors = Vec::new();
    let mut expr = match normalize_node(&term.node) {
        Ok(expr) => expr,
        Err(mut node_errors) => {
            errors.append(&mut node_errors);
            return Err(errors);
        }
    };

    for op in term.postfix_ops.iter().rev() {
        expr = Expr::Postfix {
            expr: Box::new(expr),
            op: op.clone(),
        };
    }

    for op in term.prefix_ops.iter().rev() {
        expr = Expr::Prefix {
            op: op.clone(),
            expr: Box::new(expr),
        };
    }

    if let Some(feature) = expr.has_unsupported() {
        errors.push(ConvertError::UnsupportedFeature {
            feature: feature.to_string(),
            detail: "PUSH/POP/DROP/PEEK are not supported in v1".to_string(),
        });
    }

    if errors.is_empty() {
        Ok(expr)
    } else {
        Err(errors)
    }
}

fn normalize_node(node: &Node) -> ConvertResult<Expr> {
    match node {
        Node::Grouped(inner) => normalize_expression(inner),
        Node::Terminal(terminal) => normalize_terminal(terminal),
    }
}

fn normalize_terminal(terminal: &Terminal) -> ConvertResult<Expr> {
    match terminal {
        Terminal::Identifier(name) => {
            if matches!(
                name.as_str(),
                "PUSH" | "POP" | "POP_ALL" | "DROP" | "PEEK" | "PEEK_ALL"
            ) {
                Err(vec![ConvertError::UnsupportedFeature {
                    feature: "stack construct".to_string(),
                    detail: format!("{name} is not supported in v1"),
                }])
            } else {
                Ok(Expr::RuleRef(name.clone()))
            }
        }
        Terminal::String(lit) => Ok(Expr::Literal(lit.clone())),
        Terminal::InsensitiveString(lit) => Ok(Expr::InsensitiveLiteral(lit.clone())),
        Terminal::Range { start, end } => Ok(Expr::Range {
            start: *start,
            end: *end,
        }),
        Terminal::Push(_) | Terminal::PushLiteral(_) | Terminal::PeekSlice { .. } => {
            Err(vec![ConvertError::UnsupportedFeature {
                feature: "stack construct".to_string(),
                detail: "PUSH/POP/DROP/PEEK are not supported in v1".to_string(),
            }])
        }
    }
}

fn resolve_builtins(expr: &mut Expr, defined: &std::collections::HashSet<String>) {
    match expr {
        Expr::RuleRef(name) => {
            if !defined.contains(name) {
                if let Some(builtin) = Builtin::from_name(name) {
                    *expr = Expr::Builtin(builtin);
                }
            }
        }
        Expr::Sequence(items) | Expr::Choice(items) => {
            for item in items {
                resolve_builtins(item, defined);
            }
        }
        Expr::Prefix { expr, .. } | Expr::Postfix { expr, .. } => {
            resolve_builtins(expr, defined);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{GrammarItem, Modifier};

    fn rule(name: &str, expr: Expression) -> Grammar {
        Grammar {
            items: vec![GrammarItem::Rule(GrammarRule {
                name: name.to_string(),
                modifier: None,
                expression: expr,
            })],
        }
    }

    #[test]
    fn leading_choice_becomes_empty_alternative() {
        let expr = Expression {
            leading_choice: true,
            terms: vec![Term {
                tag: None,
                prefix_ops: vec![],
                node: Node::Terminal(Terminal::String("a".to_string())),
                postfix_ops: vec![],
            }],
            infix_ops: vec![],
        };
        let table = build_rule_table(&rule("r", expr)).unwrap();
        assert_eq!(
            table.rules[0].expr,
            Expr::Choice(vec![Expr::Empty, Expr::Literal("a".to_string()),])
        );
    }

    #[test]
    fn sequence_and_choice_precedence() {
        let a = Term {
            tag: None,
            prefix_ops: vec![],
            node: Node::Terminal(Terminal::String("a".to_string())),
            postfix_ops: vec![],
        };
        let b = Term {
            tag: None,
            prefix_ops: vec![],
            node: Node::Terminal(Terminal::String("b".to_string())),
            postfix_ops: vec![],
        };
        let c = Term {
            tag: None,
            prefix_ops: vec![],
            node: Node::Terminal(Terminal::String("c".to_string())),
            postfix_ops: vec![],
        };
        let expr = Expression {
            leading_choice: false,
            terms: vec![a, b, c],
            infix_ops: vec![InfixOp::Sequence, InfixOp::Choice],
        };
        let table = build_rule_table(&rule("r", expr)).unwrap();
        assert_eq!(
            table.rules[0].expr,
            Expr::Choice(vec![
                Expr::Sequence(vec![
                    Expr::Literal("a".to_string()),
                    Expr::Literal("b".to_string()),
                ]),
                Expr::Literal("c".to_string()),
            ])
        );
    }

    #[test]
    fn prefix_and_postfix_wrap_term() {
        let expr = Expression {
            leading_choice: false,
            terms: vec![Term {
                tag: None,
                prefix_ops: vec![PrefixOp::PositivePredicate],
                node: Node::Terminal(Terminal::Identifier("x".to_string())),
                postfix_ops: vec![PostfixOp::RepeatOnce],
            }],
            infix_ops: vec![],
        };
        let table = build_rule_table(&rule("r", expr)).unwrap();
        assert_eq!(
            table.rules[0].expr,
            Expr::Prefix {
                op: PrefixOp::PositivePredicate,
                expr: Box::new(Expr::Postfix {
                    expr: Box::new(Expr::RuleRef("x".to_string())),
                    op: PostfixOp::RepeatOnce,
                }),
            }
        );
    }

    #[test]
    fn leading_choice_with_sequence() {
        let a = Term {
            tag: None,
            prefix_ops: vec![],
            node: Node::Terminal(Terminal::String("a".to_string())),
            postfix_ops: vec![],
        };
        let b = Term {
            tag: None,
            prefix_ops: vec![],
            node: Node::Terminal(Terminal::String("b".to_string())),
            postfix_ops: vec![],
        };
        let expr = Expression {
            leading_choice: true,
            terms: vec![a, b],
            infix_ops: vec![InfixOp::Sequence],
        };
        let table = build_rule_table(&rule("r", expr)).unwrap();
        assert_eq!(
            table.rules[0].expr,
            Expr::Choice(vec![
                Expr::Empty,
                Expr::Sequence(vec![
                    Expr::Literal("a".to_string()),
                    Expr::Literal("b".to_string()),
                ]),
            ])
        );
    }

    #[test]
    fn user_defined_rule_wins_over_builtin() {
        let grammar = Grammar {
            items: vec![
                GrammarItem::Rule(GrammarRule {
                    name: "ANY".to_string(),
                    modifier: None,
                    expression: Expression {
                        leading_choice: false,
                        terms: vec![Term {
                            tag: None,
                            prefix_ops: vec![],
                            node: Node::Terminal(Terminal::String("x".to_string())),
                            postfix_ops: vec![],
                        }],
                        infix_ops: vec![],
                    },
                }),
                GrammarItem::Rule(GrammarRule {
                    name: "main".to_string(),
                    modifier: None,
                    expression: Expression {
                        leading_choice: false,
                        terms: vec![Term {
                            tag: None,
                            prefix_ops: vec![],
                            node: Node::Terminal(Terminal::Identifier("ANY".to_string())),
                            postfix_ops: vec![],
                        }],
                        infix_ops: vec![],
                    },
                }),
            ],
        };
        let table = build_rule_table(&grammar).unwrap();
        let main = table.rules.iter().find(|r| r.name == "main").unwrap();
        assert_eq!(main.expr, Expr::RuleRef("ANY".to_string()));
    }

    #[test]
    fn duplicate_rules_are_rejected() {
        let grammar = Grammar {
            items: vec![
                GrammarItem::Rule(GrammarRule {
                    name: "a".to_string(),
                    modifier: None,
                    expression: Expression {
                        leading_choice: false,
                        terms: vec![Term {
                            tag: None,
                            prefix_ops: vec![],
                            node: Node::Terminal(Terminal::String("x".to_string())),
                            postfix_ops: vec![],
                        }],
                        infix_ops: vec![],
                    },
                }),
                GrammarItem::Rule(GrammarRule {
                    name: "a".to_string(),
                    modifier: Some(Modifier::Silent),
                    expression: Expression {
                        leading_choice: false,
                        terms: vec![Term {
                            tag: None,
                            prefix_ops: vec![],
                            node: Node::Terminal(Terminal::String("y".to_string())),
                            postfix_ops: vec![],
                        }],
                        infix_ops: vec![],
                    },
                }),
            ],
        };
        let err = build_rule_table(&grammar).unwrap_err();
        assert!(matches!(err[0], ConvertError::DuplicateRule { .. }));
    }
}
