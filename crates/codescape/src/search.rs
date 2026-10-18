use codescape_format::{ArchivedGraph, NodeFlags, NodeId, NodeKind};
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use std::cell::RefCell;

const EXACT_BONUS: u32 = 1 << 20;

pub struct SearchIndex {
    nodes: Vec<NodeId>,
    matcher: RefCell<Matcher>,
}

impl SearchIndex {
    pub fn new(g: &ArchivedGraph) -> SearchIndex {
        let nodes = (0..g.node_count() as NodeId)
            .filter(|&n| matches!(g.kind(n), NodeKind::Module | NodeKind::Package | NodeKind::Type | NodeKind::Member))
            .filter(|&n| !g.flags(n).intersects(NodeFlags::EXTERNAL | NodeFlags::MOCK))
            .collect();
        SearchIndex { nodes, matcher: RefCell::new(Matcher::new(Config::DEFAULT)) }
    }

    pub fn query(&self, g: &ArchivedGraph, q: &str, limit: usize) -> Vec<NodeId> {
        let q = q.trim();
        if q.is_empty() {
            return Vec::new();
        }
        let pattern = Pattern::parse(q, CaseMatching::Ignore, Normalization::Smart);
        let mut matcher = self.matcher.borrow_mut();
        let mut buf = Vec::new();
        let mut hits: Vec<(u32, usize, NodeId)> = self
            .nodes
            .iter()
            .filter_map(|&n| {
                let name = g.name(n);
                let score = pattern.score(Utf32Str::new(name, &mut buf), &mut matcher)?;
                let bonus = if name.eq_ignore_ascii_case(q) { EXACT_BONUS } else { 0 };
                Some((score + bonus, name.len(), n))
            })
            .collect();
        hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
        hits.truncate(limit);
        hits.into_iter().map(|(_, _, n)| n).collect()
    }
}

pub fn step_selection(current: usize, len: usize, down: bool) -> usize {
    match (len, down) {
        (0, _) => 0,
        (_, true) => (current + 1).min(len - 1),
        (_, false) => current.min(len - 1).saturating_sub(1),
    }
}

pub fn file_structure(g: &ArchivedGraph, owner: NodeId) -> Vec<NodeId> {
    let file = g.file(owner);
    let mut found = Vec::new();
    let mut stack: Vec<NodeId> = g.children(owner).collect();
    while let Some(n) = stack.pop() {
        if g.file(n) == file && matches!(g.kind(n), NodeKind::Type | NodeKind::Member) {
            found.push(n);
        }
        stack.extend(g.children(n));
    }
    found.sort_by_key(|&n| (g.span(n).0, n));
    found
}

pub fn filter_structure(g: &ArchivedGraph, nodes: &[NodeId], q: &str) -> Vec<NodeId> {
    let q = q.trim();
    if q.is_empty() {
        return nodes.to_vec();
    }
    let pattern = Pattern::parse(q, CaseMatching::Ignore, Normalization::Smart);
    let mut matcher = Matcher::new(Config::DEFAULT);
    let mut buf = Vec::new();
    let mut hits: Vec<(u32, usize, NodeId)> =
        nodes.iter().enumerate().filter_map(|(i, &n)| Some((pattern.score(Utf32Str::new(g.name(n), &mut buf), &mut matcher)?, i, n))).collect();
    hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    hits.into_iter().map(|(_, _, n)| n).collect()
}
