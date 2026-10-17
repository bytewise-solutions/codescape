use srcrelief_format::{ArchivedGraph, EdgeFlags, EdgeKind, NodeFlags, NodeId, NodeKind, Scope, SubKind, Visibility};
use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Usage {
    Used,
    OnlyTests,
    Unused,
    NoCallerInProject,
}

impl Usage {
    pub fn label(self) -> Option<&'static str> {
        match self {
            Usage::Used => None,
            Usage::OnlyTests => Some("only tests use it"),
            Usage::Unused => Some("unused"),
            Usage::NoCallerInProject => Some("no caller in the project"),
        }
    }
}

pub struct Analysis {
    pub usage: Vec<Usage>,
    pub tested: Vec<bool>,
    pub callers: Vec<u32>,
}

const SET_ASIDE: NodeFlags =
    NodeFlags(NodeFlags::EXTERNAL.0 | NodeFlags::TEST.0 | NodeFlags::MOCK.0 | NodeFlags::GENERATED.0 | NodeFlags::WIRING.0 | NodeFlags::NOISE.0);

fn test_code(g: &ArchivedGraph, n: NodeId) -> bool {
    g.path(n).into_iter().any(|p| g.flags(p).contains(NodeFlags::TEST))
}

fn set_aside(g: &ArchivedGraph, n: NodeId) -> bool {
    g.path(n).into_iter().any(|p| g.flags(p).intersects(SET_ASIDE))
}

fn is_constructor(g: &ArchivedGraph, n: NodeId) -> bool {
    g.sub_kind(n) == SubKind::Constructor || g.name(n).starts_with('<')
}

fn file_body(g: &ArchivedGraph, n: NodeId) -> bool {
    g.file(n).is_some_and(|f| f.rsplit('/').next() == Some(g.name(n)))
}

fn supertypes(g: &ArchivedGraph, t: NodeId) -> impl Iterator<Item = NodeId> + '_ {
    g.out_edges(t).iter().filter(|e| matches!(EdgeKind::from_u8(e.kind), EdgeKind::Implements | EdgeKind::Inherits)).map(|e| e.to.to_native())
}

fn overrides(g: &ArchivedGraph, member: NodeId) -> bool {
    let Some(owner) = g.parent(member).filter(|&p| g.kind(p) == NodeKind::Type) else { return false };
    let name = g.name(member);
    let mut seen = HashSet::from([owner]);
    let mut todo: Vec<NodeId> = supertypes(g, owner).collect();
    while let Some(s) = todo.pop() {
        if !seen.insert(s) {
            continue;
        }
        if g.flags(s).contains(NodeFlags::EXTERNAL) {
            if g.visibility(member) != Visibility::Private {
                return true;
            }
            continue;
        }
        if g.children(s).any(|c| g.name(c) == name) {
            return true;
        }
        todo.extend(supertypes(g, s));
    }
    false
}

pub fn analyse(g: &ArchivedGraph, root: &Path) -> Analysis {
    let count = g.node_count();
    let mut main_use = vec![false; count];
    let mut test_use = vec![false; count];
    let mut incoming: Vec<HashSet<NodeId>> = vec![HashSet::new(); count];
    let tests: Vec<bool> = (0..count as NodeId).map(|n| test_code(g, n)).collect();
    for src in 0..count as NodeId {
        for e in g.out_edges(src).iter().filter(|e| !EdgeFlags(e.flags).contains(EdgeFlags::NOISE)) {
            let to = e.to.to_native();
            let by_test = tests[src as usize] || Scope::from_u8(e.scope) == Scope::Test;
            if !by_test {
                incoming[to as usize].insert(src);
            }
            let mut cur = Some(to);
            while let Some(t) = cur.filter(|&t| !g.is_ancestor(t, src)) {
                if by_test {
                    test_use[t as usize] = true;
                } else {
                    main_use[t as usize] = true;
                }
                cur = g.parent(t);
            }
        }
    }
    let mut entry = vec![false; count];
    for ep in g.entries() {
        entry[ep.node.to_native() as usize] = true;
    }
    let entered = |n: NodeId| g.path(n).into_iter().any(|p| entry[p as usize]);
    let mut usage = vec![Usage::Used; count];
    for n in 0..count as NodeId {
        if !matches!(g.kind(n), NodeKind::Type | NodeKind::Member) || set_aside(g, n) || main_use[n as usize] || entered(n) {
            continue;
        }
        let owner = g.parent(n).filter(|&p| g.kind(p) == NodeKind::Type);
        if owner.is_some_and(|o| !main_use[o as usize] && !entered(o)) || (is_constructor(g, n) && owner.is_some()) || file_body(g, n) || overrides(g, n) {
            continue;
        }
        usage[n as usize] = if test_use[n as usize] {
            Usage::OnlyTests
        } else if g.visibility(n) == Visibility::Public {
            Usage::NoCallerInProject
        } else {
            Usage::Unused
        };
    }
    let candidates: HashSet<&str> = (0..count as NodeId).filter(|&n| usage[n as usize] != Usage::Used).map(|n| g.name(n)).collect();
    let mentions = mentions(g, root, &candidates, &tests);
    for n in 0..count as NodeId {
        if usage[n as usize] == Usage::Used {
            continue;
        }
        let mut spans = vec![n];
        let mut i = 0;
        while i < spans.len() {
            spans.extend(g.children(spans[i]));
            i += 1;
        }
        let own = |&(file, line, _): &(&str, u32, bool)| {
            spans.iter().any(|&m| {
                let (start, end) = g.span(m);
                Some(file) == g.file(m) && (start..=end).contains(&line)
            })
        };
        let elsewhere: Vec<&(&str, u32, bool)> = mentions.get(g.name(n)).into_iter().flatten().filter(|m| !own(m)).collect();
        if elsewhere.iter().any(|m| !m.2) {
            usage[n as usize] = Usage::Used;
        } else if !elsewhere.is_empty() {
            usage[n as usize] = Usage::OnlyTests;
        }
    }
    let callers = (0..count as NodeId)
        .map(|n| {
            let owner = g.parent(n).filter(|&p| g.kind(n) == NodeKind::Member && g.kind(p) == NodeKind::Type).unwrap_or(n);
            incoming[n as usize].iter().filter(|&&s| !g.is_ancestor(owner, s)).count() as u32
        })
        .collect();
    Analysis { usage, tested: tested(g, &tests), callers }
}

fn mentions<'g>(g: &'g ArchivedGraph, root: &Path, names: &HashSet<&str>, tests: &[bool]) -> HashMap<String, Vec<(&'g str, u32, bool)>> {
    let mut files: HashMap<&str, bool> = HashMap::new();
    for n in 0..g.node_count() as NodeId {
        if let Some(f) = g.file(n) {
            *files.entry(f).or_default() |= tests[n as usize];
        }
    }
    let mut out: HashMap<String, Vec<(&str, u32, bool)>> = HashMap::new();
    for (&file, &test) in &files {
        let Ok(text) = std::fs::read_to_string(root.join(file)) else { continue };
        for (i, line) in text.lines().enumerate() {
            for word in line.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$')).filter(|w| names.contains(w)) {
                out.entry(word.to_string()).or_default().push((file, i as u32 + 1, test));
            }
        }
    }
    out
}

fn tested(g: &ArchivedGraph, tests: &[bool]) -> Vec<bool> {
    let mut reached = vec![false; g.node_count()];
    let mut todo: Vec<NodeId> = (0..g.node_count() as NodeId).filter(|&n| tests[n as usize]).collect();
    while let Some(n) = todo.pop() {
        for e in g.out_edges(n).iter().filter(|e| EdgeKind::from_u8(e.kind).is_call_like()) {
            let to = e.to.to_native();
            if !reached[to as usize] && !tests[to as usize] {
                reached[to as usize] = true;
                todo.push(to);
            }
        }
    }
    for n in 0..g.node_count() as NodeId {
        if reached[n as usize] {
            let mut cur = g.parent(n);
            while let Some(p) = cur.filter(|&p| g.kind(p) == NodeKind::Type) {
                reached[p as usize] = true;
                cur = g.parent(p);
            }
        }
    }
    reached
}
