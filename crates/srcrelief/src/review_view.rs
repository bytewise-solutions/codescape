use srcrelief_format::{ArchivedGraph, NodeId, NodeKind, Visibility};
use srcrelief_index::analysis::{Analysis, Usage};
use srcrelief_index::review::{Change, Review, Status};
use std::collections::HashMap;
use std::path::Path;

const COMPLEXITY_HINT: u16 = 10;
const MANY_CALLERS: u32 = 5;

#[derive(Clone, Debug, PartialEq)]
pub enum Risk {
    Complexity(u16, u16),
    Callers(u32),
    Signature,
    MadePublic,
    Untested,
    Unused(Usage),
}

impl Risk {
    pub fn label(&self) -> String {
        match self {
            Risk::Complexity(before, after) => format!("complexity {before} to {after}"),
            Risk::Callers(n) => format!("used by {n}"),
            Risk::Signature => "signature".into(),
            Risk::MadePublic => "made public".into(),
            Risk::Untested => "untested".into(),
            Risk::Unused(u) => u.label().unwrap_or_default().into(),
        }
    }

    fn weight(&self) -> u32 {
        match self {
            Risk::Complexity(before, after) => 30 + u32::from(after - before),
            Risk::Callers(n) => 20 + n.min(&30),
            Risk::Signature => 25,
            Risk::Unused(_) => 20,
            Risk::MadePublic => 15,
            Risk::Untested => 10,
        }
    }
}

fn risks(g: &ArchivedGraph, n: NodeId, c: &Change, a: &Analysis) -> Vec<Risk> {
    let mut out = Vec::new();
    if let Some((before, after)) = c.complexity.filter(|&(b, a)| a > b && a >= COMPLEXITY_HINT) {
        out.push(Risk::Complexity(before, after));
    }
    let callers = a.callers[n as usize];
    if c.status == Status::Changed && callers >= MANY_CALLERS {
        out.push(Risk::Callers(callers));
    }
    if c.status == Status::Changed && c.signature && g.visibility(n) == Visibility::Public {
        out.push(Risk::Signature);
    }
    if c.widened.is_some_and(|(_, to)| to == Visibility::Public) {
        out.push(Risk::MadePublic);
    }
    if c.status == Status::New && a.usage[n as usize] != Usage::Used {
        out.push(Risk::Unused(a.usage[n as usize]));
    }
    if a.tested.iter().any(|&t| t) && !a.tested[n as usize] {
        out.push(Risk::Untested);
    }
    out
}

pub fn by_risk(items: &mut [ReviewItem]) {
    items.sort_by_key(|i| std::cmp::Reverse(i.risks.iter().map(Risk::weight).sum::<u32>()));
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkState {
    Unseen,
    Reviewed,
    ChangedSince,
}

#[derive(Clone, Debug, Default)]
pub struct ReviewMarks {
    seen: HashMap<String, u64>,
}

impl ReviewMarks {
    pub fn load(path: &Path) -> ReviewMarks {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        let seen = text.lines().filter_map(|l| l.split_once('\t')).filter_map(|(h, k)| Some((k.to_string(), u64::from_str_radix(h, 16).ok()?))).collect();
        ReviewMarks { seen }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let mut rows: Vec<String> = self.seen.iter().map(|(k, h)| format!("{h:016x}\t{k}")).collect();
        rows.sort();
        std::fs::write(path, rows.join("\n") + "\n")
    }

    pub fn state(&self, item: &ReviewItem) -> MarkState {
        match self.seen.get(&item.key) {
            None => MarkState::Unseen,
            Some(&h) if h == item.fingerprint => MarkState::Reviewed,
            Some(_) => MarkState::ChangedSince,
        }
    }

    pub fn toggle(&mut self, item: &ReviewItem) {
        if self.state(item) == MarkState::Reviewed {
            self.seen.remove(&item.key);
        } else {
            self.seen.insert(item.key.clone(), item.fingerprint);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ItemKind {
    New,
    Changed,
    Removed,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReviewItem {
    pub node: Option<NodeId>,
    pub parent: Option<NodeId>,
    pub group: String,
    pub label: String,
    pub kind: ItemKind,
    pub detail: String,
    pub key: String,
    pub fingerprint: u64,
    pub risks: Vec<Risk>,
}

fn key_of(g: &ArchivedGraph, n: NodeId) -> String {
    g.path(n).iter().skip(1).map(|&p| g.name(p)).collect::<Vec<_>>().join("/")
}

fn group_of(g: &ArchivedGraph, n: NodeId) -> String {
    g.path(n).into_iter().filter(|&p| matches!(g.kind(p), NodeKind::Module | NodeKind::Package)).map(|p| g.name(p)).collect::<Vec<_>>().join(" / ")
}

fn label_of(g: &ArchivedGraph, n: NodeId) -> String {
    match g.parent(n).filter(|&p| g.kind(n) == NodeKind::Member && g.kind(p) == NodeKind::Type) {
        Some(t) => format!("{}.{}", g.name(t), g.name(n)),
        None => g.name(n).to_string(),
    }
}

fn under_new(g: &ArchivedGraph, r: &Review, n: NodeId) -> bool {
    g.path(n).into_iter().rev().skip(1).any(|p| matches!(g.kind(p), NodeKind::Type) && r.change(p).status == Status::New)
}

pub fn items(g: &ArchivedGraph, r: &Review, a: &Analysis) -> Vec<ReviewItem> {
    let mut out = Vec::new();
    for n in (0..g.node_count() as NodeId).filter(|&n| matches!(g.kind(n), NodeKind::Type | NodeKind::Member)) {
        let c = r.change(n);
        let item = |kind: ItemKind, detail: String| ReviewItem {
            node: Some(n),
            parent: g.parent(n),
            group: group_of(g, n),
            label: label_of(g, n),
            kind,
            detail,
            key: key_of(g, n),
            fingerprint: c.fingerprint,
            risks: risks(g, n, c, a),
        };
        match c.status {
            Status::New if !under_new(g, r, n) => out.push(item(ItemKind::New, format!("+{} lines", c.added))),
            Status::Changed => {
                let own = c.added + c.removed > 0 || c.complexity.is_some();
                let leaf = g.kind(n) == NodeKind::Member || c.inside == Default::default();
                if own && leaf {
                    let mut detail = format!("+{} −{}", c.added, c.removed);
                    if let Some((before, after)) = c.complexity {
                        detail.push_str(&format!(" · complexity {before} to {after}"));
                    }
                    out.push(item(ItemKind::Changed, detail));
                }
            }
            _ => {}
        }
    }
    for rm in &r.removed {
        let name = rm.path.rsplit('/').next().unwrap_or(&rm.path);
        let (group, label) = match rm.parent {
            Some(p) if g.kind(p) == NodeKind::Type => (group_of(g, p), format!("{}.{name}", g.name(p))),
            Some(p) => (group_of(g, p), name.to_string()),
            None => (String::new(), rm.path.clone()),
        };
        out.push(ReviewItem { node: None, parent: rm.parent, group, label, kind: ItemKind::Removed, detail: "removed".into(), key: format!("removed:{}", rm.path), fingerprint: 1, risks: Vec::new() });
    }
    out.sort_by(|a, b| (&a.group, &a.label).cmp(&(&b.group, &b.label)));
    out
}

pub fn step(len: usize, current: Option<usize>, forward: bool) -> Option<usize> {
    if len == 0 {
        return None;
    }
    Some(match (current, forward) {
        (None, true) => 0,
        (None, false) => len - 1,
        (Some(i), true) => (i + 1) % len,
        (Some(i), false) => (i + len - 1) % len,
    })
}
