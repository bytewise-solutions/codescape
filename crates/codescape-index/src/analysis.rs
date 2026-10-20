use codescape_format::{ArchivedGraph, EdgeFlags, EdgeKind, Lang, NodeFlags, NodeId, NodeKind, Scope, SubKind, Visibility};
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

const IMPLICIT: &[(Lang, &str, usize)] = &[
    (Lang::Java, "equals", 1),
    (Lang::Java, "hashCode", 0),
    (Lang::Java, "toString", 0),
    (Lang::Java, "clone", 0),
    (Lang::Java, "finalize", 0),
    (Lang::Go, "String", 0),
    (Lang::Go, "Error", 0),
    (Lang::TypeScript, "toString", 0),
    (Lang::TypeScript, "toJSON", 0),
    (Lang::TypeScript, "toJSON", 1),
    (Lang::TypeScript, "valueOf", 0),
    (Lang::JavaScript, "toString", 0),
    (Lang::JavaScript, "toJSON", 0),
    (Lang::JavaScript, "toJSON", 1),
    (Lang::JavaScript, "valueOf", 0),
];

fn arity(text: &str, start_line: u32, name: &str) -> Option<usize> {
    let head: String = text.lines().skip(start_line.saturating_sub(1) as usize).take(12).collect::<Vec<_>>().join("\n");
    let at = head.find(&format!("{name}("))? + name.len() + 1;
    let (mut depth, mut commas, mut any) = (0usize, 0usize, false);
    for c in head[at..].chars() {
        match c {
            ')' if depth == 0 => return Some(if any { commas + 1 } else { 0 }),
            '(' | '<' | '[' | '{' => depth += 1,
            ')' | '>' | ']' | '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => commas += 1,
            c if !c.is_whitespace() => any = true,
            _ => {}
        }
    }
    None
}

fn declaration(text: &str, start_line: u32, name: &str) -> Option<String> {
    let head: String = text.lines().skip(start_line.saturating_sub(1) as usize).take(12).collect::<Vec<_>>().join("\n");
    let at = head.find(&format!("{name}("))?;
    Some(head[..at].to_string())
}

fn marked_override(lang: Lang, declaration: &str) -> bool {
    let words = || declaration.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '@'));
    match lang {
        Lang::Java => words().any(|w| w == "@Override"),
        Lang::TypeScript | Lang::JavaScript => words().any(|w| w == "override"),
        _ => false,
    }
}

fn implicitly_called(g: &ArchivedGraph, root: &Path, n: NodeId, sources: &mut HashMap<String, Option<String>>) -> bool {
    if g.kind(n) != NodeKind::Member || g.sub_kind(n) != SubKind::Method {
        return false;
    }
    let (lang, name) = (g.lang(n), g.name(n));
    let wanted: Vec<usize> = IMPLICIT.iter().filter(|&&(l, m, _)| l == lang && m == name).map(|&(_, _, a)| a).collect();
    if wanted.is_empty() && !matches!(lang, Lang::Java | Lang::TypeScript | Lang::JavaScript) {
        return false;
    }
    let Some(file) = g.file(n) else { return false };
    let Some(text) = sources.entry(file.to_string()).or_insert_with(|| std::fs::read_to_string(root.join(file)).ok()) else { return false };
    let start = g.span(n).0;
    declaration(text, start, name).is_some_and(|d| marked_override(lang, &d)) || arity(text, start, name).is_some_and(|a| wanted.contains(&a))
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
    let mut holds_entry = vec![false; count];
    for ep in g.entries() {
        for p in g.path(ep.node.to_native()) {
            holds_entry[p as usize] = true;
        }
    }
    let mut usage = vec![Usage::Used; count];
    let mut sources = HashMap::new();
    for n in 0..count as NodeId {
        if !matches!(g.kind(n), NodeKind::Type | NodeKind::Member) || set_aside(g, n) || main_use[n as usize] || entered(n) || holds_entry[n as usize] {
            continue;
        }
        let owner = g.parent(n).filter(|&p| g.kind(p) == NodeKind::Type);
        if owner.is_some_and(|o| !main_use[o as usize] && !entered(o) && !holds_entry[o as usize])
            || (is_constructor(g, n) && owner.is_some())
            || file_body(g, n)
            || overrides(g, n)
            || implicitly_called(g, root, n, &mut sources)
        {
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
