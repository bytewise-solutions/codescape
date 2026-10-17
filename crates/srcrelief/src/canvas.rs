use crate::camera::{Camera, WRect};
use crate::focus::{Mark, Marks};
use crate::source::file_font;
use srcrelief_format::{ArchivedGraph, NodeFlags, NodeId, NodeKind, SubKind};
use srcrelief_layout::{CARD_MAX_ROWS, CARD_ROW, CARD_TOP, ContainerLayout, Direction, LayoutCache, Rect, box_size, card_members, display_children, display_path};
use srcrelief_rules::VisualRules;

pub const OPEN_THRESHOLD_PX: f32 = 220.0;
pub const MIN_DRAW_PX: f32 = 3.0;
pub const MIN_LABEL_PX: f32 = 7.0;
pub const MIN_CHILD_PX: f32 = 36.0;
pub const FILE_VIEW_SHARE: f32 = 0.45;
pub const MIN_ROW_PX: f32 = 7.0;

#[derive(Clone, Debug, PartialEq)]
pub struct CardRow {
    pub node: NodeId,
    pub rect: Rect,
    pub mark: Mark,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SceneBox {
    pub node: NodeId,
    pub rect: Rect,
    pub open: bool,
    pub depth: u32,
    pub kind: NodeKind,
    pub sub: SubKind,
    pub flags: NodeFlags,
    pub mark: Mark,
    pub dimmed: bool,
    pub tag_color: Option<[u8; 3]>,
    pub hidden_details: usize,
    pub shown_details: usize,
    pub source: bool,
    pub rows: Vec<CardRow>,
    pub more_rows: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SceneEdge {
    pub depth: u32,
    pub a: NodeId,
    pub b: NodeId,
    pub points: Vec<[f32; 2]>,
    pub dir: Direction,
    pub undeclared: bool,
    pub test_only: bool,
    pub mark: Mark,
    pub dimmed: bool,
    pub weight: u32,
    pub links: u32,
    pub wiring: u32,
    pub implements: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SceneFrame {
    pub depth: u32,
    pub label: String,
    pub rect: Rect,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SceneLabel {
    pub depth: u32,
    pub node: NodeId,
    pub pos: [f32; 2],
    pub text: String,
    pub detail: Option<String>,
    pub size: f32,
    pub centered: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SceneStats {
    pub visited: usize,
    pub drawn: usize,
    pub layouts_computed: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Scene {
    pub boxes: Vec<SceneBox>,
    pub edges: Vec<SceneEdge>,
    pub frames: Vec<SceneFrame>,
    pub labels: Vec<SceneLabel>,
    pub stats: SceneStats,
}

impl Scene {
    pub fn box_at(&self, p: [f32; 2]) -> Option<NodeId> {
        self.boxes.iter().rev().find(|b| b.rect.contains(p)).map(|b| b.node)
    }

    pub fn row_at(&self, p: [f32; 2]) -> Option<NodeId> {
        self.boxes.iter().rev().filter(|b| !b.source).find_map(|b| b.rows.iter().find(|r| r.rect.contains(p)).map(|r| r.node))
    }

    pub fn deepest_open_at(&self, p: [f32; 2]) -> Option<NodeId> {
        self.boxes.iter().filter(|b| b.open && b.rect.contains(p)).max_by_key(|b| b.depth).map(|b| b.node)
    }
}

struct Xform {
    s: f64,
    dx: f64,
    dy: f64,
}

impl Xform {
    fn fit(layout: &ContainerLayout, world: WRect) -> Xform {
        let (w, h) = (layout.size[0] as f64, layout.size[1] as f64);
        if w <= 0.0 || h <= 0.0 {
            return Xform { s: 0.0, dx: world.x, dy: world.y };
        }
        let s = (world.w / w).min(world.h / h);
        Xform { s, dx: world.x + (world.w - w * s) / 2.0, dy: world.y + (world.h - h * s) / 2.0 }
    }

    fn rect(&self, r: Rect) -> WRect {
        WRect::new(self.dx + r.x as f64 * self.s, self.dy + r.y as f64 * self.s, r.w as f64 * self.s, r.h as f64 * self.s)
    }

    fn point(&self, p: [f32; 2]) -> [f64; 2] {
        [self.dx + p[0] as f64 * self.s, self.dy + p[1] as f64 * self.s]
    }
}

pub fn root_world(cache: &mut LayoutCache, g: &ArchivedGraph) -> WRect {
    let size = cache.shared(g, g.root()).size;
    WRect::new(0.0, 0.0, size[0] as f64, size[1] as f64)
}

pub fn world_rect(g: &ArchivedGraph, cache: &mut LayoutCache, node: NodeId) -> Option<WRect> {
    let path = display_path(g, node);
    let mut world = root_world(cache, g);
    for pair in path.windows(2) {
        let layout = cache.shared(g, pair[0]);
        let child = layout.child(pair[1])?;
        world = Xform::fit(&layout, world).rect(child.rect);
    }
    Some(world)
}

pub fn fit_label(name: &str, width: f32, size: f32) -> String {
    let char_w = size * 0.5;
    let fits = |s: &str| s.chars().count() as f32 * char_w <= width;
    if fits(name) || !name.contains('.') {
        return name.to_string();
    }
    let segs: Vec<&str> = name.split('.').collect();
    for k in 1..segs.len() {
        let candidate = format!("…{}", segs[k..].join("."));
        if fits(&candidate) {
            return candidate;
        }
    }
    format!("…{}", segs[segs.len() - 1])
}

fn summary(g: &ArchivedGraph, node: NodeId) -> String {
    let kids = g.children(node).filter(|&c| !g.flags(c).intersects(NodeFlags::MOCK | NodeFlags::TEST | NodeFlags::EXTERNAL | NodeFlags::WIRING)).count();
    let unit = match g.kind(node) {
        NodeKind::Module => "pkgs",
        NodeKind::Package => "types",
        NodeKind::Type => "members",
        _ => "",
    };
    let lines = g.subtree_lines(node);
    let loc = if lines >= 1000 { format!("{:.1}k loc", lines as f32 / 1000.0) } else { format!("{lines} loc") };
    if unit.is_empty() || kids == 0 { loc } else { format!("{kids} {unit} · {loc}") }
}

struct Builder<'a> {
    g: &'a ArchivedGraph,
    cache: &'a mut LayoutCache,
    cam: &'a Camera,
    view: Rect,
    vis: &'a VisualRules,
    marks: Option<&'a Marks>,
    anchor: [f32; 2],
    open_files: &'a [NodeId],
    scene: Scene,
}

impl Builder<'_> {
    fn mark(&self, n: NodeId) -> Mark {
        self.marks.map_or(Mark::None, |m| m.mark(n))
    }

    fn edge_mark(&self, a: NodeId, b: NodeId) -> Mark {
        match (self.mark(a), self.mark(b)) {
            (Mark::None, _) | (_, Mark::None) => Mark::None,
            (Mark::Up, _) | (_, Mark::Up) => Mark::Up,
            (_, Mark::Down) | (Mark::Down, _) => Mark::Down,
            _ => Mark::Focus,
        }
    }

    fn children_legible(&mut self, node: NodeId, rect: Rect) -> bool {
        let layout = self.cache.shared(self.g, node);
        if layout.children.is_empty() || layout.size[0] <= 0.0 {
            return false;
        }
        let scale = (rect.w / layout.size[0]).min(rect.h / layout.size[1]);
        let mean = layout.children.iter().map(|c| c.rect.w).sum::<f32>() / layout.children.len() as f32;
        mean * scale >= MIN_CHILD_PX
    }

    fn open_container(&mut self, node: NodeId, world: WRect, depth: u32) {
        let screen = self.cam.world_to_screen_rect(world, self.view);
        let legible = self.children_legible(node, screen);
        let layout = self.cache.shared(self.g, node);
        let xf = Xform::fit(&layout, world);
        for f in &layout.frames {
            let rect = self.cam.world_to_screen_rect(xf.rect(f.rect), self.view);
            if rect.intersects(&self.view) {
                self.scene.frames.push(SceneFrame { depth: depth + 1, label: f.label.clone(), rect });
            }
        }
        for e in layout.edges.iter().filter(|_| legible) {
            let points: Vec<[f32; 2]> = e.points.iter().map(|p| self.cam.world_to_screen(xf.point(*p), self.view)).collect();
            let bbox = points.iter().skip(1).fold(Rect::new(points[0][0], points[0][1], 0.0, 0.0), |acc, p| acc.union(&Rect::new(p[0], p[1], 0.0, 0.0)));
            if !bbox.expand(2.0).intersects(&self.view) {
                continue;
            }
            self.scene.edges.push(SceneEdge {
                depth: depth + 1,
                a: e.a,
                b: e.b,
                points,
                dir: e.dir,
                undeclared: e.undeclared,
                test_only: e.test_only,
                mark: self.edge_mark(e.a, e.b),
                dimmed: self.marks.is_some() && self.edge_mark(e.a, e.b) == Mark::None,
                weight: e.calls + e.deps,
                links: e.links,
                wiring: e.wiring,
                implements: e.implements,
            });
        }
        for c in &layout.children {
            self.visit(c.node, xf.rect(c.rect), depth + 1);
        }
    }

    fn card_rows(&mut self, node: NodeId, world: WRect, open: bool) -> (Vec<CardRow>, usize) {
        let g = self.g;
        let (ox, oy, s, w) = if open {
            let layout = self.cache.shared(g, node);
            let xf = Xform::fit(&layout, world);
            (xf.dx, xf.dy, xf.s, layout.size[0] as f64 * xf.s)
        } else {
            (world.x, world.y, world.w / box_size(g, node)[0] as f64, world.w)
        };
        if (CARD_ROW as f64 * s * self.cam.zoom) < MIN_ROW_PX as f64 {
            return (Vec::new(), 0);
        }
        let opts = self.cache.options();
        let members: Vec<NodeId> = card_members(g, node).into_iter().filter(|&m| !opts.hides(g, m)).collect();
        let shown = members.len().min(CARD_MAX_ROWS);
        let rows = members
            .iter()
            .take(shown)
            .enumerate()
            .map(|(i, &m)| {
                let r = WRect::new(ox + 8.0 * s, oy + (CARD_TOP as f64 + i as f64 * CARD_ROW as f64) * s, (w - 16.0 * s).max(0.0), CARD_ROW as f64 * s);
                CardRow { node: m, rect: self.cam.world_to_screen_rect(r, self.view), mark: self.mark(m) }
            })
            .collect();
        (rows, members.len() - shown)
    }

    fn visit(&mut self, node: NodeId, world: WRect, depth: u32) {
        let rect = self.cam.world_to_screen_rect(world, self.view);
        if rect.w < MIN_DRAW_PX || !rect.intersects(&self.view) {
            return;
        }
        self.scene.stats.visited += 1;
        let g = self.g;
        let opts = self.cache.options();
        let shown: Vec<NodeId> = display_children(g, node).into_iter().filter(|&c| !opts.hides(g, c)).collect();
        let card = g.kind(node) == NodeKind::Type;
        let has_children = shown.iter().any(|&c| !(card && g.kind(c) == NodeKind::Member));
        let file_level = match g.kind(node) {
            NodeKind::Type => shown.iter().all(|&c| g.kind(c) != NodeKind::Type || g.parent(c) == Some(node)),
            NodeKind::Member => g.parent(node).is_some_and(|p| g.kind(p) == NodeKind::Package),
            _ => false,
        };
        let wide = rect.w >= FILE_VIEW_SHARE * self.view.w;
        let chosen = if self.open_files.is_empty() { rect.contains(self.anchor) } else { self.open_files.contains(&node) };
        let source = file_level && wide && chosen && g.file(node).is_some() && file_font(rect.w).is_some();
        let open = !source && has_children && rect.w >= OPEN_THRESHOLD_PX;
        let tag_color = g.tags(node).find_map(|t| self.vis.style_for(t).map(|s| s.color));
        self.scene.boxes.push(SceneBox { node, rect, open, depth, kind: g.kind(node), sub: g.sub_kind(node), flags: g.flags(node), mark: self.mark(node), dimmed: self.marks.is_some() && self.mark(node) == Mark::None, tag_color, hidden_details: 0, shown_details: 0, source, rows: Vec::new(), more_rows: 0 });
        let (rows, more_rows) = if card && !source { self.card_rows(node, world, open) } else { (Vec::new(), 0) };
        let compact = !rows.is_empty();
        let idx = self.scene.boxes.len() - 1;
        self.scene.boxes[idx].rows = rows;
        self.scene.boxes[idx].more_rows = more_rows;
        self.scene.stats.drawn += 1;
        if !source && rect.h >= MIN_LABEL_PX * 1.6 {
            let size = if open { 13.0 } else { (rect.h * 0.2).clamp(MIN_LABEL_PX, 16.0) };
            let size = match self.scene.boxes[idx].rows.first() {
                Some(r) => size.min((r.rect.y - rect.y - 4.0) * 0.6).max(MIN_LABEL_PX),
                None => size,
            };
            let (pos, centered, detail) = if compact {
                ([rect.x + 8.0, rect.y + 6.0], false, None)
            } else if open {
                ([rect.x + 8.0, rect.y + 6.0], false, Some(summary(g, node)))
            } else {
                let detail = (rect.h >= 40.0).then(|| summary(g, node));
                ([rect.x + 8.0, rect.y + 6.0], false, detail)
            };
            let text = fit_label(g.name(node), rect.w - 16.0, size);
            self.scene.labels.push(SceneLabel { depth, node, pos, text, detail, size, centered });
        }
        if open {
            let idx = self.scene.boxes.len() - 1;
            let layout = self.cache.shared(g, node);
            self.scene.boxes[idx].hidden_details = layout.hidden_details;
            self.scene.boxes[idx].shown_details = layout.shown_details;
            self.open_container(node, world, depth);
        }
    }
}

pub fn build_scene(g: &ArchivedGraph, cache: &mut LayoutCache, cam: &Camera, view: Rect, vis: &VisualRules, marks: Option<&Marks>) -> Scene {
    build_scene_at(g, cache, cam, view, vis, marks, view.center(), &[])
}

pub fn build_scene_at(g: &ArchivedGraph, cache: &mut LayoutCache, cam: &Camera, view: Rect, vis: &VisualRules, marks: Option<&Marks>, anchor: [f32; 2], open_files: &[NodeId]) -> Scene {
    let before = cache.computed();
    let world = root_world(cache, g);
    let mut b = Builder { g, cache, cam, view, vis, marks, anchor, open_files, scene: Scene::default() };
    b.open_container(g.root(), world, 0);
    let computed = b.cache.computed();
    let mut scene = b.scene;
    scene.stats.layouts_computed = computed - before;
    scene
}
