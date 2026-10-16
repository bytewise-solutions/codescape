use crate::reach::ReachSet;
use srcrelief_format::*;
use srcrelief_layout::Rect;
use srcrelief_rules::channel_label;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

const MIN_SHARED_STEM: usize = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum End {
    Clients,
    App(usize),
    Component(usize, usize),
    Channel(usize),
    External(usize),
}

#[derive(Clone, Debug)]
pub struct Component {
    pub label: String,
    pub detail: String,
    pub node: NodeId,
    pub entry: usize,
    starts: Vec<NodeId>,
}

#[derive(Clone, Debug)]
pub struct App {
    pub label: String,
    pub entry: usize,
    pub node: NodeId,
    pub components: Vec<Component>,
}

#[derive(Clone, Debug)]
pub struct Channel {
    pub label: String,
    pub stem: String,
    pub publishers: Vec<NodeId>,
}

#[derive(Clone, Debug)]
pub struct Link {
    pub from: End,
    pub to: End,
    pub label: &'static str,
    pub inferred: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Overview {
    pub apps: Vec<App>,
    pub channels: Vec<Channel>,
    pub externals: Vec<String>,
    pub links: Vec<Link>,
}

impl Overview {
    pub fn label(&self, e: End) -> String {
        match e {
            End::Clients => "HTTP clients".into(),
            End::App(a) => self.apps[a].label.clone(),
            End::Component(a, c) => self.apps[a].components[c].label.clone(),
            End::Channel(c) => self.channels[c].label.clone(),
            End::External(x) => self.externals[x].clone(),
        }
    }

    pub fn has_clients(&self) -> bool {
        self.links.iter().any(|l| l.from == End::Clients)
    }
}

fn owner_type(g: &ArchivedGraph, n: NodeId) -> Option<NodeId> {
    g.path(n).into_iter().rev().find(|&p| g.kind(p) == NodeKind::Type)
}

fn subtree(g: &ArchivedGraph, n: NodeId) -> Vec<NodeId> {
    let mut out = vec![n];
    let mut i = 0;
    while i < out.len() {
        out.extend(g.children(out[i]));
        i += 1;
    }
    out
}

struct Reacher<'g> {
    g: &'g ArchivedGraph,
    implementors: HashMap<NodeId, Vec<NodeId>>,
    boundary: ReachSet,
    module_of: Vec<NodeId>,
}

fn is_boundary(g: &ArchivedGraph, n: NodeId) -> bool {
    g.sub_kind(n) == SubKind::Constructor || std::iter::once(n).chain(owner_type(g, n)).any(|m| g.tags(m).any(|t| t.starts_with("external:")))
}

impl<'g> Reacher<'g> {
    fn new(g: &'g ArchivedGraph) -> Reacher<'g> {
        let mut implementors: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        for n in 0..g.node_count() as NodeId {
            for e in g.out_edges(n).iter().filter(|e| matches!(EdgeKind::from_u8(e.kind), EdgeKind::Implements | EdgeKind::Inherits)) {
                implementors.entry(e.to.to_native()).or_default().push(n);
            }
        }
        let mut boundary = ReachSet::new(g.node_count());
        for n in 0..g.node_count() as NodeId {
            if is_boundary(g, n) {
                boundary.insert(n);
            }
        }
        let mut module_of = vec![g.root(); g.node_count()];
        for m in g.children(g.root()) {
            for n in subtree(g, m) {
                module_of[n as usize] = m;
            }
        }
        Reacher { g, implementors, boundary, module_of }
    }

    fn visible_from(&self, n: NodeId) -> ReachSet {
        let g = self.g;
        let mut seen = ReachSet::new(g.node_count());
        let start = self.module_of[n as usize];
        seen.insert(start);
        let mut q = vec![start];
        while let Some(m) = q.pop() {
            for e in g.out_edges(m).iter().filter(|e| EdgeKind::from_u8(e.kind) == EdgeKind::DependsOn && Scope::from_u8(e.scope) != Scope::Test) {
                if seen.insert(e.to.to_native()) {
                    q.push(e.to.to_native());
                }
            }
        }
        seen
    }

    fn reach(&self, starts: &[NodeId], follow_wiring: bool, components: &ReachSet, visible: &ReachSet) -> Vec<NodeId> {
        let g = self.g;
        let mut seen = ReachSet::new(g.node_count());
        let mut out = Vec::new();
        let mut q: VecDeque<NodeId> = VecDeque::new();
        for &s in starts {
            if seen.insert(s) {
                q.push_back(s);
            }
        }
        while let Some(n) = q.pop_front() {
            out.push(n);
            let foreign = owner_type(g, n).is_some_and(|o| components.contains(o) && !starts.contains(&o));
            if self.boundary.contains(n) || foreign {
                continue;
            }
            let mut next: Vec<NodeId> = g
                .out_edges(n)
                .iter()
                .filter(|e| !EdgeFlags(e.flags).contains(EdgeFlags::NOISE) && (follow_wiring || !EdgeFlags(e.flags).contains(EdgeFlags::WIRING)))
                .filter(|e| !matches!(EdgeKind::from_u8(e.kind), EdgeKind::DependsOn | EdgeKind::Implements | EdgeKind::Inherits))
                .map(|e| e.to.to_native())
                .collect();
            if let Some(impls) = g.parent(n).and_then(|p| self.implementors.get(&p)) {
                next.extend(impls.iter().filter_map(|&t| g.children(t).find(|&c| g.name(c) == g.name(n))));
            }
            for m in next {
                if visible.contains(self.module_of[m as usize]) && !g.flags(m).intersects(NodeFlags::TEST | NodeFlags::MOCK) && seen.insert(m) {
                    q.push_back(m);
                }
            }
        }
        out
    }
}

#[derive(Default)]
struct Found {
    publishes: BTreeMap<String, NodeId>,
    externals: BTreeSet<String>,
}

fn found(g: &ArchivedGraph, reached: &[NodeId]) -> Found {
    let mut f = Found::default();
    for &n in reached {
        let owner = owner_type(g, n);
        for m in std::iter::once(n).chain(owner.filter(|&o| o != n)) {
            for t in g.tags(m) {
                if let Some(stem) = t.strip_prefix("channel:out:") {
                    f.publishes.entry(stem.to_string()).or_insert(owner.unwrap_or(m));
                } else if let Some(x) = t.strip_prefix("external:") {
                    f.externals.insert(x.to_string());
                }
            }
        }
    }
    f
}

fn stems_match(a: &str, b: &str) -> bool {
    a.len().min(b.len()) >= MIN_SHARED_STEM && (a.starts_with(b) || b.starts_with(a))
}

fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

fn components(g: &ArchivedGraph, app: usize) -> Vec<Component> {
    let children: Vec<usize> = (0..g.entries().len()).filter(|&j| g.entry_parent(j) == Some(app) && g.entry_category(j) != Category::Test).collect();
    let mut out = Vec::new();
    let routes: Vec<usize> = children.iter().copied().filter(|&j| g.entry_kind(j) == "http-route").collect();
    if !routes.is_empty() {
        out.push(Component {
            label: "HTTP API".into(),
            detail: plural(routes.len(), "route"),
            node: g.entries()[app].node.to_native(),
            entry: app,
            starts: routes.iter().map(|&j| g.entries()[j].node.to_native()).collect(),
        });
    }
    let mut owners: BTreeMap<(String, NodeId), Vec<usize>> = BTreeMap::new();
    for &j in children.iter().filter(|&&j| g.entry_kind(j) != "http-route") {
        let node = g.entries()[j].node.to_native();
        let owner = owner_type(g, node).unwrap_or(node);
        owners.entry((g.name(owner).to_string(), owner)).or_default().push(j);
    }
    for ((label, owner), entries) in owners {
        let kind = g.entry_kind(entries[0]).replace('-', " ");
        out.push(Component { label, detail: plural(entries.len(), &kind), node: owner, entry: entries[0], starts: subtree(g, owner) });
    }
    out
}

pub fn build(g: &ArchivedGraph) -> Overview {
    let mut mains: Vec<usize> =
        (0..g.entries().len()).filter(|&i| g.entry_kind(i) == "main" && g.entry_category(i) == Category::Application && g.entry_parent(i).is_none()).collect();
    mains.sort_by(|&a, &b| g.entry_label(a).cmp(g.entry_label(b)));
    let mut o = Overview::default();
    for &i in &mains {
        o.apps.push(App { label: g.entry_label(i).to_string(), entry: i, node: g.entries()[i].node.to_native(), components: components(g, i) });
    }
    o.apps.sort_by_key(|a| a.components.is_empty());
    let reacher = Reacher::new(g);
    let mut owners = ReachSet::new(g.node_count());
    for c in o.apps.iter().flat_map(|a| &a.components).filter(|c| g.kind(c.node) == NodeKind::Type) {
        owners.insert(c.node);
    }
    let mut per_end: Vec<(End, Found)> = Vec::new();
    for (a, app) in o.apps.iter().enumerate() {
        let visible = reacher.visible_from(app.node);
        let mut covered = Found::default();
        for (c, comp) in app.components.iter().enumerate() {
            let f = found(g, &reacher.reach(&comp.starts, false, &owners, &visible));
            covered.publishes.extend(f.publishes.iter().map(|(k, v)| (k.clone(), *v)));
            covered.externals.extend(f.externals.iter().cloned());
            per_end.push((End::Component(a, c), f));
        }
        let mut rest = found(g, &reacher.reach(&[app.node], true, &owners, &visible));
        rest.publishes.retain(|k, _| !covered.publishes.contains_key(k));
        rest.externals.retain(|x| !covered.externals.contains(x));
        per_end.push((End::App(a), rest));
    }
    let mut channel_of: BTreeMap<String, usize> = BTreeMap::new();
    let mut publishers: BTreeMap<String, BTreeSet<NodeId>> = BTreeMap::new();
    for (_, f) in &per_end {
        for (stem, &owner) in &f.publishes {
            publishers.entry(stem.clone()).or_default().insert(owner);
        }
    }
    for (stem, owners) in &publishers {
        let first = *owners.iter().min_by_key(|&&n| g.name(n)).unwrap();
        channel_of.insert(stem.clone(), o.channels.len());
        o.channels.push(Channel { label: channel_label(g.name(first)), stem: stem.clone(), publishers: owners.iter().copied().collect() });
    }
    let externals: BTreeSet<String> = per_end.iter().flat_map(|(_, f)| f.externals.iter().cloned()).collect();
    o.externals = externals.into_iter().collect();
    let mut links: BTreeMap<(End, End), Link> = BTreeMap::new();
    let mut add = |from: End, to: End, label: &'static str, inferred: bool| {
        links.entry((from, to)).or_insert(Link { from, to, label, inferred });
    };
    for (end, f) in &per_end {
        for stem in f.publishes.keys() {
            add(*end, End::Channel(channel_of[stem]), "publishes", false);
        }
        for x in &f.externals {
            add(*end, End::External(o.externals.iter().position(|e| e == x).unwrap()), "uses", false);
        }
    }
    for (a, app) in o.apps.iter().enumerate() {
        for (c, comp) in app.components.iter().enumerate() {
            if comp.label == "HTTP API" && comp.node == app.node {
                add(End::Clients, End::Component(a, c), "HTTP", false);
            }
            for stem in g.tags(comp.node).filter_map(|t| t.strip_prefix("channel:in:")) {
                for (k, ch) in o.channels.iter().enumerate() {
                    if stems_match(stem, &ch.stem) && !ch.publishers.contains(&comp.node) {
                        add(End::Channel(k), End::Component(a, c), "consumes", true);
                    }
                }
            }
        }
    }
    o.links = links.into_values().collect();
    o
}

pub struct Placed {
    pub boxes: Vec<(End, Rect)>,
    pub size: [f32; 2],
}

impl Placed {
    pub fn rect(&self, e: End) -> Option<Rect> {
        self.boxes.iter().find(|(x, _)| *x == e).map(|(_, r)| *r)
    }
}

const MARGIN: f32 = 24.0;
const BAND_GAP: f32 = 80.0;
const MAX_BAND_GAP: f32 = 240.0;
const COMP: [f32; 2] = [190.0, 52.0];
const GAP: f32 = 16.0;
const HEADER: f32 = 30.0;
const SMALL: [f32; 2] = [170.0, 40.0];
const MAX_COLS: usize = 4;
const APP_GAP: f32 = 40.0;

fn row(boxes: &mut Vec<(End, Rect)>, ends: impl Iterator<Item = End>, y: f32, width: f32) -> f32 {
    let ends: Vec<End> = ends.collect();
    if ends.is_empty() {
        return y;
    }
    let per_row = (((width - 2.0 * MARGIN + GAP) / (SMALL[0] + GAP)) as usize).max(1);
    let mut bottom = y;
    for (r, chunk) in ends.chunks(per_row).enumerate() {
        let total = chunk.len() as f32 * (SMALL[0] + GAP) - GAP;
        let x0 = ((width - total) / 2.0).max(MARGIN);
        let ry = y + r as f32 * (SMALL[1] + GAP);
        for (i, &e) in chunk.iter().enumerate() {
            boxes.push((e, Rect::new(x0 + i as f32 * (SMALL[0] + GAP), ry, SMALL[0], SMALL[1])));
        }
        bottom = ry + SMALL[1];
    }
    bottom
}

fn frame_size(a: &App) -> [f32; 2] {
    let n = a.components.len();
    if n == 0 {
        return [SMALL[0], HEADER + GAP];
    }
    let cols = n.min(MAX_COLS);
    [cols as f32 * (COMP[0] + GAP) + GAP, HEADER + n.div_ceil(cols) as f32 * (COMP[1] + GAP)]
}

pub fn layout(o: &Overview, width: f32, height: f32) -> Placed {
    let first = place(o, width, BAND_GAP);
    let bands = [o.has_clients(), true, !o.channels.is_empty(), !o.externals.is_empty()].iter().filter(|&&b| b).count();
    let scale = (width / first.size[0]).min(height / first.size[1]).min(1.0);
    let spare = height / scale - first.size[1];
    if bands < 2 || spare <= 0.0 {
        return first;
    }
    place(o, width, (BAND_GAP + spare / (bands - 1) as f32).min(MAX_BAND_GAP))
}

fn place(o: &Overview, width: f32, band_gap: f32) -> Placed {
    let frames: Vec<[f32; 2]> = o.apps.iter().map(frame_size).collect();
    let limit = 2.0 * width - 2.0 * MARGIN;
    let mut lines: Vec<Vec<usize>> = vec![Vec::new()];
    let mut used = 0.0;
    for (a, f) in frames.iter().enumerate() {
        if used > 0.0 && used + f[0] > limit {
            lines.push(Vec::new());
            used = 0.0;
        }
        lines.last_mut().unwrap().push(a);
        used += f[0] + APP_GAP;
    }
    let line_w = |l: &[usize]| l.iter().map(|&a| frames[a][0] + APP_GAP).sum::<f32>() - APP_GAP;
    let width = lines.iter().map(|l| line_w(l) + 2.0 * MARGIN).fold(width, f32::max);
    let mut boxes: Vec<(End, Rect)> = Vec::new();
    let mut y = MARGIN;
    if o.has_clients() {
        y = row(&mut boxes, std::iter::once(End::Clients), y, width) + band_gap;
    }
    for line in lines.iter().filter(|l| !l.is_empty()) {
        let mut x = (width - line_w(line)) / 2.0;
        for &a in line {
            let [w, h] = frames[a];
            boxes.push((End::App(a), Rect::new(x, y, w, h)));
            let cols = o.apps[a].components.len().clamp(1, MAX_COLS);
            for c in 0..o.apps[a].components.len() {
                let (r, k) = (c / cols, c % cols);
                boxes.push((End::Component(a, c), Rect::new(x + GAP + k as f32 * (COMP[0] + GAP), y + HEADER + r as f32 * (COMP[1] + GAP), COMP[0], COMP[1])));
            }
            x += w + APP_GAP;
        }
        y += line.iter().map(|&a| frames[a][1]).fold(0.0, f32::max) + band_gap;
    }
    let mut bottom = y - band_gap;
    if !o.channels.is_empty() {
        bottom = row(&mut boxes, (0..o.channels.len()).map(End::Channel), y, width);
        y = bottom + band_gap;
    }
    if !o.externals.is_empty() {
        bottom = row(&mut boxes, (0..o.externals.len()).map(End::External), y, width);
    }
    Placed { boxes, size: [width, bottom + MARGIN] }
}
