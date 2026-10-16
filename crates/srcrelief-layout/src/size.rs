use srcrelief_format::{ArchivedGraph, NodeId};

pub fn box_size(g: &ArchivedGraph, node: NodeId) -> [f32; 2] {
    let lines = g.subtree_lines(node) as f32;
    let w = (160.0 + 60.0 * (1.0 + lines / 200.0).log2()).max(120.0);
    [w, (w / 1.6).max(60.0)]
}
