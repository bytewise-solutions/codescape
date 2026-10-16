mod layered;
pub mod order;
mod size;

pub use layered::{count_crossings, layout_container};
pub use size::box_size;

use srcrelief_format::{ArchivedGraph, NodeFlags, NodeId};
use srcrelief_rules::FrameRule;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn center(&self) -> [f32; 2] {
        [self.x + self.w / 2.0, self.y + self.h / 2.0]
    }

    pub fn intersects(&self, o: &Rect) -> bool {
        self.x < o.right() && o.x < self.right() && self.y < o.bottom() && o.y < self.bottom()
    }

    pub fn contains_rect(&self, o: &Rect) -> bool {
        o.x >= self.x && o.y >= self.y && o.right() <= self.right() && o.bottom() <= self.bottom()
    }

    pub fn contains(&self, p: [f32; 2]) -> bool {
        p[0] >= self.x && p[0] <= self.right() && p[1] >= self.y && p[1] <= self.bottom()
    }

    pub fn union(&self, o: &Rect) -> Rect {
        let x = self.x.min(o.x);
        let y = self.y.min(o.y);
        Rect::new(x, y, self.right().max(o.right()) - x, self.bottom().max(o.bottom()) - y)
    }

    pub fn expand(&self, by: f32) -> Rect {
        Rect::new(self.x - by, self.y - by, self.w + 2.0 * by, self.h + 2.0 * by)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Down,
    Up,
    Sideways,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutOptions {
    pub include_test_edges: bool,
    pub hidden: NodeFlags,
    pub show_wiring: bool,
}

impl Default for LayoutOptions {
    fn default() -> Self {
        LayoutOptions { include_test_edges: false, hidden: NodeFlags::EXTERNAL | NodeFlags::MOCK | NodeFlags::TEST | NodeFlags::WIRING, show_wiring: false }
    }
}

impl LayoutOptions {
    pub fn effective_hidden(&self) -> NodeFlags {
        if self.show_wiring { NodeFlags(self.hidden.0 & !NodeFlags::WIRING.0) } else { self.hidden }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChildBox {
    pub node: NodeId,
    pub rect: Rect,
    pub layer: u32,
    pub pinned: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LaidEdge {
    pub a: NodeId,
    pub b: NodeId,
    pub calls: u32,
    pub deps: u32,
    pub wiring: u32,
    pub implements: u32,
    pub dir: Direction,
    pub undeclared: bool,
    pub test_only: bool,
    pub points: Vec<[f32; 2]>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FrameBox {
    pub label: String,
    pub rect: Rect,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct ContainerLayout {
    pub size: [f32; 2],
    pub header: f32,
    pub children: Vec<ChildBox>,
    pub edges: Vec<LaidEdge>,
    pub frames: Vec<FrameBox>,
}

impl ContainerLayout {
    pub fn child(&self, node: NodeId) -> Option<&ChildBox> {
        self.children.iter().find(|c| c.node == node)
    }

    pub fn up_edges(&self) -> usize {
        self.edges.iter().filter(|e| e.dir == Direction::Up).count()
    }
}

pub struct LayoutCache {
    frames: Vec<FrameRule>,
    opts: LayoutOptions,
    map: HashMap<NodeId, Rc<ContainerLayout>>,
    computed: usize,
}

impl LayoutCache {
    pub fn new(frames: Vec<FrameRule>, opts: LayoutOptions) -> LayoutCache {
        LayoutCache { frames, opts, map: HashMap::new(), computed: 0 }
    }

    pub fn options(&self) -> &LayoutOptions {
        &self.opts
    }

    pub fn set_options(&mut self, opts: LayoutOptions) {
        if opts != self.opts {
            self.opts = opts;
            self.invalidate_all();
        }
    }

    pub fn get(&self, node: NodeId) -> Option<&ContainerLayout> {
        self.map.get(&node).map(|l| l.as_ref())
    }

    pub fn get_or_compute(&mut self, g: &ArchivedGraph, node: NodeId) -> &ContainerLayout {
        self.shared(g, node);
        &self.map[&node]
    }

    pub fn shared(&mut self, g: &ArchivedGraph, node: NodeId) -> Rc<ContainerLayout> {
        if let Some(l) = self.map.get(&node) {
            return l.clone();
        }
        let l = Rc::new(layout_container(g, node, &self.frames, &self.opts));
        self.computed += 1;
        self.map.insert(node, l.clone());
        l
    }

    pub fn computed(&self) -> usize {
        self.computed
    }

    pub fn invalidate_all(&mut self) {
        self.map.clear();
    }
}
