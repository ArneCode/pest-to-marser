use std::collections::{HashSet, VecDeque};

use crate::ast::Modifier;
use crate::expr::MatchingContext;
use crate::normalize::{RuleDef, RuleTable};
use crate::specialize::{SpecializationGraph, collect_rule_deps};

/// Rules reachable only from the `WHITESPACE` / `COMMENT` closure and not from
/// content rules in the specialization graph.
pub fn compute_trivia_only_rules(
    table: &RuleTable,
    graph: &SpecializationGraph,
) -> HashSet<String> {
    if !table.has_whitespace && !table.has_comment {
        return HashSet::new();
    }

    let mut trivia = HashSet::new();
    let mut queue = VecDeque::new();
    if table.has_whitespace {
        queue.push_back("WHITESPACE".to_string());
    }
    if table.has_comment {
        queue.push_back("COMMENT".to_string());
    }

    while let Some(name) = queue.pop_front() {
        if !trivia.insert(name.clone()) {
            continue;
        }
        let Some(rule) = graph.rule_map.get(&name) else {
            continue;
        };
        let mut deps = HashSet::new();
        collect_rule_deps(
            &rule.expr,
            MatchingContext::AtomicNoWs,
            &graph.rule_map,
            &mut deps,
        );
        for dep in deps {
            queue.push_back(dep.rule);
        }
    }

    let content: HashSet<String> = graph.nodes.iter().map(|sym| sym.rule.clone()).collect();
    trivia.retain(|name| !content.contains(name));
    trivia
}

/// Rules emitted as plain matchers: trivia-only helpers and all silent (`_`) rules.
pub fn compute_matcher_only_rules(
    table: &RuleTable,
    graph: &SpecializationGraph,
) -> HashSet<String> {
    let mut rules = compute_trivia_only_rules(table, graph);
    for rule in &table.rules {
        if rule.modifier == Some(Modifier::Silent) {
            rules.insert(rule.name.clone());
        }
    }
    rules
}

pub fn is_silent_rule(rule: &RuleDef) -> bool {
    rule.modifier == Some(Modifier::Silent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::prepare_codegen;
    use crate::grammar::parse_pest_grammar;
    use crate::normalize::build_rule_table;
    use marser::parser::Parser;

    fn trivia_for(source: &str, entry: &str) -> HashSet<String> {
        let grammar = parse_pest_grammar().parse_str(source).unwrap().0;
        let table = build_rule_table(&grammar, crate::syntax::InputSyntax::Pest).unwrap();
        let (graph, _) = prepare_codegen(&table, entry).unwrap();
        compute_trivia_only_rules(&table, &graph)
    }

    #[test]
    fn simple_fixture_marks_trivia_helpers() {
        let src = include_str!("../tests/fixtures/simple.pest");
        let trivia = trivia_for(src, "main");
        for name in ["newline", "line_comment", "WHITESPACE", "COMMENT"] {
            assert!(trivia.contains(name), "expected {name} to be trivia-only");
        }
        for name in ["main", "item", "ident", "number"] {
            assert!(
                !trivia.contains(name),
                "expected {name} not to be trivia-only"
            );
        }
    }

    #[test]
    fn calc_fixture_marks_whitespace_only() {
        let src = include_str!("../tests/fixtures/calc.pest");
        let trivia = trivia_for(src, "expr");
        assert_eq!(trivia, HashSet::from(["WHITESPACE".to_string()]));
    }

    #[test]
    fn dual_use_rule_is_not_trivia_only() {
        let src = r#"
WHITESPACE = _{ " " | tab }
tab = _{ "\t" }
pair = @{ word ~ tab ~ word }
word = @{ ASCII_ALPHA+ }
main = { SOI ~ pair ~ EOI }
"#;
        let trivia = trivia_for(src, "main");
        assert!(trivia.contains("WHITESPACE"));
        assert!(!trivia.contains("tab"), "tab is also used from pair");
    }

    #[test]
    fn matcher_only_includes_all_silent_rules() {
        let src = r#"
WHITESPACE = _{ " " | tab }
tab = _{ "\t" }
pair = @{ word ~ tab ~ word }
word = @{ ASCII_ALPHA+ }
main = { SOI ~ pair ~ EOI }
"#;
        let grammar = parse_pest_grammar().parse_str(src).unwrap().0;
        let table = build_rule_table(&grammar, crate::syntax::InputSyntax::Pest).unwrap();
        let (graph, _) = prepare_codegen(&table, "main").unwrap();
        let matcher_only = compute_matcher_only_rules(&table, &graph);
        assert!(matcher_only.contains("WHITESPACE"));
        assert!(matcher_only.contains("tab"));
        assert!(!matcher_only.contains("word"));
    }
}
