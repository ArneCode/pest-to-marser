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
