use srcrelief_format::{ArchivedGraph, NodeId, NodeKind, SubKind};

fn is_interface(g: &ArchivedGraph, n: NodeId) -> bool {
    g.kind(n) == NodeKind::Type && g.sub_kind(n) == SubKind::Interface
}

pub fn folded_into(g: &ArchivedGraph, node: NodeId) -> Option<NodeId> {
    if g.kind(node) != NodeKind::Type || is_interface(g, node) {
        return None;
    }
    let parent = g.parent(node)?;
    g.aggregates(parent)
        .iter()
        .filter(|a| a.a.to_native() == node && a.implements.to_native() > 0)
        .map(|a| a.b.to_native())
        .filter(|&b| is_interface(g, b))
        .min()
}

pub fn display_children(g: &ArchivedGraph, node: NodeId) -> Vec<NodeId> {
    let mut out: Vec<NodeId> = g.children(node).filter(|&c| folded_into(g, c).is_none()).collect();
    if is_interface(g, node) {
        if let Some(p) = g.parent(node) {
            out.extend(g.children(p).filter(|&c| folded_into(g, c) == Some(node)));
        }
    }
    out
}

pub fn display_path(g: &ArchivedGraph, node: NodeId) -> Vec<NodeId> {
    let mut out = Vec::new();
    for n in g.path(node) {
        if let Some(i) = folded_into(g, n) {
            out.push(i);
        }
        out.push(n);
    }
    out
}
