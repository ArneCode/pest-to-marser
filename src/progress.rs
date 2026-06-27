use std::collections::HashMap;

use crate::ast::{PostfixOp, PrefixOp};
use crate::expr::{Builtin, Expr};
use crate::normalize::RuleDef;

pub fn is_non_failing(
    expr: &Expr,
    rules: &HashMap<String, &RuleDef>,
    trace: &mut Vec<String>,
) -> bool {
    match expr {
        Expr::Empty => true,
        Expr::Literal(s) | Expr::InsensitiveLiteral(s) => s.is_empty(),
        Expr::Builtin(_) => false,
        Expr::Range { .. } => false,
        Expr::RuleRef(name) => {
            if trace.contains(name) {
                return false;
            }
            if let Some(rule) = rules.get(name) {
                trace.push(name.clone());
                let result = is_non_failing(&rule.expr, rules, trace);
                trace.pop();
                result
            } else {
                false
            }
        }
        Expr::Sequence(items) => items.iter().all(|item| is_non_failing(item, rules, trace)),
        Expr::Choice(items) => items.iter().any(|item| is_non_failing(item, rules, trace)),
        Expr::Prefix { op, .. } => matches!(op, PrefixOp::NegativePredicate),
        Expr::Postfix { expr, op } => match op {
            PostfixOp::Optional | PostfixOp::Repeat | PostfixOp::RepeatMax(_) => true,
            PostfixOp::RepeatExact(min) | PostfixOp::RepeatMin(min) => {
                *min == 0 || is_non_failing(expr, rules, trace)
            }
            PostfixOp::RepeatMinMax(min, _) => *min == 0 || is_non_failing(expr, rules, trace),
            PostfixOp::RepeatOnce => is_non_failing(expr, rules, trace),
        },
    }
}

pub fn is_non_progressing(
    expr: &Expr,
    rules: &HashMap<String, &RuleDef>,
    trace: &mut Vec<String>,
) -> bool {
    match expr {
        Expr::Empty => true,
        Expr::Literal(s) | Expr::InsensitiveLiteral(s) => s.is_empty(),
        Expr::Builtin(b) => matches!(b, Builtin::Soi | Builtin::Eoi),
        Expr::Range { .. } => false,
        Expr::RuleRef(name) => {
            if trace.contains(name) {
                return false;
            }
            if let Some(rule) = rules.get(name) {
                trace.push(name.clone());
                let result = is_non_progressing(&rule.expr, rules, trace);
                trace.pop();
                result
            } else {
                false
            }
        }
        Expr::Sequence(items) => items
            .iter()
            .all(|item| is_non_progressing(item, rules, trace)),
        Expr::Choice(items) => items
            .iter()
            .any(|item| is_non_progressing(item, rules, trace)),
        Expr::Prefix { .. } => true,
        Expr::Postfix { expr, op } => match op {
            PostfixOp::Optional | PostfixOp::Repeat | PostfixOp::RepeatMax(_) => true,
            PostfixOp::RepeatExact(min) | PostfixOp::RepeatMin(min) => {
                *min == 0 || is_non_progressing(expr, rules, trace)
            }
            PostfixOp::RepeatMinMax(min, _) => *min == 0 || is_non_progressing(expr, rules, trace),
            PostfixOp::RepeatOnce => is_non_progressing(expr, rules, trace),
        },
    }
}
