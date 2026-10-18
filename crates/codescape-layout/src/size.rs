use crate::LayoutOptions;
use codescape_format::{ArchivedGraph, NodeFlags, NodeId, NodeKind, SubKind, Visibility};

pub const CARD_TOP: f32 = 34.0;
pub const CARD_ROW: f32 = 13.0;
pub const CARD_MAX_ROWS: usize = 14;
pub const CARD_MAX_FIELDS: usize = 8;
pub const CARD_FIELD_GAP: f32 = 6.0;

pub fn card_field_rows(g: &ArchivedGraph, node: NodeId, opts: &LayoutOptions) -> (usize, usize) {
    let n = if g.kind(node) == NodeKind::Type && opts.card.fields {
        g.fields(node).filter(|f| opts.card.private || f.visibility != Visibility::Private).count()
    } else {
        0
    };
    let shown = n.min(CARD_MAX_FIELDS);
    (shown, n - shown)
}

pub fn card_methods_top(g: &ArchivedGraph, node: NodeId, opts: &LayoutOptions) -> f32 {
    let (shown, more) = card_field_rows(g, node, opts);
    let rows = shown + usize::from(more > 0);
    CARD_TOP + if rows > 0 { rows as f32 * CARD_ROW + CARD_FIELD_GAP } else { 0.0 }
}

pub fn card_members(g: &ArchivedGraph, node: NodeId, opts: &LayoutOptions) -> Vec<NodeId> {
    if g.kind(node) != NodeKind::Type {
        return Vec::new();
    }
    let card = opts.card;
    let mut members: Vec<NodeId> = g
        .children(node)
        .filter(|&c| g.kind(c) == NodeKind::Member && !g.flags(c).intersects(NodeFlags::TEST | NodeFlags::MOCK | NodeFlags::WIRING))
        .filter(|&c| !opts.hidden_nodes.contains(&c))
        .filter(|&c| card.private || g.visibility(c) != Visibility::Private)
        .filter(|&c| card.accessors || !g.has_tag(c, "accessor"))
        .filter(|&c| card.constructors || !is_constructor(g, c))
        .collect();
    members.sort_by_key(|&m| (!is_constructor(g, m), g.span(m).0, m));
    members
}

pub fn is_constructor(g: &ArchivedGraph, member: NodeId) -> bool {
    g.sub_kind(member) == SubKind::Constructor || g.name(member) == "<init>"
}

pub fn card_rows_bottom(g: &ArchivedGraph, node: NodeId, opts: &LayoutOptions) -> f32 {
    let n = card_members(g, node, opts).len();
    let rows = n.min(CARD_MAX_ROWS) + usize::from(n > CARD_MAX_ROWS);
    card_methods_top(g, node, opts) + rows as f32 * CARD_ROW
}

pub fn box_size(g: &ArchivedGraph, node: NodeId, opts: &LayoutOptions) -> [f32; 2] {
    let lines = g.subtree_lines(node) as f32;
    let w = (160.0 + 60.0 * (1.0 + lines / 200.0).log2()).max(120.0);
    let h = (w / 1.6).max(60.0);
    if g.kind(node) == NodeKind::Type { [w, h.max(card_rows_bottom(g, node, opts) + 8.0)] } else { [w, h] }
}
