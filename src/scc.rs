use std::collections::{HashMap, HashSet};

use crate::error::{ConvertError, ConvertResult};
use crate::expr::SymKey;
use crate::specialize::SpecializationGraph;

#[derive(Clone, Debug)]
pub struct Scc {
    pub members: Vec<SymKey>,
    pub has_self_loop: bool,
}

pub fn tarjan_scc(graph: &SpecializationGraph) -> ConvertResult<Vec<Scc>> {
    let mut index = 0usize;
    let mut stack = Vec::new();
    let mut on_stack = HashSet::new();
    let mut indices: HashMap<SymKey, usize> = HashMap::new();
    let mut lowlink: HashMap<SymKey, usize> = HashMap::new();
    let mut sccs = Vec::new();

    for node in graph.nodes.iter() {
        if !indices.contains_key(node) {
            strongconnect(
                node,
                graph,
                &mut index,
                &mut stack,
                &mut on_stack,
                &mut indices,
                &mut lowlink,
                &mut sccs,
            );
        }
    }

    if sccs.iter().any(|scc| scc.members.len() > 12) {
        let size = sccs.iter().map(|scc| scc.members.len()).max().unwrap_or(0);
        return Err(vec![ConvertError::SccTooLarge { size }]);
    }

    Ok(sccs)
}

fn strongconnect(
    v: &SymKey,
    graph: &SpecializationGraph,
    index: &mut usize,
    stack: &mut Vec<SymKey>,
    on_stack: &mut HashSet<SymKey>,
    indices: &mut HashMap<SymKey, usize>,
    lowlink: &mut HashMap<SymKey, usize>,
    sccs: &mut Vec<Scc>,
) {
    indices.insert(v.clone(), *index);
    lowlink.insert(v.clone(), *index);
    *index += 1;
    stack.push(v.clone());
    on_stack.insert(v.clone());

    if let Some(neighbors) = graph.edges.get(v) {
        for w in neighbors {
            if !indices.contains_key(w) {
                strongconnect(w, graph, index, stack, on_stack, indices, lowlink, sccs);
                let w_low = lowlink[w];
                let v_low = lowlink.get_mut(v).unwrap();
                *v_low = (*v_low).min(w_low);
            } else if on_stack.contains(w) {
                let w_idx = indices[w];
                let v_low = lowlink.get_mut(v).unwrap();
                *v_low = (*v_low).min(w_idx);
            }
        }
    }

    if lowlink[v] == indices[v] {
        let mut members = Vec::new();
        loop {
            let w = stack.pop().unwrap();
            on_stack.remove(&w);
            members.push(w.clone());
            if w == *v {
                break;
            }
        }
        members.reverse();
        let has_self_loop = graph.edges.get(v).is_some_and(|deps| deps.contains(v));
        sccs.push(Scc {
            members,
            has_self_loop,
        });
    }
}

pub fn condensation_topo(sccs: &[Scc], graph: &SpecializationGraph) -> Vec<usize> {
    let mut sym_to_scc: HashMap<SymKey, usize> = HashMap::new();
    for (idx, scc) in sccs.iter().enumerate() {
        for member in &scc.members {
            sym_to_scc.insert(member.clone(), idx);
        }
    }

    let mut scc_edges: HashMap<usize, HashSet<usize>> = HashMap::new();
    for (from, deps) in &graph.edges {
        let from_idx = sym_to_scc[from];
        for dep in deps {
            let to_idx = sym_to_scc[dep];
            if from_idx != to_idx {
                scc_edges.entry(to_idx).or_default().insert(from_idx);
            }
        }
    }

    let mut indegree: HashMap<usize, usize> = HashMap::new();
    for idx in 0..sccs.len() {
        indegree.entry(idx).or_insert(0);
    }
    for deps in scc_edges.values() {
        for dep in deps {
            *indegree.entry(*dep).or_insert(0) += 1;
        }
    }

    let mut queue: Vec<usize> = indegree
        .iter()
        .filter_map(|(idx, deg)| if *deg == 0 { Some(*idx) } else { None })
        .collect();
    queue.sort_unstable();
    let mut order = Vec::new();

    while let Some(idx) = queue.pop() {
        order.push(idx);
        if let Some(deps) = scc_edges.get(&idx) {
            for dep in deps {
                let entry = indegree.get_mut(dep).unwrap();
                *entry -= 1;
                if *entry == 0 {
                    queue.push(*dep);
                }
            }
        }
    }

    order
}

pub fn is_cyclic(scc: &Scc) -> bool {
    scc.members.len() > 1 || scc.has_self_loop
}

/// Partition a cyclic SCC into feedback-vertex-set members (need `recursive` weak
/// handles) and non-FVS members (can be local `let` bindings inside the closure).
pub fn partition_scc_for_recursion(
    scc: &Scc,
    graph: &SpecializationGraph,
) -> (Vec<SymKey>, Vec<SymKey>) {
    let edges = internal_edges(scc, graph);
    let mut remaining: Vec<SymKey> = scc.members.clone();
    let mut fvs = Vec::new();
    let preferred = scc.members.iter().find(|m| **m == graph.entry);

    while induced_subgraph_has_cycle(&remaining, &edges) {
        let pick = pick_fvs_node(&remaining, &edges, &scc.members, preferred);
        fvs.push(pick.clone());
        remaining.retain(|m| m != &pick);
    }

    let non_fvs_topo = topo_sort_subset(&remaining, &edges);
    (fvs, non_fvs_topo)
}

/// Size of the feedback vertex set for a cyclic SCC (used for `recursiveN` arity).
pub fn recursive_arity(scc: &Scc, graph: &SpecializationGraph) -> usize {
    if !is_cyclic(scc) {
        return 0;
    }
    partition_scc_for_recursion(scc, graph).0.len()
}

fn internal_edges(
    scc: &Scc,
    graph: &SpecializationGraph,
) -> HashMap<SymKey, HashSet<SymKey>> {
    let members: HashSet<_> = scc.members.iter().cloned().collect();
    let mut edges = HashMap::new();
    for member in &scc.members {
        let deps: HashSet<_> = graph
            .edges
            .get(member)
            .into_iter()
            .flatten()
            .filter(|d| members.contains(d))
            .cloned()
            .collect();
        edges.insert(member.clone(), deps);
    }
    edges
}

fn induced_subgraph_has_cycle(
    nodes: &[SymKey],
    edges: &HashMap<SymKey, HashSet<SymKey>>,
) -> bool {
    if nodes.is_empty() {
        return false;
    }
    for node in nodes {
        if edges.get(node).is_some_and(|deps| deps.contains(node)) {
            return true;
        }
    }
    topo_sort_subset(nodes, edges).len() < nodes.len()
}

fn topo_sort_subset(
    nodes: &[SymKey],
    edges: &HashMap<SymKey, HashSet<SymKey>>,
) -> Vec<SymKey> {
    let set: HashSet<_> = nodes.iter().cloned().collect();
    let mut indegree: HashMap<SymKey, usize> = nodes.iter().map(|n| (n.clone(), 0)).collect();
    let mut dependents: HashMap<SymKey, Vec<SymKey>> = HashMap::new();
    for u in nodes {
        if let Some(deps) = edges.get(u) {
            for v in deps {
                if set.contains(v) {
                    dependents.entry(v.clone()).or_default().push(u.clone());
                    *indegree.get_mut(u).unwrap() += 1;
                }
            }
        }
    }

    let mut queue: Vec<SymKey> = nodes
        .iter()
        .filter(|n| indegree[*n] == 0)
        .cloned()
        .collect();
    let mut order = Vec::new();
    while let Some(u) = queue.pop() {
        order.push(u.clone());
        if let Some(waiters) = dependents.get(&u) {
            for w in waiters {
                let entry = indegree.get_mut(w).unwrap();
                *entry -= 1;
                if *entry == 0 {
                    queue.push(w.clone());
                }
            }
        }
    }
    order
}

fn pick_fvs_node(
    remaining: &[SymKey],
    edges: &HashMap<SymKey, HashSet<SymKey>>,
    member_order: &[SymKey],
    preferred: Option<&SymKey>,
) -> SymKey {
    let set: HashSet<_> = remaining.iter().cloned().collect();
    let mut best_score = 0usize;
    let mut best_preferred = false;
    let mut best_order_idx = usize::MAX;
    let mut best_sym = remaining[0].clone();

    for sym in remaining {
        let in_deg = remaining
            .iter()
            .filter(|u| edges.get(*u).is_some_and(|deps| deps.contains(sym)))
            .count();
        let out_deg = edges
            .get(sym)
            .map(|deps| deps.iter().filter(|d| set.contains(d)).count())
            .unwrap_or(0);
        let score = in_deg * out_deg;
        let is_preferred = preferred == Some(sym);
        let order_idx = member_order
            .iter()
            .position(|m| m == sym)
            .unwrap_or(usize::MAX);
        let better = score > best_score
            || (score == best_score && is_preferred && !best_preferred)
            || (score == best_score
                && is_preferred == best_preferred
                && order_idx < best_order_idx);
        if better {
            best_score = score;
            best_preferred = is_preferred;
            best_order_idx = order_idx;
            best_sym = sym.clone();
        }
    }
    best_sym
}

#[cfg(test)]
mod fvs_tests {
    use super::*;
    use crate::expr::MatchingContext;

    fn sym(rule: &str) -> SymKey {
        SymKey {
            rule: rule.to_string(),
            context: MatchingContext::NormalWs,
        }
    }

    fn cycle_scc(members: &[&str]) -> Scc {
        Scc {
            members: members.iter().map(|m| sym(m)).collect(),
            has_self_loop: false,
        }
    }

    fn cycle_graph(edges: &[(&str, &str)]) -> SpecializationGraph {
        let members: HashSet<SymKey> = edges
            .iter()
            .flat_map(|(a, b)| [sym(a), sym(b)])
            .collect();
        let mut graph_edges: HashMap<SymKey, HashSet<SymKey>> = HashMap::new();
        for member in &members {
            graph_edges.insert(member.clone(), HashSet::new());
        }
        for (from, to) in edges {
            graph_edges
                .get_mut(&sym(from))
                .unwrap()
                .insert(sym(to));
        }
        SpecializationGraph {
            nodes: members,
            edges: graph_edges,
            entry: sym("expr"),
            rule_map: HashMap::new(),
            warnings: vec![],
        }
    }

    #[test]
    fn non_fvs_topo_respects_dependencies() {
        let scc = cycle_scc(&["expr", "term", "factor"]);
        let mut graph = cycle_graph(&[
            ("expr", "term"),
            ("term", "factor"),
            ("factor", "expr"),
        ]);
        graph.entry = sym("expr");
        let (fvs, non_fvs) = partition_scc_for_recursion(&scc, &graph);
        assert_eq!(fvs.len(), 1);
        assert_eq!(fvs[0].rule, "expr");
        assert_eq!(non_fvs.len(), 2);
        assert_eq!(non_fvs[0].rule, "factor");
        assert_eq!(non_fvs[1].rule, "term");
        assert_eq!(recursive_arity(&scc, &graph), 1);
    }

    #[test]
    fn two_cycle_needs_one_fvs_member() {
        let scc = cycle_scc(&["a", "b"]);
        let graph = cycle_graph(&[("a", "b"), ("b", "a")]);
        let (fvs, non_fvs) = partition_scc_for_recursion(&scc, &graph);
        assert_eq!(fvs.len(), 1);
        assert_eq!(non_fvs.len(), 1);
    }

    #[test]
    fn self_loop_node_is_fvs() {
        let scc = Scc {
            members: vec![sym("a")],
            has_self_loop: true,
        };
        let graph = cycle_graph(&[("a", "a")]);
        let (fvs, non_fvs) = partition_scc_for_recursion(&scc, &graph);
        assert_eq!(fvs.len(), 1);
        assert!(non_fvs.is_empty());
    }
}
