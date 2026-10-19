use codescape_format::{ArchivedGraph, EdgeFlags, EdgeKind, NodeFlags, NodeId, NodeKind, Scope, SubKind};
use std::collections::{HashMap, HashSet, VecDeque};

const LEVELS: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Call {
    pub to: NodeId,
    pub line: Option<u32>,
    pub create: bool,
}

pub struct CallIndex {
    out: Vec<Vec<Call>>,
    callers: Vec<Vec<NodeId>>,
    entries: HashSet<NodeId>,
    implementers: HashMap<NodeId, Vec<NodeId>>,
}

impl CallIndex {
    pub fn new(g: &ArchivedGraph) -> CallIndex {
        let count = g.node_count();
        let mut out: Vec<Vec<Call>> = vec![Vec::new(); count];
        let mut callers: Vec<Vec<NodeId>> = vec![Vec::new(); count];
        let mut implementers: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        for n in 0..count as NodeId {
            for e in g.out_edges(n) {
                let to = e.to.to_native();
                let kind = EdgeKind::from_u8(e.kind);
                if matches!(kind, EdgeKind::Implements | EdgeKind::Inherits) {
                    implementers.entry(to).or_default().push(n);
                    continue;
                }
                let main = Scope::from_u8(e.scope) == Scope::Main && !EdgeFlags(e.flags).contains(EdgeFlags::NOISE);
                if !kind.is_call_like() || !main || g.flags(to).contains(NodeFlags::EXTERNAL) || g.kind(to) == NodeKind::External {
                    continue;
                }
                out[n as usize].push(Call { to, line: g.call_line(n, e), create: kind == EdgeKind::Instantiates });
                if g.kind(to) == NodeKind::Member {
                    callers[to as usize].push(n);
                }
            }
        }
        for calls in &mut out {
            calls.sort_by_key(|c| (c.line.unwrap_or(u32::MAX), c.to));
            let mut seen = HashSet::new();
            calls.retain(|c| seen.insert(c.to));
        }
        for list in &mut callers {
            list.sort_by(|&a, &b| g.name(a).cmp(g.name(b)).then(a.cmp(&b)));
            list.dedup();
        }
        let entries = g.entries().iter().map(|e| e.node.to_native()).collect();
        CallIndex { out, callers, entries, implementers }
    }

    pub fn callers(&self, n: NodeId) -> &[NodeId] {
        &self.callers[n as usize]
    }

    pub fn calls(&self, n: NodeId) -> &[Call] {
        &self.out[n as usize]
    }

    fn line(&self, from: NodeId, to: NodeId) -> Option<u32> {
        self.out[from as usize].iter().find(|c| c.to == to).and_then(|c| c.line)
    }

    fn is_entry(&self, g: &ArchivedGraph, n: NodeId) -> bool {
        self.entries.contains(&n) || g.parent(n).is_some_and(|p| self.entries.contains(&p))
    }

    pub fn default_chain(&self, g: &ArchivedGraph, n: NodeId) -> Vec<NodeId> {
        let mut from: HashMap<NodeId, NodeId> = HashMap::new();
        let mut queue = VecDeque::from([n]);
        let mut seen = HashSet::from([n]);
        while let Some(cur) = queue.pop_front() {
            if cur != n && self.is_entry(g, cur) {
                let mut chain = vec![cur];
                let mut at = cur;
                while let Some(&next) = from.get(&at) {
                    chain.push(next);
                    at = next;
                }
                return chain;
            }
            for &c in self.callers(cur) {
                if seen.insert(c) {
                    from.insert(c, cur);
                    queue.push_back(c);
                }
            }
        }
        let mut chain = vec![n];
        while let [only] = self.callers(chain[0]) {
            if chain.contains(only) {
                break;
            }
            chain.insert(0, *only);
        }
        chain
    }

    pub fn one_level(&self, g: &ArchivedGraph, n: NodeId) -> Vec<NodeId> {
        let chain = self.default_chain(g, n);
        chain[chain.len().saturating_sub(2)..].to_vec()
    }

    pub fn methods(&self, g: &ArchivedGraph, owner: NodeId) -> Vec<NodeId> {
        let mut found: Vec<NodeId> = g
            .children(owner)
            .filter(|&c| g.kind(c) == NodeKind::Member && matches!(g.sub_kind(c), SubKind::Method | SubKind::Function | SubKind::Constructor))
            .collect();
        found.sort_by_key(|&m| (g.span(m).0, m));
        found
    }

    pub fn lead_method(&self, g: &ArchivedGraph, owner: NodeId) -> Option<NodeId> {
        let methods = self.methods(g, owner);
        let outside = |m: NodeId| self.callers(m).iter().filter(|&&c| g.parent(c) != Some(owner)).count();
        let rank = |m: NodeId| (self.entries.contains(&m), outside(m), self.calls(m).len());
        methods.iter().copied().rev().max_by_key(|&m| rank(m))
    }

    pub fn implementations(&self, g: &ArchivedGraph, member: NodeId) -> Vec<NodeId> {
        let Some(owner) = g.parent(member).filter(|&p| g.kind(p) == NodeKind::Type) else { return Vec::new() };
        let name = g.name(member);
        let mut out = Vec::new();
        let mut todo: Vec<NodeId> = self.implementers.get(&owner).cloned().unwrap_or_default();
        let mut seen = HashSet::new();
        while let Some(t) = todo.pop() {
            if !seen.insert(t) {
                continue;
            }
            out.extend(g.children(t).filter(|&c| g.name(c) == name && g.kind(c) == NodeKind::Member));
            todo.extend(self.implementers.get(&t).into_iter().flatten());
        }
        out.sort();
        out
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Lifeline {
    pub node: NodeId,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    pub from: usize,
    pub to: usize,
    pub caller: NodeId,
    pub callee: NodeId,
    pub target: NodeId,
    pub line: Option<u32>,
    pub depth: u32,
    pub focus: bool,
    pub more: bool,
    pub on_stack: bool,
    pub create: bool,
    pub implementations: Vec<NodeId>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Diagram {
    pub lifelines: Vec<Lifeline>,
    pub messages: Vec<Message>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Query {
    pub focus: NodeId,
    pub chain: Vec<NodeId>,
    pub expanded: HashSet<NodeId>,
    pub chosen: HashMap<NodeId, NodeId>,
}

impl Query {
    pub fn new(focus: NodeId, chain: Vec<NodeId>) -> Query {
        Query { focus, chain, expanded: HashSet::new(), chosen: HashMap::new() }
    }
}

struct Builder<'a> {
    g: &'a ArchivedGraph,
    calls: &'a CallIndex,
    q: &'a Query,
    d: Diagram,
    lanes: HashMap<NodeId, usize>,
    stack: Vec<NodeId>,
}

impl Builder<'_> {
    fn lane(&mut self, n: NodeId) -> usize {
        let owner = if self.g.kind(n) == NodeKind::Type { n } else { self.g.parent(n).unwrap_or(n) };
        if let Some(&i) = self.lanes.get(&owner) {
            return i;
        }
        self.d.lifelines.push(Lifeline { node: owner, label: self.g.name(owner).to_string() });
        self.lanes.insert(owner, self.d.lifelines.len() - 1);
        self.d.lifelines.len() - 1
    }

    fn walk(&mut self, n: NodeId, level: u32, base: u32) {
        let g = self.g;
        let calls = self.calls.calls(n).to_vec();
        let impls: Vec<Vec<NodeId>> = calls.iter().map(|c| self.calls.implementations(g, c.to)).collect();
        let implied: HashSet<NodeId> = impls.iter().flatten().copied().collect();
        for (call, implementations) in calls.into_iter().zip(impls) {
            if implied.contains(&call.to) {
                continue;
            }
            let interface = g.parent(call.to).is_some_and(|o| g.sub_kind(o) == SubKind::Interface);
            let auto = (interface && implementations.len() == 1).then(|| implementations[0]);
            let target = self.q.chosen.get(&call.to).copied().or(auto).unwrap_or(call.to);
            let on_stack = self.stack.contains(&target);
            let has_calls = !self.calls.calls(target).is_empty();
            let open = !call.create && !on_stack && has_calls && (level < LEVELS || self.q.expanded.contains(&target));
            let (from, to) = (self.lane(n), self.lane(target));
            self.d.messages.push(Message {
                from,
                to,
                caller: n,
                callee: call.to,
                target,
                line: call.line,
                depth: base + level - 1,
                focus: false,
                more: has_calls && !open && !on_stack && !call.create,
                on_stack,
                create: call.create,
                implementations,
            });
            if open {
                self.stack.push(target);
                self.walk(target, level + 1, base);
                self.stack.pop();
            }
        }
    }
}

pub fn diagram(g: &ArchivedGraph, calls: &CallIndex, q: &Query) -> Diagram {
    let mut b = Builder { g, calls, q, d: Diagram::default(), lanes: HashMap::new(), stack: q.chain.clone() };
    let chain = if q.chain.last() == Some(&q.focus) { q.chain.clone() } else { vec![q.focus] };
    b.lane(chain[0]);
    for (i, w) in chain.windows(2).enumerate() {
        let (from, to) = (b.lane(w[0]), b.lane(w[1]));
        b.d.messages.push(Message {
            from,
            to,
            caller: w[0],
            callee: w[1],
            target: w[1],
            line: calls.line(w[0], w[1]),
            depth: i as u32,
            focus: w[1] == q.focus,
            more: false,
            on_stack: false,
            create: false,
            implementations: Vec::new(),
        });
    }
    if !b.stack.contains(&q.focus) {
        b.stack.push(q.focus);
    }
    b.walk(q.focus, 1, chain.len() as u32 - 1);
    b.d
}
