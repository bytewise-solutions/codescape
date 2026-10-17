use srcrelief_format::{ArchivedGraph, NodeId, NodeKind};
use srcrelief_index::review::{Review, Status};

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

pub fn items(g: &ArchivedGraph, r: &Review) -> Vec<ReviewItem> {
    let mut out = Vec::new();
    for n in (0..g.node_count() as NodeId).filter(|&n| matches!(g.kind(n), NodeKind::Type | NodeKind::Member)) {
        let c = r.change(n);
        let item = |kind: ItemKind, detail: String| ReviewItem { node: Some(n), parent: g.parent(n), group: group_of(g, n), label: label_of(g, n), kind, detail };
        match c.status {
            Status::New if !under_new(g, r, n) => out.push(item(ItemKind::New, format!("+{} lines", c.added))),
            Status::Changed => {
                let own = c.added + c.removed > 0 || c.complexity.is_some();
                let leaf = g.kind(n) == NodeKind::Member || c.inside == Default::default();
                if own && leaf {
                    let mut detail = format!("+{} −{}", c.added, c.removed);
                    if let Some((before, after)) = c.complexity {
                        detail.push_str(&format!(" · complexity {before} → {after}"));
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
        out.push(ReviewItem { node: None, parent: rm.parent, group, label, kind: ItemKind::Removed, detail: "removed".into() });
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
