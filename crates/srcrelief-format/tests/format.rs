use srcrelief_format::*;
use std::path::PathBuf;

fn roundtrip(b: GraphBuilder) -> (tempfile::TempDir, ReliefFile) {
    let dir = tempfile::tempdir().unwrap();
    let path: PathBuf = dir.path().join("g.relief");
    write_graph(&b.finish(), &path).unwrap();
    let file = ReliefFile::open(&path).unwrap();
    (dir, file)
}

fn find(g: &ArchivedGraph, parent: NodeId, name: &str) -> NodeId {
    g.children(parent).find(|&c| g.name(c) == name).unwrap_or_else(|| panic!("no child {name}"))
}

#[test]
fn children_are_contiguous_after_finish() {
    let mut b = GraphBuilder::new("proj");
    let root = b.root();
    let m1 = b.add_node(root, NodeKind::Module, SubKind::None, "m1", Lang::Java);
    let m2 = b.add_node(root, NodeKind::Module, SubKind::None, "m2", Lang::Java);
    for i in 0..3 {
        b.add_node(m1, NodeKind::Package, SubKind::None, &format!("a{i}"), Lang::Java);
        b.add_node(m2, NodeKind::Package, SubKind::None, &format!("b{i}"), Lang::Java);
    }
    let (_d, f) = roundtrip(b);
    let g = f.graph();
    assert_eq!(g.name(g.root()), "proj");
    let m1 = find(g, g.root(), "m1");
    let m2 = find(g, g.root(), "m2");
    let names: Vec<&str> = g.children(m1).map(|c| g.name(c)).collect();
    assert_eq!(names, ["a0", "a1", "a2"]);
    let names: Vec<&str> = g.children(m2).map(|c| g.name(c)).collect();
    assert_eq!(names, ["b0", "b1", "b2"]);
    for c in g.children(m2) {
        assert_eq!(g.parent(c), Some(m2));
    }
    assert_eq!(g.parent(g.root()), None);
}

fn two_module_graph() -> (GraphBuilder, [NodeId; 6]) {
    let mut b = GraphBuilder::new("proj");
    let root = b.root();
    let m1 = b.add_node(root, NodeKind::Module, SubKind::None, "m1", Lang::Java);
    let m2 = b.add_node(root, NodeKind::Module, SubKind::None, "m2", Lang::Java);
    let p1 = b.add_node(m1, NodeKind::Package, SubKind::None, "p1", Lang::Java);
    let p2 = b.add_node(m1, NodeKind::Package, SubKind::None, "p2", Lang::Java);
    let p3 = b.add_node(m2, NodeKind::Package, SubKind::None, "p3", Lang::Java);
    let t = b.add_node(p1, NodeKind::Type, SubKind::Class, "T", Lang::Java);
    let f = b.add_node(t, NodeKind::Member, SubKind::Method, "f", Lang::Java);
    let u = b.add_node(p3, NodeKind::Type, SubKind::Class, "U", Lang::Java);
    let g = b.add_node(u, NodeKind::Member, SubKind::Method, "g", Lang::Java);
    let v = b.add_node(p2, NodeKind::Type, SubKind::Class, "V", Lang::Java);
    let h = b.add_node(v, NodeKind::Member, SubKind::Method, "h", Lang::Java);
    (b, [m1, m2, p1, f, g, h])
}

#[test]
fn aggregates_live_at_lowest_common_ancestor() {
    let (mut b, [_, _, _, f, g, h]) = two_module_graph();
    b.add_edge(f, g, EdgeKind::Calls, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    b.add_edge(f, h, EdgeKind::Calls, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    let (_d, file) = roundtrip(b);
    let gr = file.graph();
    let m1 = find(gr, gr.root(), "m1");
    let m2 = find(gr, gr.root(), "m2");
    let root_aggs = gr.aggregates(gr.root());
    assert_eq!(root_aggs.len(), 1);
    assert_eq!((root_aggs[0].a.to_native(), root_aggs[0].b.to_native()), (m1, m2));
    assert_eq!(root_aggs[0].calls.to_native(), 1);
    let p1 = find(gr, m1, "p1");
    let p2 = find(gr, m1, "p2");
    let m1_aggs = gr.aggregates(m1);
    assert_eq!(m1_aggs.len(), 1);
    assert_eq!((m1_aggs[0].a.to_native(), m1_aggs[0].b.to_native()), (p1, p2));
    assert!(gr.aggregates(m2).is_empty());
}

#[test]
fn test_scope_edges_are_masked() {
    let (mut b, [m1, m2, ..]) = two_module_graph();
    b.add_edge(m1, m2, EdgeKind::DependsOn, Scope::Test, Confidence::Resolved, EdgeFlags::NONE);
    let (_d, file) = roundtrip(b);
    let g = file.graph();
    let aggs = g.aggregates(g.root());
    assert_eq!(aggs.len(), 1);
    assert_eq!(aggs[0].scope_mask, SCOPE_TEST);
    assert_eq!(aggs[0].deps.to_native(), 1);
}

#[test]
fn duplicate_edges_merge_counts() {
    let (mut b, [_, _, _, f, g, _]) = two_module_graph();
    for _ in 0..3 {
        b.add_edge(f, g, EdgeKind::Calls, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    }
    let (_d, file) = roundtrip(b);
    let gr = file.graph();
    assert_eq!(gr.edge_count(), 1);
    assert_eq!(gr.aggregates(gr.root())[0].calls.to_native(), 3);
}

#[test]
fn undeclared_flag_reaches_aggregate() {
    let (mut b, [_, _, _, f, g, _]) = two_module_graph();
    b.add_edge(f, g, EdgeKind::DependsOn, Scope::Main, Confidence::Resolved, EdgeFlags::UNDECLARED);
    let (_d, file) = roundtrip(b);
    let gr = file.graph();
    assert!(gr.aggregates(gr.root())[0].undeclared);
}

#[test]
fn tags_entries_spans_roundtrip() {
    let (mut b, [m1, _, p1, f, ..]) = two_module_graph();
    b.add_tag(p1, "layer:repository");
    b.add_tag(p1, "noise:logging");
    b.set_span(f, "m1/src/T.java", 3, 9);
    b.set_lines(f, 7);
    b.add_flags(m1, NodeFlags::TEST);
    let main = b.add_entry(EntryPointSpec {
        node: f,
        kind: "main".into(),
        category: Category::Application,
        label: "T.main".into(),
        parent: None,
        lifecycle: Lifecycle::Once,
        order: 0,
        rule: "java-core.main".into(),
        handoff: Some("libGDX".into()),
    });
    b.add_entry(EntryPointSpec {
        node: f,
        kind: "framework-callback".into(),
        category: Category::Application,
        label: "render".into(),
        parent: Some(main),
        lifecycle: Lifecycle::Repeating,
        order: 2,
        rule: "libgdx.callbacks".into(),
        handoff: None,
    });
    let (_d, file) = roundtrip(b);
    let g = file.graph();
    let m1 = find(g, g.root(), "m1");
    let p1 = find(g, m1, "p1");
    let tags: Vec<&str> = g.tags(p1).collect();
    assert_eq!(tags, ["layer:repository", "noise:logging"]);
    assert!(g.flags(m1).contains(NodeFlags::TEST));
    let t = find(g, p1, "T");
    let f = find(g, t, "f");
    assert_eq!(g.file(f), Some("m1/src/T.java"));
    assert_eq!(g.lines(f), 7);
    assert_eq!(g.entries().len(), 2);
    let e = &g.entries()[1];
    assert_eq!(e.node.to_native(), f);
    assert_eq!(g.str(e.label), "render");
    assert_eq!(e.parent.to_native(), 0);
    assert_eq!(e.lifecycle, Lifecycle::Repeating as u8);
    assert_eq!(g.entry_handoff(0), Some("libGDX"));
    assert_eq!(g.subtree_lines(m1), 7);
}

#[test]
fn open_rejects_wrong_version_and_truncation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("g.relief");
    let (b, _) = two_module_graph();
    write_graph(&b.finish(), &path).unwrap();
    let mut bytes = std::fs::read(&path).unwrap();

    let mut bumped = bytes.clone();
    bumped[8] = bumped[8].wrapping_add(1);
    let p2 = dir.path().join("v.relief");
    std::fs::write(&p2, &bumped).unwrap();
    assert!(matches!(ReliefFile::open(&p2), Err(FormatError::Version { .. })));

    bytes.truncate(bytes.len() / 2);
    let p3 = dir.path().join("t.relief");
    std::fs::write(&p3, &bytes).unwrap();
    assert!(matches!(ReliefFile::open(&p3), Err(FormatError::Corrupt(_))));

    let p4 = dir.path().join("e.relief");
    std::fs::write(&p4, b"").unwrap();
    assert!(matches!(ReliefFile::open(&p4), Err(FormatError::Corrupt(_))));
}
