use codescape_format::{ArchivedGraph, NodeFlags, NodeId, NodeKind, Visibility};
use codescape_index::analysis::{Analysis, Usage};
use codescape_index::review::{Change, Review, Status};
use std::collections::HashMap;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limits {
    pub complexity_hint: u16,
    pub many_callers: u32,
    pub hotspot_days: u32,
    pub unused: bool,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { complexity_hint: 10, many_callers: 5, hotspot_days: 365, unused: true }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Risk {
    Complexity(u16, u16),
    Callers(u32),
    Signature,
    MadePublic,
    Untested,
    Unused(Usage),
    Upward(String),
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
            Risk::Upward(target) => format!("new upward dependency on {target}"),
        }
    }

    fn weight(&self) -> u32 {
        match self {
            Risk::Complexity(before, after) => 30 + u32::from(after - before),
            Risk::Callers(n) => 20 + n.min(&30),
            Risk::Signature | Risk::Upward(_) => 25,
            Risk::Unused(_) => 20,
            Risk::MadePublic => 15,
            Risk::Untested => 10,
        }
    }
}

fn risks(g: &ArchivedGraph, n: NodeId, c: &Change, a: &Analysis, limits: Limits) -> Vec<Risk> {
    let mut out = Vec::new();
    if let Some((before, after)) = c.complexity.filter(|&(b, a)| a > b && a >= limits.complexity_hint) {
        out.push(Risk::Complexity(before, after));
    }
    let callers = a.callers[n as usize];
    if c.status == Status::Changed && callers >= limits.many_callers {
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
    let test_code = g.path(n).into_iter().any(|p| g.flags(p).intersects(NodeFlags::TEST | NodeFlags::MOCK));
    if a.tested.iter().any(|&t| t) && !a.tested[n as usize] && !test_code {
        out.push(Risk::Untested);
    }
    out
}

fn reaches(g: &ArchivedGraph, n: NodeId, target: NodeId) -> bool {
    g.out_edges(n).iter().any(|e| g.is_ancestor(target, e.to.to_native())) || g.children(n).any(|c| reaches(g, c, target))
}

pub fn flag_upward(g: &ArchivedGraph, items: &mut [ReviewItem], upward: &[(NodeId, NodeId)]) {
    for &(from, to) in upward {
        let inside: Vec<usize> = (0..items.len()).filter(|&i| items[i].node.is_some_and(|n| g.is_ancestor(from, n) || g.is_ancestor(n, from))).collect();
        let callers: Vec<usize> = inside.iter().copied().filter(|&i| items[i].node.is_some_and(|n| reaches(g, n, to))).collect();
        for i in if callers.is_empty() { inside } else { callers } {
            let risk = Risk::Upward(g.name(to).to_string());
            if !items[i].risks.contains(&risk) {
                items[i].risks.push(risk);
            }
        }
    }
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReviewProgress {
    pub reviewed: usize,
    pub changed_since: usize,
    pub unseen: usize,
}

impl ReviewProgress {
    pub fn total(&self) -> usize {
        self.reviewed + self.changed_since + self.unseen
    }
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

    pub fn progress(&self, items: &[ReviewItem], which: impl IntoIterator<Item = usize>) -> ReviewProgress {
        let mut p = ReviewProgress::default();
        for i in which {
            match self.state(&items[i]) {
                MarkState::Reviewed => p.reviewed += 1,
                MarkState::ChangedSince => p.changed_since += 1,
                MarkState::Unseen => p.unseen += 1,
            }
        }
        p
    }

    pub fn all_reviewed(&self, items: &[ReviewItem], which: &[usize]) -> bool {
        !which.is_empty() && which.iter().all(|&i| self.state(&items[i]) == MarkState::Reviewed)
    }

    pub fn set_reviewed(&mut self, items: &[ReviewItem], which: &[usize], reviewed: bool) {
        for &i in which {
            if reviewed {
                self.seen.insert(items[i].key.clone(), items[i].fingerprint);
            } else {
                self.seen.remove(&items[i].key);
            }
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
    pub base_flags: NodeFlags,
}

pub fn shown_items(g: &ArchivedGraph, items: &[ReviewItem], opts: &codescape_layout::LayoutOptions) -> Vec<ReviewItem> {
    let hidden = opts.effective_hidden().0 & !NodeFlags::DETAIL.0;
    let on_map = |n: NodeId| !g.path(n).into_iter().any(|p| opts.hides_beyond_details(g, p));
    items.iter().filter(|i| i.base_flags.0 & hidden == 0 && i.node.or(i.parent).is_none_or(on_map)).cloned().collect()
}

pub fn within(g: &ArchivedGraph, items: &[ReviewItem], selection: NodeId) -> Vec<usize> {
    (0..items.len()).filter(|&i| items[i].node.or(items[i].parent).is_some_and(|n| g.is_ancestor(selection, n))).collect()
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
    items_with(g, r, a, Limits::default())
}

pub fn items_with(g: &ArchivedGraph, r: &Review, a: &Analysis, limits: Limits) -> Vec<ReviewItem> {
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
            key: r.key(n).to_string(),
            fingerprint: c.fingerprint,
            risks: risks(g, n, c, a, limits),
            base_flags: NodeFlags::NONE,
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
        out.push(ReviewItem {
            node: None,
            parent: rm.parent,
            group,
            label,
            kind: ItemKind::Removed,
            detail: "removed".into(),
            key: format!("removed:{}", rm.path),
            fingerprint: 1,
            risks: Vec::new(),
            base_flags: rm.flags,
        });
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

#[derive(Clone, Debug, PartialEq)]
pub enum CommitStep {
    Commit(codescape_index::review::Commit),
    Uncommitted,
}

impl CommitStep {
    pub fn range(&self) -> (Option<String>, Option<String>) {
        match self {
            CommitStep::Commit(c) => (Some(format!("{}^", c.sha)), Some(c.sha.clone())),
            CommitStep::Uncommitted => (Some("HEAD".into()), None),
        }
    }

    pub fn label(&self) -> &str {
        match self {
            CommitStep::Commit(c) => &c.subject,
            CommitStep::Uncommitted => "Uncommitted changes",
        }
    }
}

pub fn step_from(visible: &[usize], cursor: Option<usize>, forward: bool) -> Option<usize> {
    let c = match cursor {
        Some(c) if !visible.contains(&c) => c,
        _ => return step(visible.len(), cursor.and_then(|c| visible.iter().position(|&v| v == c)), forward).map(|i| visible[i]),
    };
    let next = if forward { visible.iter().find(|&&v| v > c).or(visible.first()) } else { visible.iter().rev().find(|&&v| v < c).or(visible.last()) };
    next.copied()
}

pub fn marks_cursor_only(items: &[ReviewItem], cursor: Option<usize>, selected: Option<NodeId>) -> bool {
    cursor.is_some_and(|c| items[c].node.is_none() && selected.is_some() && items[c].parent == selected)
}

pub fn current(items: &[ReviewItem], cursor: Option<usize>, selected: Option<NodeId>) -> Option<usize> {
    let Some(s) = selected else { return cursor };
    if cursor.is_some_and(|c| items[c].node.or(items[c].parent) == Some(s)) {
        return cursor;
    }
    items.iter().position(|i| i.node == Some(s)).or(cursor)
}

pub fn scroll_to_row(row: usize, pitch: f32, offset: f32, view_h: f32) -> Option<f32> {
    let top = row as f32 * pitch;
    if top >= offset && top + pitch <= offset + view_h {
        return None;
    }
    Some((top - (view_h - pitch) / 2.0).max(0.0))
}

pub fn mark_label(marks: &ReviewMarks, items: &[ReviewItem], inside: &[usize]) -> Option<String> {
    if inside.is_empty() {
        return None;
    }
    if marks.all_reviewed(items, inside) {
        return Some(format!("Unmark reviewed ({})", inside.len()));
    }
    let done = inside.iter().filter(|&&i| marks.state(&items[i]) == MarkState::Reviewed).count();
    Some(format!("Mark reviewed ({done} / {} changes)", inside.len()))
}

pub fn scope_of(g: &ArchivedGraph, n: NodeId) -> Option<NodeId> {
    match g.kind(n) {
        NodeKind::Project | NodeKind::External => None,
        NodeKind::Member => g.path(n).into_iter().rev().skip(1).find(|&p| g.kind(p) == NodeKind::Type).or(g.parent(n).filter(|&p| p != g.root())),
        _ => Some(n),
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReviewScope {
    pub node: Option<NodeId>,
    from: Option<NodeId>,
}

impl ReviewScope {
    pub fn follow(&mut self, g: &ArchivedGraph, selected: Option<NodeId>) {
        if selected != self.from {
            self.from = selected;
            self.node = selected.and_then(|n| scope_of(g, n));
        }
    }

    pub fn keep(&mut self, selected: Option<NodeId>) {
        self.from = selected;
    }

    pub fn show_all(&mut self) {
        self.node = None;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BoxCounts {
    pub new: usize,
    pub changed: usize,
    pub removed: usize,
    pub reviewed: usize,
}

impl BoxCounts {
    pub fn to_review(&self) -> usize {
        self.new + self.changed + self.removed
    }

    pub fn summary(&self) -> String {
        format!("{} new · {} changed · {} removed · {} reviewed", self.new, self.changed, self.removed, self.reviewed)
    }
}

pub fn box_counts(g: &ArchivedGraph, items: &[ReviewItem], marks: &ReviewMarks) -> HashMap<NodeId, BoxCounts> {
    let mut out: HashMap<NodeId, BoxCounts> = HashMap::new();
    for item in items {
        let reviewed = marks.state(item) == MarkState::Reviewed;
        let mut at = item.node.or(item.parent);
        while let Some(n) = at {
            let c = out.entry(n).or_default();
            match (reviewed, item.kind) {
                (true, _) => c.reviewed += 1,
                (false, ItemKind::New) => c.new += 1,
                (false, ItemKind::Changed) => c.changed += 1,
                (false, ItemKind::Removed) => c.removed += 1,
            }
            at = g.parent(n);
        }
    }
    out
}

pub fn on_review(g: &ArchivedGraph, r: &Review, items: &[ReviewItem], counts: &HashMap<NodeId, BoxCounts>, n: NodeId) -> bool {
    counts.contains_key(&n) || g.path(n).into_iter().any(|p| r.change(p).status == Status::New && items.iter().any(|i| i.node == Some(p)))
}

pub fn short_name(name: &str) -> &str {
    name.rsplit(['.', '/']).find(|s| !s.is_empty()).unwrap_or(name)
}
