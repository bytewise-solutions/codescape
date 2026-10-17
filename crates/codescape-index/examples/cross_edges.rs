use codescape_format::*;

fn main() {
    let mut args = std::env::args().skip(1);
    let (path, from, to) = (args.next().unwrap(), args.next().unwrap(), args.next().unwrap());
    let file = IndexFile::open(std::path::Path::new(&path)).unwrap();
    let g = file.graph();
    let m = |n: &str| g.children(g.root()).find(|&c| g.name(c) == n).unwrap();
    let (a, b) = (m(&from), m(&to));
    let mut shown = 0;
    for n in 0..g.node_count() as NodeId {
        if !g.is_ancestor(a, n) {
            continue;
        }
        for e in g.out_edges(n) {
            let t = e.to.to_native();
            if g.is_ancestor(b, t) && shown < 15 {
                shown += 1;
                let p = |x: NodeId| g.path(x).iter().skip(1).map(|&y| g.name(y)).collect::<Vec<_>>().join(" › ");
                println!("{:?} {:?} {} -> {}", EdgeKind::from_u8(e.kind), Confidence::from_u8(e.confidence), p(n), p(t));
            }
        }
    }
}
