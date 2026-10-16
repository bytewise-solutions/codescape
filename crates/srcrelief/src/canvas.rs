use crate::camera::Camera;
use crate::reach::ReachSet;
use srcrelief_format::{ArchivedGraph, NodeFlags, NodeId, NodeKind};
use srcrelief_layout::{ContainerLayout, Direction, LayoutCache, Rect};
use srcrelief_rules::VisualRules;

pub const OPEN_THRESHOLD_PX: f32 = 220.0;
pub const MIN_DRAW_PX: f32 = 3.0;
pub const MIN_LABEL_PX: f32 = 7.0;

#[derive(Clone, Debug, PartialEq)]
pub struct SceneBox {
    pub node: NodeId,
    pub rect: Rect,
    pub open: bool,
    pub depth: u32,
    pub kind: NodeKind,
    pub flags: NodeFlags,
    pub highlighted: bool,
    pub tag_color: Option<[u8; 3]>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SceneEdge {
    pub a: NodeId,
    pub b: NodeId,
    pub points: Vec<[f32; 2]>,
    pub dir: Direction,
    pub undeclared: bool,
    pub test_only: bool,
    pub highlighted: bool,
    pub weight: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SceneFrame {
    pub label: String,
    pub rect: Rect,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SceneLabel {
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

    pub fn deepest_open_at(&self, p: [f32; 2]) -> Option<NodeId> {
        self.boxes.iter().filter(|b| b.open && b.rect.contains(p)).max_by_key(|b| b.depth).map(|b| b.node)
    }
}

struct Xform {
    s: f32,
    dx: f32,
    dy: f32,
}

impl Xform {
    fn fit(layout: &ContainerLayout, world: Rect) -> Xform {
        let [w, h] = layout.size;
        if w <= 0.0 || h <= 0.0 {
            return Xform { s: 0.0, dx: world.x, dy: world.y };
        }
        let s = (world.w / w).min(world.h / h);
        Xform { s, dx: world.x + (world.w - w * s) / 2.0, dy: world.y + (world.h - h * s) / 2.0 }
    }

    fn rect(&self, r: Rect) -> Rect {
        Rect::new(self.dx + r.x * self.s, self.dy + r.y * self.s, r.w * self.s, r.h * self.s)
    }

    fn point(&self, p: [f32; 2]) -> [f32; 2] {
        [self.dx + p[0] * self.s, self.dy + p[1] * self.s]
    }
}

pub fn root_world(cache: &mut LayoutCache, g: &ArchivedGraph) -> Rect {
    let size = cache.shared(g, g.root()).size;
    Rect::new(0.0, 0.0, size[0], size[1])
}

pub fn world_rect(g: &ArchivedGraph, cache: &mut LayoutCache, node: NodeId) -> Option<Rect> {
    let path = g.path(node);
    let mut world = root_world(cache, g);
    for pair in path.windows(2) {
        let layout = cache.shared(g, pair[0]);
        let child = layout.child(pair[1])?;
        world = Xform::fit(&layout, world).rect(child.rect);
    }
    Some(world)
}

fn summary(g: &ArchivedGraph, node: NodeId) -> String {
    let kids = g.children(node).filter(|&c| !g.flags(c).intersects(NodeFlags::MOCK | NodeFlags::TEST | NodeFlags::EXTERNAL)).count();
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
    highlight: Option<&'a ReachSet>,
    scene: Scene,
}

impl Builder<'_> {
    fn highlighted(&self, n: NodeId) -> bool {
        self.highlight.is_some_and(|r| r.contains(n))
    }

    fn open_container(&mut self, node: NodeId, world: Rect, depth: u32) {
        let layout = self.cache.shared(self.g, node);
        let xf = Xform::fit(&layout, world);
        for f in &layout.frames {
            let rect = self.cam.world_to_screen_rect(xf.rect(f.rect), self.view);
            if rect.intersects(&self.view) {
                self.scene.frames.push(SceneFrame { label: f.label.clone(), rect });
            }
        }
        for e in &layout.edges {
            let points: Vec<[f32; 2]> = e.points.iter().map(|p| self.cam.world_to_screen(xf.point(*p), self.view)).collect();
            let bbox = points.iter().skip(1).fold(Rect::new(points[0][0], points[0][1], 0.0, 0.0), |acc, p| acc.union(&Rect::new(p[0], p[1], 0.0, 0.0)));
            if !bbox.expand(2.0).intersects(&self.view) {
                continue;
            }
            self.scene.edges.push(SceneEdge {
                a: e.a,
                b: e.b,
                points,
                dir: e.dir,
                undeclared: e.undeclared,
                test_only: e.test_only,
                highlighted: self.highlighted(e.a) && self.highlighted(e.b),
                weight: e.calls + e.deps,
            });
        }
        for c in &layout.children {
            self.visit(c.node, xf.rect(c.rect), depth + 1);
        }
    }

    fn visit(&mut self, node: NodeId, world: Rect, depth: u32) {
        let rect = self.cam.world_to_screen_rect(world, self.view);
        if rect.w < MIN_DRAW_PX || !rect.intersects(&self.view) {
            return;
        }
        self.scene.stats.visited += 1;
        let g = self.g;
        let hidden = self.cache.options().hidden;
        let has_children = g.children(node).any(|c| !g.flags(c).intersects(hidden));
        let open = has_children && rect.w >= OPEN_THRESHOLD_PX;
        let tag_color = g.tags(node).find_map(|t| self.vis.style_for(t).map(|s| s.color));
        self.scene.boxes.push(SceneBox { node, rect, open, depth, kind: g.kind(node), flags: g.flags(node), highlighted: self.highlighted(node), tag_color });
        self.scene.stats.drawn += 1;
        if rect.h >= MIN_LABEL_PX * 1.6 {
            let size = if open { 13.0 } else { (rect.h * 0.2).clamp(MIN_LABEL_PX, 16.0) };
            let (pos, centered, detail) = if open {
                ([rect.x + 8.0, rect.y + 6.0], false, Some(summary(g, node)))
            } else {
                let detail = (rect.h >= 40.0).then(|| summary(g, node));
                ([rect.x + 8.0, rect.y + 6.0], false, detail)
            };
            self.scene.labels.push(SceneLabel { node, pos, text: g.name(node).to_string(), detail, size, centered });
        }
        if open {
            self.open_container(node, world, depth);
        }
    }
}

pub fn build_scene(g: &ArchivedGraph, cache: &mut LayoutCache, cam: &Camera, view: Rect, vis: &VisualRules, highlight: Option<&ReachSet>) -> Scene {
    let before = cache.computed();
    let world = root_world(cache, g);
    let mut b = Builder { g, cache, cam, view, vis, highlight, scene: Scene::default() };
    b.open_container(g.root(), world, 0);
    let computed = b.cache.computed();
    let mut scene = b.scene;
    scene.stats.layouts_computed = computed - before;
    scene
}
