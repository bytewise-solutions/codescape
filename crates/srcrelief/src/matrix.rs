use srcrelief_format::NodeId;
use srcrelief_layout::{ContainerLayout, Direction, Rect};
use std::collections::HashMap;

pub const MATRIX_MIN_CHILDREN: usize = 12;
pub const MATRIX_UNTIL_CHILD_PX: f32 = 140.0;
const HEADER: f32 = 36.0;
const PAD: f32 = 8.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell {
    pub row: usize,
    pub col: usize,
    pub links: u32,
    pub dir: Direction,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Matrix {
    pub rows: Vec<NodeId>,
    pub cells: Vec<Cell>,
    pub max_links: u32,
}

impl Matrix {
    pub fn from_layout(layout: &ContainerLayout) -> Matrix {
        let mut kids: Vec<_> = layout.children.iter().collect();
        kids.sort_by(|a, b| a.layer.cmp(&b.layer).then(a.rect.x.total_cmp(&b.rect.x)).then(a.node.cmp(&b.node)));
        let rows: Vec<NodeId> = kids.iter().map(|c| c.node).collect();
        let index: HashMap<NodeId, usize> = rows.iter().enumerate().map(|(i, &n)| (n, i)).collect();
        let mut merged: HashMap<(usize, usize), Cell> = HashMap::new();
        for e in layout.edges.iter().filter(|e| e.calls + e.deps + e.implements > 0) {
            let (Some(&row), Some(&col)) = (index.get(&e.a), index.get(&e.b)) else { continue };
            let cell = merged.entry((row, col)).or_insert(Cell { row, col, links: 0, dir: e.dir });
            cell.links += e.links.max(1);
        }
        let mut cells: Vec<Cell> = merged.into_values().collect();
        cells.sort_by_key(|c| (c.row, c.col));
        let max_links = cells.iter().map(|c| c.links).max().unwrap_or(0);
        Matrix { rows, cells, max_links }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Row(usize),
    Cell(usize, usize),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    pub label_x: f32,
    pub label_w: f32,
    pub x0: f32,
    pub y0: f32,
    pub cell: f32,
    pub n: usize,
}

impl Grid {
    pub fn new(rect: Rect, n: usize) -> Grid {
        let label_w = (rect.w * 0.35).clamp(60.0, 280.0);
        let n_f = n.max(1) as f32;
        let cell = ((rect.w - label_w - 2.0 * PAD) / n_f).min((rect.h - HEADER - PAD) / n_f).max(1.0);
        Grid { label_x: rect.x + PAD, label_w, x0: rect.x + PAD + label_w, y0: rect.y + HEADER, cell, n }
    }

    pub fn cell_rect(&self, row: usize, col: usize) -> Rect {
        Rect::new(self.x0 + col as f32 * self.cell, self.y0 + row as f32 * self.cell, self.cell, self.cell)
    }

    pub fn row_rect(&self, row: usize) -> Rect {
        Rect::new(self.label_x, self.y0 + row as f32 * self.cell, self.label_w, self.cell)
    }

    pub fn hit(&self, p: [f32; 2]) -> Option<Hit> {
        let span = self.n as f32 * self.cell;
        if p[1] < self.y0 || p[1] >= self.y0 + span {
            return None;
        }
        let row = ((p[1] - self.y0) / self.cell) as usize;
        if p[0] >= self.label_x && p[0] < self.x0 {
            return Some(Hit::Row(row));
        }
        if p[0] >= self.x0 && p[0] < self.x0 + span {
            return Some(Hit::Cell(row, ((p[0] - self.x0) / self.cell) as usize));
        }
        None
    }
}

fn prefix(name: &str, k: usize) -> Option<&str> {
    name.match_indices(['.', '/']).nth(k).map(|(i, _)| &name[..=i])
}

pub fn short_names(names: &[&str]) -> Vec<String> {
    let mut keep = 0;
    if names.len() > 1 {
        while let Some(p) = prefix(names[0], keep) {
            if !names.iter().all(|n| prefix(n, keep) == Some(p)) {
                break;
            }
            keep += 1;
        }
    }
    let cut = |n: &str| if keep == 0 { 0 } else { prefix(n, keep - 1).map_or(0, |p| p.len()) };
    names.iter().map(|n| n[cut(n)..].to_string()).collect()
}
