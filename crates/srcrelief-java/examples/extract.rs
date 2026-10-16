use srcrelief_format::*;

fn main() {
    let dir = std::env::args().nth(1).expect("usage: extract <dir>");
    let t = std::time::Instant::now();
    let ex = srcrelief_build::default_excludes();
    let d = srcrelief_build::discover(std::path::Path::new(&dir), &ex).unwrap();
    let mut b = GraphBuilder::new("x");
    let nodes: Vec<NodeId> = d.modules.iter().map(|m| b.add_node(b.root(), NodeKind::Module, SubKind::None, &m.name, m.lang)).collect();
    let mut facts = srcrelief_facts::ProjectFacts::default();
    let s = srcrelief_java::extract_java(&d, &nodes, &ex, &mut b, &mut facts);
    let mains: usize = facts.files.iter().map(|f| f.functions.iter().filter(|x| x.is_main).count()).sum();
    let ext: usize = facts.files.iter().map(|f| f.functions.iter().filter(|x| x.overrides_external.is_some()).count()).sum();
    println!("{s:?}\nnodes={} edges={} mains={mains} external-overrides={ext} in {:?}", b.node_count(), b.edges().len(), t.elapsed());
}
