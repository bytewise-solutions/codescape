use crate::refs::followable;
use crate::review_view::Limits;
use codescape_format::{ArchivedGraph, NodeId, NodeKind, Visibility};
use codescape_index::analysis::{Analysis, Usage};
use codescape_layout::{LayoutOptions, upward_edges};
use std::collections::HashMap;

const CAP: usize = 50;
pub const HOTSPOT_MIN_COMMITS: u32 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FlagKind {
    Complex,
    Hotspot,
    Unused,
    OnlyTests,
    NoCaller,
    Untested,
    Duplicate,
    Upward,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Flag {
    pub kind: FlagKind,
    pub short: String,
    pub explanation: String,
    pub links: Vec<NodeId>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub node: NodeId,
    pub reason: String,
    pub score: u32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Insights {
    pub complex: Vec<Row>,
    pub hotspots: Vec<Row>,
    pub depended: Vec<Row>,
    pub upward: Vec<Row>,
    pub dead: Vec<Row>,
    pub untested: Vec<Row>,
    pub duplicates: Vec<Row>,
}

struct Ctx<'a> {
    g: &'a ArchivedGraph,
    a: &'a Analysis,
    churn: &'a HashMap<String, u32>,
    opts: &'a LayoutOptions,
    limits: &'a Limits,
}

impl Ctx<'_> {
    fn shown(&self, n: NodeId) -> bool {
        followable(self.g, self.opts, n)
    }

    fn complex(&self, n: NodeId) -> Option<u16> {
        self.g.complexity(n).filter(|&c| c > 1 && c >= self.limits.complexity_hint)
    }

    fn commits(&self, n: NodeId) -> u32 {
        self.g.file(n).and_then(|f| self.churn.get(f)).copied().unwrap_or(0)
    }

    fn lines(&self, n: NodeId) -> u32 {
        let (start, end) = self.g.span(n);
        end.saturating_sub(start) + 1
    }

    fn usage(&self, n: NodeId) -> Usage {
        if matches!(self.g.kind(n), NodeKind::Type | NodeKind::Member) { self.a.usage[n as usize] } else { Usage::Used }
    }

    fn untested(&self, n: NodeId) -> bool {
        self.g.complexity(n).is_some() && self.g.visibility(n) == Visibility::Public && !self.a.tested[n as usize] && self.a.tested.iter().any(|&t| t)
    }

    fn duplicates(&self, n: NodeId) -> Vec<(u32, Vec<(NodeId, u32, u32)>)> {
        let mut out: Vec<(u32, Vec<(NodeId, u32, u32)>)> = Vec::new();
        for (group, others) in self.g.clones_of(n) {
            let others: Vec<(NodeId, u32, u32)> = others.into_iter().filter(|p| self.shown(p.0)).collect();
            if !others.is_empty() {
                out.push((self.g.clone_groups().nth(group as usize).map_or(0, |(t, _)| t), others));
            }
        }
        out
    }

    fn upward(&self, n: NodeId) -> Vec<(NodeId, u32)> {
        let path = self.g.path(n);
        path[..path.len() - 1].iter().flat_map(|&c| upward_edges(self.g, c, self.opts)).filter(|e| e.0 == n && self.shown(e.1)).map(|e| (e.1, e.2)).collect()
    }
}

fn label(g: &ArchivedGraph, n: NodeId) -> String {
    match g.parent(n).filter(|&p| g.kind(n) == NodeKind::Member && g.kind(p) == NodeKind::Type) {
        Some(t) => format!("{}.{}", g.name(t), g.name(n)),
        None => g.name(n).to_string(),
    }
}

fn plural(n: u32, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn ranked(mut rows: Vec<Row>) -> Vec<Row> {
    rows.sort_by_key(|r| std::cmp::Reverse(r.score));
    rows.truncate(CAP);
    rows
}

pub fn compute(g: &ArchivedGraph, analysis: &Analysis, churn: &HashMap<String, u32>, opts: &LayoutOptions, limits: &Limits, scope: Option<NodeId>) -> Insights {
    let cx = Ctx { g, a: analysis, churn, opts, limits };
    let nodes: Vec<NodeId> = (0..g.node_count() as NodeId).filter(|&n| scope.is_none_or(|s| g.is_ancestor(s, n)) && cx.shown(n)).collect();
    let mut out = Insights::default();
    let row = |node: NodeId, reason: String, score: u32| Row { node, reason, score };
    for &n in &nodes {
        if let Some(c) = cx.complex(n) {
            let nesting = g.complexity_detail(n).map_or(0, |d| d.2);
            let mut reason = format!("complexity {c}");
            if nesting > 0 {
                reason.push_str(&format!(" · nesting {nesting}"));
            }
            reason.push_str(&format!(" · {}", plural(cx.lines(n), "line", "lines")));
            out.complex.push(row(n, reason, c.into()));
            let commits = cx.commits(n);
            if commits >= HOTSPOT_MIN_COMMITS {
                out.hotspots.push(row(n, format!("changed {commits}× · complexity {c}"), commits * u32::from(c)));
            }
        }
        let callers = analysis.callers[n as usize];
        if matches!(g.kind(n), NodeKind::Type | NodeKind::Member) && callers >= limits.many_callers {
            out.depended.push(row(n, format!("used by {callers} from outside"), callers));
        }
        if let Some(l) = cx.usage(n).label() {
            out.dead.push(row(n, l.to_string(), cx.lines(n)));
        }
        if cx.untested(n) {
            out.untested.push(row(n, "untested".into(), g.complexity(n).unwrap_or(1).into()));
        }
    }
    for c in (0..g.node_count() as NodeId).filter(|&c| g.kind(c) != NodeKind::Member && !g.aggregates(c).is_empty() && cx.shown(c)) {
        for (a, b, count) in upward_edges(g, c, opts) {
            if cx.shown(a) && cx.shown(b) && scope.is_none_or(|s| g.is_ancestor(s, a)) {
                out.upward.push(row(a, format!("→ {}, a higher layer", label(g, b)), count));
            }
        }
    }
    for (tokens, places) in g.clone_groups() {
        let visible: Vec<NodeId> = places.iter().map(|p| p.0).filter(|&p| cx.shown(p)).collect();
        if visible.len() < 2 {
            continue;
        }
        for &p in visible.iter().filter(|&&p| scope.is_none_or(|s| g.is_ancestor(s, p))) {
            if !out.duplicates.iter().any(|r| r.node == p && r.score >= tokens) {
                out.duplicates.retain(|r| r.node != p);
                out.duplicates.push(row(p, format!("{tokens} tokens · {} places", visible.len()), tokens));
            }
        }
    }
    Insights {
        complex: ranked(out.complex),
        hotspots: ranked(out.hotspots),
        depended: ranked(out.depended),
        upward: ranked(out.upward),
        dead: ranked(out.dead),
        untested: ranked(out.untested),
        duplicates: ranked(out.duplicates),
    }
}

pub fn complexity_facts(g: &ArchivedGraph, n: NodeId) -> Vec<String> {
    let (_, parts, nesting) = g.complexity_detail(n).unwrap_or_default();
    let mut facts = Vec::new();
    if !parts.is_empty() {
        let count: u32 = parts.iter().map(|p| u32::from(p.1)).sum();
        let kinds: Vec<String> = parts.iter().map(|(l, k)| format!("{l} ×{k}")).collect();
        facts.push(format!("{} — {}", plural(count, "decision point", "decision points"), kinds.join(", ")));
    }
    if nesting > 0 {
        facts.push(format!("nesting {nesting}"));
    }
    facts
}

const NAME_NOWHERE: &str = "its name appears nowhere else in the sources; reflection or a framework could still use it.";

pub fn flags(g: &ArchivedGraph, analysis: &Analysis, churn: &HashMap<String, u32>, opts: &LayoutOptions, limits: &Limits, n: NodeId) -> Vec<Flag> {
    let cx = Ctx { g, a: analysis, churn, opts, limits };
    let mut out = Vec::new();
    if !cx.shown(n) {
        return out;
    }
    let flag = |kind: FlagKind, short: String, explanation: String, links: Vec<NodeId>| Flag { kind, short, explanation, links };
    if let Some(c) = cx.complex(n) {
        let mut facts = complexity_facts(g, n);
        facts.push(plural(cx.lines(n), "line", "lines"));
        let explanation = format!("Complexity {c} (flagged from {}): {}.", limits.complexity_hint, facts.join("; "));
        out.push(flag(FlagKind::Complex, format!("complexity {c}"), explanation, Vec::new()));
        let commits = cx.commits(n);
        if commits >= HOTSPOT_MIN_COMMITS {
            let explanation = format!(
                "Changed in {} in the last {} days and complexity {c}: a frequent, risky place to edit.",
                plural(commits, "commit", "commits"),
                limits.hotspot_days
            );
            out.push(flag(FlagKind::Hotspot, format!("changed {commits}× · complexity {c}"), explanation, Vec::new()));
        }
    }
    let usage = cx.usage(n);
    let (kind, explanation) = match usage {
        Usage::Used => (None, String::new()),
        Usage::Unused => (Some(FlagKind::Unused), format!("Unused: {NAME_NOWHERE}")),
        Usage::NoCallerInProject => (Some(FlagKind::NoCaller), format!("No caller in the project: {NAME_NOWHERE}")),
        Usage::OnlyTests => (Some(FlagKind::OnlyTests), "Only tests use it: no main code calls it or names it.".to_string()),
    };
    if let (Some(kind), Some(short)) = (kind, usage.label()) {
        out.push(flag(kind, short.into(), explanation, Vec::new()));
    }
    if cx.untested(n) {
        out.push(flag(FlagKind::Untested, "untested".into(), "Public, and no test reaches it.".into(), Vec::new()));
    }
    for (tokens, others) in cx.duplicates(n) {
        let (first, start, end) = others[0];
        let rest = match others.len() - 1 {
            0 => String::new(),
            k => format!(" and {}", plural(k as u32, "other place", "other places")),
        };
        let explanation = format!("Same code as `{}` (lines {start}–{end}){rest}: {tokens} tokens.", label(g, first));
        out.push(flag(FlagKind::Duplicate, format!("duplicate · {tokens} tokens"), explanation, others.iter().map(|p| p.0).collect()));
    }
    for (to, _) in cx.upward(n) {
        let explanation = format!("Depends on `{}`, which sits in a higher layer: this arrow points up and breaks the layering.", label(g, to));
        out.push(flag(FlagKind::Upward, format!("points up to {}", label(g, to)), explanation, vec![to]));
    }
    out
}

#[derive(Clone, Debug, PartialEq)]
pub struct InsightsKey {
    pub analysis: usize,
    pub churn: usize,
    pub opts: LayoutOptions,
    pub limits: Limits,
    pub scope: Option<NodeId>,
}

#[derive(Default)]
pub struct InsightsCache {
    held: Option<(InsightsKey, Insights)>,
}

impl InsightsCache {
    pub fn get(&mut self, key: InsightsKey, make: impl FnOnce() -> Insights) -> &Insights {
        if self.held.as_ref().is_none_or(|(k, _)| *k != key) {
            self.held = Some((key, make()));
        }
        self.current().expect("just filled")
    }

    pub fn current(&self) -> Option<&Insights> {
        self.held.as_ref().map(|h| &h.1)
    }
}

#[derive(Default)]
pub struct FlagsCache {
    held: Option<(InsightsKey, Vec<Flag>)>,
}

impl FlagsCache {
    pub fn get(&mut self, key: InsightsKey, make: impl FnOnce() -> Vec<Flag>) -> &[Flag] {
        if self.held.as_ref().is_none_or(|(k, _)| *k != key) {
            self.held = Some((key, make()));
        }
        self.current()
    }

    pub fn current(&self) -> &[Flag] {
        self.held.as_ref().map_or(&[], |h| &h.1)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Mark {
    pub line: u32,
    pub flags: Vec<Flag>,
}

impl Mark {
    pub fn kinds(&self) -> Vec<FlagKind> {
        let mut kinds: Vec<FlagKind> = self.flags.iter().map(|f| f.kind).collect();
        kinds.dedup();
        kinds
    }
}

pub fn gutter_marks(items: impl IntoIterator<Item = (u32, Vec<Flag>)>) -> Vec<Mark> {
    let mut by_line: std::collections::BTreeMap<u32, Vec<Flag>> = Default::default();
    for (line, flags) in items.into_iter().filter(|(_, f)| !f.is_empty()) {
        by_line.entry(line).or_default().extend(flags);
    }
    by_line
        .into_iter()
        .map(|(line, mut flags)| {
            flags.sort_by_key(|f| f.kind);
            Mark { line, flags }
        })
        .collect()
}

pub fn file_marks(g: &ArchivedGraph, analysis: &Analysis, churn: &HashMap<String, u32>, opts: &LayoutOptions, limits: &Limits, file: NodeId) -> Vec<Mark> {
    let Some(path) = g.file(file) else { return Vec::new() };
    let mut nodes = vec![file];
    let mut stack: Vec<NodeId> = g.children(file).collect();
    while let Some(n) = stack.pop() {
        if g.file(n) == Some(path) {
            nodes.push(n);
        }
        stack.extend(g.children(n));
    }
    let blocks = nodes.into_iter().filter(|&n| matches!(g.kind(n), NodeKind::Member | NodeKind::Type));
    gutter_marks(blocks.map(|n| (g.span(n).0, flags(g, analysis, churn, opts, limits, n))))
}

#[derive(Default)]
pub struct FileMarks {
    held: std::cell::RefCell<HashMap<NodeId, (InsightsKey, std::rc::Rc<Vec<Mark>>)>>,
}

impl FileMarks {
    pub fn get(&self, file: NodeId, key: &InsightsKey, make: impl FnOnce() -> Vec<Mark>) -> std::rc::Rc<Vec<Mark>> {
        if let Some((k, marks)) = self.held.borrow().get(&file)
            && k == key
        {
            return marks.clone();
        }
        let marks = std::rc::Rc::new(make());
        self.held.borrow_mut().insert(file, (key.clone(), marks.clone()));
        marks
    }

    pub fn retain(&self, keep: impl Fn(NodeId) -> bool) {
        self.held.borrow_mut().retain(|n, _| keep(*n));
    }
}

pub fn worst_complexity(g: &ArchivedGraph, opts: &LayoutOptions) -> HashMap<NodeId, u16> {
    let mut worst = HashMap::new();
    for n in 0..g.node_count() as NodeId {
        let Some(c) = g.complexity(n) else { continue };
        if g.path(n).iter().any(|&p| opts.hides(g, p)) {
            continue;
        }
        for p in g.path(n) {
            let w = worst.entry(p).or_insert(0);
            *w = (*w).max(c);
        }
    }
    worst
}
