use std::collections::{HashMap, HashSet, VecDeque};

use crate::ast::Modifier;
use crate::error::{ConvertError, ConvertResult};
use crate::expr::{Expr, MatchingContext, SymKey};
use crate::normalize::RuleDef;
use crate::validate::forced_context;

#[derive(Clone, Debug)]
pub struct SpecializationGraph {
    pub nodes: HashSet<SymKey>,
    pub edges: HashMap<SymKey, HashSet<SymKey>>,
    pub entry: SymKey,
    pub rule_map: HashMap<String, RuleDef>,
    pub warnings: Vec<String>,
}

pub fn build_specialization_graph(
    rules: &[RuleDef],
    entry_rule: &str,
    _has_whitespace: bool,
    _has_comment: bool,
) -> ConvertResult<SpecializationGraph> {
    let rule_map: HashMap<_, _> = rules.iter().map(|r| (r.name.clone(), r.clone())).collect();
    let entry_rule_def = rule_map.get(entry_rule).ok_or_else(|| {
        vec![ConvertError::UnknownEntryRule {
            name: entry_rule.to_string(),
        }]
    })?;

    let entry_context =
        forced_context(entry_rule_def.modifier.as_ref()).unwrap_or(MatchingContext::NormalWs);
    let entry = SymKey {
        rule: entry_rule.to_string(),
        context: entry_context,
    };

    let mut nodes = HashSet::new();
    let mut edges: HashMap<SymKey, HashSet<SymKey>> = HashMap::new();
    let mut warnings = Vec::new();
    let mut queue = VecDeque::from([entry.clone()]);

    while let Some(sym) = queue.pop_front() {
        if !nodes.insert(sym.clone()) {
            continue;
        }

        let rule = rule_map.get(&sym.rule).expect("reachable rule must exist");
        let effective_context = sym.context;

        let mut deps = HashSet::new();
        collect_deps(
            &rule.expr,
            effective_context,
            &rule_map,
            &mut deps,
            &mut warnings,
        );
        edges.insert(sym.clone(), deps.clone());

        for dep in deps {
            if !nodes.contains(&dep) {
                queue.push_back(dep);
            }
        }
    }

    Ok(SpecializationGraph {
        nodes,
        edges,
        entry,
        rule_map,
        warnings,
    })
}

pub(crate) fn collect_rule_deps(
    expr: &Expr,
    caller_context: MatchingContext,
    rules: &HashMap<String, RuleDef>,
    deps: &mut HashSet<SymKey>,
) {
    let mut warnings = Vec::new();
    collect_deps(expr, caller_context, rules, deps, &mut warnings);
}

fn collect_deps(
    expr: &Expr,
    caller_context: MatchingContext,
    rules: &HashMap<String, RuleDef>,
    deps: &mut HashSet<SymKey>,
    warnings: &mut Vec<String>,
) {
    match expr {
        Expr::RuleRef(name) => {
            if let Some(rule) = rules.get(name) {
                let context = callee_context(caller_context, rule.modifier.as_ref());
                let key = SymKey {
                    rule: name.clone(),
                    context,
                };
                if forced_context(rule.modifier.as_ref()).is_none() && context != caller_context {
                    warnings.push(format!(
                        "rule {name} specialized for both normal and atomic contexts"
                    ));
                }
                deps.insert(key);
            }
        }
        Expr::Sequence(items) | Expr::Choice(items) => {
            for item in items {
                collect_deps(item, caller_context, rules, deps, warnings);
            }
        }
        Expr::Prefix { expr, .. } | Expr::Postfix { expr, .. } => {
            collect_deps(expr, caller_context, rules, deps, warnings);
        }
        _ => {}
    }
}

pub fn callee_context(
    caller_context: MatchingContext,
    modifier: Option<&Modifier>,
) -> MatchingContext {
    match forced_context(modifier) {
        Some(ctx) => ctx,
        None => match caller_context {
            MatchingContext::AtomicNoWs => MatchingContext::AtomicNoWs,
            MatchingContext::NormalWs => MatchingContext::NormalWs,
        },
    }
}

pub fn expr_for_sym(graph: &SpecializationGraph, sym: &SymKey) -> Expr {
    graph
        .rule_map
        .get(&sym.rule)
        .map(|rule| rule.expr.clone())
        .unwrap_or(Expr::Empty)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Modifier;
    use crate::expr::Expr;

    fn mk_rule(name: &str, modifier: Option<Modifier>, expr: Expr) -> RuleDef {
        RuleDef {
            name: name.to_string(),
            modifier,
            expr,
            docs: vec![],
        }
    }

    #[test]
    fn atomic_cascades_to_callees() {
        let rules = vec![
            mk_rule(
                "entry",
                Some(Modifier::Atomic),
                Expr::RuleRef("inner".to_string()),
            ),
            mk_rule("inner", None, Expr::Literal("x".to_string())),
        ];
        let graph = build_specialization_graph(&rules, "entry", false, false).unwrap();
        assert!(graph.nodes.contains(&SymKey {
            rule: "inner".to_string(),
            context: MatchingContext::AtomicNoWs,
        }));
    }
}
