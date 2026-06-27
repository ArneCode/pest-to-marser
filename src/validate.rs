use std::collections::HashMap;

use crate::ast::{Modifier, PostfixOp};
use crate::error::{ConvertError, ConvertResult};
use crate::expr::{Builtin, Expr, MatchingContext};
use crate::normalize::RuleDef;
use crate::progress::{is_non_failing, is_non_progressing};

pub fn validate_repetitions(rules: &[RuleDef]) -> Vec<ConvertError> {
    let map: HashMap<_, _> = rules.iter().map(|r| (r.name.clone(), r)).collect();
    let mut errors = Vec::new();

    for rule in rules {
        collect_repetition_errors(&rule.expr, &rule.name, &map, &mut errors);
    }

    errors
}

fn collect_repetition_errors(
    expr: &Expr,
    rule_name: &str,
    rules: &HashMap<String, &RuleDef>,
    errors: &mut Vec<ConvertError>,
) {
    match expr {
        Expr::Postfix { expr, op } => {
            if is_unbounded_repetition(op) {
                if is_non_failing(expr, rules, &mut vec![]) {
                    errors.push(ConvertError::NonFailingRepetition {
                        rule: rule_name.to_string(),
                        detail:
                            "expression inside repetition cannot fail and will repeat infinitely"
                                .to_string(),
                    });
                } else if is_non_progressing(expr, rules, &mut vec![]) {
                    errors.push(ConvertError::NonProgressingRepetition {
                        rule: rule_name.to_string(),
                        detail:
                            "expression inside repetition is non-progressing and will repeat infinitely"
                                .to_string(),
                    });
                }
            }
            collect_repetition_errors(expr, rule_name, rules, errors);
        }
        Expr::Sequence(items) | Expr::Choice(items) => {
            for item in items {
                collect_repetition_errors(item, rule_name, rules, errors);
            }
        }
        Expr::Prefix { expr, .. } => collect_repetition_errors(expr, rule_name, rules, errors),
        _ => {}
    }
}

fn is_unbounded_repetition(op: &PostfixOp) -> bool {
    matches!(
        op,
        PostfixOp::Repeat | PostfixOp::RepeatOnce | PostfixOp::RepeatMin(_)
    )
}

pub fn validate_whitespace_comment(rules: &[RuleDef]) -> Vec<ConvertError> {
    let map: HashMap<_, _> = rules.iter().map(|r| (r.name.clone(), r)).collect();
    rules
        .iter()
        .filter(|rule| rule.name == "WHITESPACE" || rule.name == "COMMENT")
        .filter_map(|rule| {
            if is_non_failing(&rule.expr, &map, &mut vec![]) {
                Some(ConvertError::NonFailingWhitespace {
                    rule: rule.name.clone(),
                })
            } else if is_non_progressing(&rule.expr, &map, &mut vec![]) {
                Some(ConvertError::NonProgressingWhitespace {
                    rule: rule.name.clone(),
                })
            } else {
                None
            }
        })
        .collect()
}

pub fn validate_undefined_and_builtins(rules: &[RuleDef], entry_rule: &str) -> Vec<ConvertError> {
    let defined: std::collections::HashSet<_> = rules.iter().map(|r| r.name.as_str()).collect();
    let mut errors = Vec::new();

    if !defined.contains(entry_rule) {
        errors.push(ConvertError::UnknownEntryRule {
            name: entry_rule.to_string(),
        });
    }

    for rule in rules {
        for name in rule.expr.rule_refs() {
            if defined.contains(name) || Builtin::from_name(name).is_some() {
                continue;
            }
            if matches!(
                name,
                "PUSH" | "POP" | "POP_ALL" | "DROP" | "PEEK" | "PEEK_ALL"
            ) {
                errors.push(ConvertError::UnsupportedFeature {
                    feature: "stack construct".to_string(),
                    detail: format!("{name} is not supported in v1"),
                });
            } else if name.starts_with("ASCII_") || name == "ASCII" {
                errors.push(ConvertError::UnknownBuiltin {
                    name: name.to_string(),
                });
            } else {
                errors.push(ConvertError::UndefinedRule {
                    name: name.to_string(),
                });
            }
        }
    }

    errors
}

pub fn validate_left_recursion(rules: &[RuleDef]) -> Vec<ConvertError> {
    let map: HashMap<_, _> = rules.iter().map(|r| (r.name.clone(), r)).collect();
    let mut errors = Vec::new();

    for rule in rules {
        if let Some(chain) = find_left_recursion(&rule.expr, &map, &mut vec![rule.name.clone()]) {
            errors.push(ConvertError::LeftRecursion { chain });
        }
    }

    errors
}

fn find_left_recursion(
    expr: &Expr,
    rules: &HashMap<String, &RuleDef>,
    trace: &mut Vec<String>,
) -> Option<String> {
    match expr {
        Expr::RuleRef(name) => {
            if trace.first() == Some(name) {
                let mut chain = trace.clone();
                chain.push(name.clone());
                return Some(chain.join(" -> "));
            }
            if trace.contains(name) {
                return None;
            }
            if let Some(rule) = rules.get(name) {
                trace.push(name.clone());
                let result = find_left_recursion(&rule.expr, rules, trace);
                trace.pop();
                result
            } else {
                None
            }
        }
        Expr::Sequence(items) => {
            for (idx, item) in items.iter().enumerate() {
                if let Some(chain) = find_left_recursion(item, rules, trace) {
                    return Some(chain);
                }
                let start_rule = trace.last().cloned().unwrap_or_default();
                if !is_non_failing(item, rules, &mut vec![start_rule.clone()])
                    && !is_non_progressing(item, rules, &mut vec![start_rule.clone()])
                {
                    break;
                }
                if idx + 1 == items.len() {
                    break;
                }
            }
            None
        }
        Expr::Choice(items) => items
            .iter()
            .find_map(|item| find_left_recursion(item, rules, trace)),
        Expr::Postfix { expr, .. } | Expr::Prefix { expr, .. } => {
            find_left_recursion(expr, rules, trace)
        }
        _ => None,
    }
}

pub fn validate_all(rules: &[RuleDef], entry_rule: &str) -> ConvertResult<()> {
    let mut errors = Vec::new();
    errors.extend(validate_undefined_and_builtins(rules, entry_rule));
    errors.extend(validate_repetitions(rules));
    errors.extend(validate_whitespace_comment(rules));
    errors.extend(validate_left_recursion(rules));

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

pub fn forced_context(modifier: Option<&Modifier>) -> Option<MatchingContext> {
    match modifier {
        Some(Modifier::Atomic) | Some(Modifier::CompoundAtomic) => {
            Some(MatchingContext::AtomicNoWs)
        }
        Some(Modifier::NonAtomic) => Some(MatchingContext::NormalWs),
        Some(Modifier::Silent) | None => None,
    }
}
