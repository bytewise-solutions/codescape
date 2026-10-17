use crate::reach::ReachSet;
use codescape_format::{ArchivedGraph, EdgeFlags, EdgeKind, NodeId};
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusMode {
    Neighbours,
    Reaches,
    UsedBy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    None,
    Focus,
    Down,
    Up,
}

pub struct UsageIndex {
    incoming: Vec<Vec<NodeId>>,
}

fn followed(e: &codescape_format::ArchivedEdge) -> bool {
    !EdgeFlags(e.flags).contains(EdgeFlags::NOISE) && !matches!(EdgeKind::from_u8(e.kind), EdgeKind::DependsOn)
}

impl UsageIndex {
    pub fn used_from_outside(&self, g: &ArchivedGraph, member: NodeId) -> bool {
        let owner = g.parent(member).unwrap_or(member);
        self.incoming[member as usize].iter().any(|&s| !g.is_ancestor(owner, s))
    }

    pub fn new(g: &ArchivedGraph) -> UsageIndex {
        let mut incoming = vec![Vec::new(); g.node_count()];
        for n in 0..g.node_count() as NodeId {
            for e in g.out_edges(n).iter().filter(|e| followed(e)) {
                incoming[e.to.to_native() as usize].push(n);
            }
        }
        UsageIndex { incoming }
    }
}

#[derive(Clone, Debug)]
pub struct Marks {
    focus: ReachSet,
    down: ReachSet,
    up: ReachSet,
}

impl Marks {
    pub fn mark(&self, n: NodeId) -> Mark {
        if self.focus.contains(n) {
            Mark::Focus
        } else if self.down.contains(n) {
            Mark::Down
        } else if self.up.contains(n) {
            Mark::Up
        } else {
            Mark::None
        }
    }
}

#[derive(Clone, Debug)]
pub struct Focus {
    pub node: NodeId,
    pub entry: Option<usize>,
    pub mode: FocusMode,
    pub calls: Vec<NodeId>,
    pub callers: Vec<NodeId>,
    pub marks: Marks,
}

fn subtree(g: &ArchivedGraph, node: NodeId) -> Vec<NodeId> {
    let mut out = vec![node];
    let mut i = 0;
    while i < out.len() {
        out.extend(g.children(out[i]));
        i += 1;
    }
    out
}

fn with_ancestors(g: &ArchivedGraph, set: &mut ReachSet, n: NodeId) {
    let mut cur = Some(n);
    while let Some(c) = cur {
        if !set.insert(c) {
            break;
        }
        cur = g.parent(c);
    }
}

fn transitive(g: &ArchivedGraph, starts: &[NodeId], next: &dyn Fn(NodeId) -> Vec<NodeId>) -> Vec<NodeId> {
    let mut seen = ReachSet::new(g.node_count());
    let mut out = Vec::new();
    let mut q: VecDeque<NodeId> = starts.iter().copied().collect();
    while let Some(n) = q.pop_front() {
        for m in next(n) {
            if seen.insert(m) {
                out.push(m);
                q.push_back(m);
            }
        }
    }
    out
}

pub fn focus(g: &ArchivedGraph, idx: &UsageIndex, node: NodeId, entry: Option<usize>, mode: FocusMode) -> Focus {
    let inside = subtree(g, node);
    let outside = |n: NodeId| !g.is_ancestor(node, n);
    let mut calls: Vec<NodeId> = Vec::new();
    let mut callers: Vec<NodeId> = Vec::new();
    for &n in &inside {
        for e in g.out_edges(n).iter().filter(|e| followed(e)) {
            let t = e.to.to_native();
            if outside(t) && !calls.contains(&t) {
                calls.push(t);
            }
        }
        for &s in &idx.incoming[n as usize] {
            if outside(s) && !callers.contains(&s) {
                callers.push(s);
            }
        }
    }
    if let Some(parent) = entry.and_then(|e| g.entry_parent(e)) {
        let main = g.entries()[parent].node.to_native();
        if outside(main) && !callers.contains(&main) {
            callers.push(main);
        }
    }
    let n = g.node_count();
    let mut marks = Marks { focus: ReachSet::new(n), down: ReachSet::new(n), up: ReachSet::new(n) };
    with_ancestors(g, &mut marks.focus, node);
    let (down, up) = match mode {
        FocusMode::Neighbours => (calls.clone(), callers.clone()),
        FocusMode::Reaches => {
            let next = |m: NodeId| g.out_edges(m).iter().filter(|e| followed(e)).map(|e| e.to.to_native()).filter(|&t| outside(t)).collect();
            let mut found = transitive(g, &inside, &next);
            found.extend(calls.iter().copied());
            (found, Vec::new())
        }
        FocusMode::UsedBy => {
            let next = |m: NodeId| idx.incoming[m as usize].iter().copied().filter(|&s| outside(s)).collect();
            let mut found = transitive(g, &inside, &next);
            found.extend(callers.iter().copied());
            (Vec::new(), found)
        }
    };
    for t in down {
        with_ancestors(g, &mut marks.down, t);
    }
    for t in up {
        with_ancestors(g, &mut marks.up, t);
    }
    Focus { node, entry, mode, calls, callers, marks }
}

#[derive(Clone, Debug, Default)]
pub struct History {
    trail: Vec<NodeId>,
}

impl History {
    pub fn push(&mut self, n: NodeId) {
        if self.trail.last() != Some(&n) {
            self.trail.push(n);
        }
    }

    pub fn back(&mut self) -> Option<NodeId> {
        if self.trail.len() < 2 {
            self.trail.clear();
            return None;
        }
        self.trail.pop();
        self.trail.last().copied()
    }
}
