use codescape_format::{ArchivedGraph, EdgeFlags, EdgeKind, NodeId, Scope};
use codescape_layout::LayoutOptions;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CallLink {
    pub from: NodeId,
    pub to: NodeId,
    pub line: Option<u32>,
}

pub fn calls_between(g: &ArchivedGraph, a: NodeId, b: NodeId, opts: &LayoutOptions) -> Vec<CallLink> {
    let mut out = Vec::new();
    let mut stack = vec![a];
    while let Some(from) = stack.pop() {
        stack.extend(g.children(from));
        for e in g.out_edges(from) {
            let to = e.to.to_native();
            let main = Scope::from_u8(e.scope) == Scope::Main && !EdgeFlags(e.flags).contains(EdgeFlags::NOISE);
            if main && EdgeKind::from_u8(e.kind).is_call_like() && g.is_ancestor(b, to) && !opts.hides_logging(g, to) {
                out.push(CallLink { from, to, line: g.call_line(from, e) });
            }
        }
    }
    out.sort_by(|x, y| g.name(x.from).cmp(g.name(y.from)).then(x.line.cmp(&y.line)).then(x.from.cmp(&y.from)).then(x.to.cmp(&y.to)));
    out
}
