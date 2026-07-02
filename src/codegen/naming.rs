use std::collections::{HashMap, HashSet};

use crate::expr::{MatchingContext, SymKey};

const RUST_KEYWORDS: &[&str] = &[
    "abstract", "as", "async", "await", "become", "box", "break", "const", "continue", "crate",
    "do", "dyn", "else", "enum", "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop",
    "match", "mod", "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct",
    "super", "trait", "true", "type", "unsafe", "use", "where", "while", "yield",
];

pub fn bind_var_name(rule_name: &str) -> String {
    format!("{}_val", sanitize_ident(rule_name))
}

pub fn sanitize_ident(name: &str) -> String {
    if RUST_KEYWORDS.contains(&name) {
        format!("r#{name}")
    } else {
        name.to_string()
    }
}

pub(crate) fn contexts_by_rule(nodes: &HashSet<SymKey>) -> HashMap<String, HashSet<MatchingContext>> {
    let mut contexts = HashMap::new();
    for sym in nodes {
        contexts
            .entry(sym.rule.clone())
            .or_insert_with(HashSet::new)
            .insert(sym.context);
    }
    contexts
}

pub(crate) fn binding_name_for_graph(
    sym: &SymKey,
    contexts_by_rule: &HashMap<String, HashSet<MatchingContext>>,
) -> String {
    let base = sanitize_ident(&sym.rule);
    if contexts_by_rule
        .get(&sym.rule)
        .is_some_and(|contexts| contexts.len() > 1)
    {
        let suffix = match sym.context {
            MatchingContext::NormalWs => "__nw",
            MatchingContext::AtomicNoWs => "__anw",
        };
        format!("{base}{suffix}")
    } else {
        base
    }
}
