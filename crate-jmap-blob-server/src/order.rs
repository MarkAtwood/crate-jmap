//! Ordering the entries of one `Blob/set` or `Blob/convert` create map so
//! that every `#creationId` reference is created before it is used
//! (RFC 8620 §5.3; blobext-03 §8).

use std::collections::{BTreeMap, BTreeSet};

/// The processing order of a create map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TopoOrder<K> {
    /// Every key not in a cycle, each after the keys it depends on.
    pub(crate) order: Vec<K>,
    /// Keys on a dependency cycle (including a key that refers to itself).
    /// blobext-03 §8: "If a dependency cycle is detected, all members of
    /// the cycle MUST be rejected with an "invalidProperties" error."
    pub(crate) cyclic: BTreeSet<K>,
}

/// Order `nodes` so that each comes after its dependencies.
///
/// `deps(k)` lists the keys `k` refers to; a key not in `nodes` (a
/// reference to an earlier call, or to nothing) is ignored here and left
/// for reference resolution to report. A key that depends on a cycle
/// without being on it stays in `order`, after nothing it can use: its
/// reference then fails to resolve (`notFound`).
///
/// Ties keep the order of `nodes`, so the result is deterministic.
pub(crate) fn topo_order<K, F>(nodes: &[K], deps: F) -> TopoOrder<K>
where
    K: Ord + Clone,
    F: Fn(&K) -> Vec<K>,
{
    // Tarjan's strongly connected components. With edges from a key to
    // its dependencies, components complete dependencies-first, which is
    // the processing order.
    let index_of: BTreeMap<&K, usize> = nodes.iter().enumerate().map(|(i, k)| (k, i)).collect();
    let edges: Vec<Vec<usize>> = nodes
        .iter()
        .map(|k| {
            deps(k)
                .iter()
                .filter_map(|d| index_of.get(d).copied())
                .collect()
        })
        .collect();

    let mut t = Tarjan {
        edges: &edges,
        index: vec![None; nodes.len()],
        low: vec![0; nodes.len()],
        on_stack: vec![false; nodes.len()],
        stack: Vec::new(),
        next: 0,
        components: Vec::new(),
    };
    for v in 0..nodes.len() {
        if t.index[v].is_none() {
            t.visit(v);
        }
    }

    let mut order = Vec::new();
    let mut cyclic = BTreeSet::new();
    for comp in t.components {
        let self_loop = comp.len() == 1 && edges[comp[0]].contains(&comp[0]);
        if comp.len() > 1 || self_loop {
            cyclic.extend(comp.iter().map(|&i| nodes[i].clone()));
        } else {
            order.push(nodes[comp[0]].clone());
        }
    }
    TopoOrder { order, cyclic }
}

struct Tarjan<'a> {
    edges: &'a [Vec<usize>],
    index: Vec<Option<usize>>,
    low: Vec<usize>,
    on_stack: Vec<bool>,
    stack: Vec<usize>,
    next: usize,
    components: Vec<Vec<usize>>,
}

impl Tarjan<'_> {
    // Recursion depth is bounded by the create map, which
    // maxObjectsInSet caps.
    fn visit(&mut self, v: usize) {
        self.index[v] = Some(self.next);
        self.low[v] = self.next;
        self.next += 1;
        self.stack.push(v);
        self.on_stack[v] = true;
        for &w in &self.edges[v] {
            match self.index[w] {
                None => {
                    self.visit(w);
                    self.low[v] = self.low[v].min(self.low[w]);
                }
                Some(iw) if self.on_stack[w] => self.low[v] = self.low[v].min(iw),
                Some(_) => {}
            }
        }
        if Some(self.low[v]) == self.index[v] {
            let mut comp = Vec::new();
            while let Some(w) = self.stack.pop() {
                self.on_stack[w] = false;
                comp.push(w);
                if w == v {
                    break;
                }
            }
            comp.reverse();
            self.components.push(comp);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `edges` lists (key, dependency) pairs.
    fn run(
        nodes: &[&'static str],
        edges: &[(&'static str, &'static str)],
    ) -> TopoOrder<&'static str> {
        topo_order(nodes, |k| {
            edges
                .iter()
                .filter(|(from, _)| from == k)
                .map(|(_, to)| *to)
                .collect()
        })
    }

    fn pos(o: &TopoOrder<&str>, k: &str) -> usize {
        o.order.iter().position(|x| *x == k).unwrap()
    }

    #[test]
    fn independent_keys_keep_their_order() {
        let o = run(&["a", "b", "c"], &[]);
        assert_eq!(o.order, ["a", "b", "c"]);
        assert!(o.cyclic.is_empty());
    }

    #[test]
    fn chain_in_reverse_map_order() {
        // a needs b, b needs c: map order a, b, c, processing c, b, a.
        let o = run(&["a", "b", "c"], &[("a", "b"), ("b", "c")]);
        assert_eq!(o.order, ["c", "b", "a"]);
    }

    #[test]
    fn diamond() {
        let o = run(
            &["top", "left", "right", "base"],
            &[
                ("top", "left"),
                ("top", "right"),
                ("left", "base"),
                ("right", "base"),
            ],
        );
        assert_eq!(o.order.len(), 4);
        assert!(pos(&o, "base") < pos(&o, "left"));
        assert!(pos(&o, "base") < pos(&o, "right"));
        assert!(pos(&o, "left") < pos(&o, "top"));
        assert!(pos(&o, "right") < pos(&o, "top"));
    }

    #[test]
    fn unknown_dependencies_are_ignored() {
        let o = run(&["a"], &[("a", "earlier-call")]);
        assert_eq!(o.order, ["a"]);
        assert!(o.cyclic.is_empty());
    }

    #[test]
    fn cycle_members_are_rejected_and_the_rest_ordered() {
        // x <-> y is a cycle; z depends on x but is not on the cycle;
        // w is independent.
        let o = run(&["w", "x", "y", "z"], &[("x", "y"), ("y", "x"), ("z", "x")]);
        assert_eq!(o.cyclic, BTreeSet::from(["x", "y"]));
        assert_eq!(o.order, ["w", "z"]);
    }

    #[test]
    fn three_cycle_and_self_reference() {
        let o = run(
            &["a", "b", "c", "s", "ok"],
            &[("a", "b"), ("b", "c"), ("c", "a"), ("s", "s")],
        );
        assert_eq!(o.cyclic, BTreeSet::from(["a", "b", "c", "s"]));
        assert_eq!(o.order, ["ok"]);
    }
}
