use codescape_format::{ArchivedGraph, EdgeKind, NodeId, NodeKind};
use std::collections::VecDeque;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReachSet {
    bits: Vec<u64>,
    count: usize,
}

impl ReachSet {
    pub fn new(nodes: usize) -> ReachSet {
        ReachSet { bits: vec![0; nodes.div_ceil(64)], count: 0 }
    }

    pub fn insert(&mut self, n: NodeId) -> bool {
        let (w, b) = (n as usize / 64, n as usize % 64);
        if w >= self.bits.len() || self.bits[w] & (1 << b) != 0 {
            return false;
        }
        self.bits[w] |= 1 << b;
        self.count += 1;
        true
    }

    pub fn contains(&self, n: NodeId) -> bool {
        let (w, b) = (n as usize / 64, n as usize % 64);
        self.bits.get(w).is_some_and(|x| x & (1 << b) != 0)
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

pub fn compute_reach(g: &ArchivedGraph, entry: usize) -> ReachSet {
    let mut set = ReachSet::new(g.node_count());
    let mut q: VecDeque<NodeId> = VecDeque::new();
    q.push_back(g.entries()[entry].node.to_native());
    for i in 0..g.entries().len() {
        if g.entry_parent(i) == Some(entry) {
            q.push_back(g.entries()[i].node.to_native());
        }
    }
    let mut leaves: Vec<NodeId> = Vec::new();
    while let Some(n) = q.pop_front() {
        if !set.insert(n) {
            continue;
        }
        leaves.push(n);
        for e in g.out_edges(n) {
            if EdgeKind::from_u8(e.kind).is_call_like() {
                q.push_back(e.to.to_native());
            }
        }
        if g.kind(n) == NodeKind::Type {
            q.extend(g.children(n).filter(|&c| g.kind(c) == NodeKind::Member));
        }
    }
    for n in leaves {
        let mut cur = g.parent(n);
        while let Some(p) = cur {
            set.insert(p);
            cur = g.parent(p);
        }
    }
    set
}
