use srcrelief::search::SearchIndex;
use srcrelief_format::*;

#[test]
fn search_ranks_exact_type_name_first() {
    let mut b = GraphBuilder::new("p");
    let m = b.add_node(b.root(), NodeKind::Module, SubKind::None, "client", Lang::Java);
    let p = b.add_node(m, NodeKind::Package, SubKind::None, "com.x.screens", Lang::Java);
    for name in ["ViewManagerImpl", "ViewManager", "ViewManagerListener", "Unrelated"] {
        b.add_node(p, NodeKind::Type, SubKind::Class, name, Lang::Java);
    }
    let dir = tempfile::tempdir().unwrap();
    write_graph(&b.finish(), &dir.path().join("g")).unwrap();
    let f = ReliefFile::open(&dir.path().join("g")).unwrap();
    let g = f.graph();
    let idx = SearchIndex::new(g);
    let hits = idx.query(g, "ViewManager", 10);
    assert_eq!(g.name(hits[0]), "ViewManager");
    assert!(hits.iter().all(|&h| g.name(h) != "Unrelated"));
    assert_eq!(hits.len(), 3);
    let fuzzy = idx.query(g, "scrmgrimpl", 5);
    assert_eq!(g.name(fuzzy[0]), "ViewManagerImpl");
    assert!(idx.query(g, "", 5).is_empty());
}
