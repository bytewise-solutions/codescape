use codescape_format::*;
use std::path::PathBuf;

fn roundtrip(b: GraphBuilder) -> (tempfile::TempDir, IndexFile) {
    let dir = tempfile::tempdir().unwrap();
    let path: PathBuf = dir.path().join("g.codescape");
    write_graph(&b.finish(), &path).unwrap();
    let file = IndexFile::open(&path).unwrap();
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
    assert!(gr.aggregates(gr.root())[0].undeclared());
    assert!(!gr.aggregates(gr.root())[0].guessed());
}

#[test]
fn an_arrow_made_only_of_guessed_calls_is_marked_guessed() {
    let (mut b, [_, _, _, f, g, h]) = two_module_graph();
    b.add_edge(f, g, EdgeKind::Calls, Scope::Main, Confidence::Guessed, EdgeFlags::NONE);
    b.add_edge(f, h, EdgeKind::Calls, Scope::Main, Confidence::Guessed, EdgeFlags::NONE);
    b.add_edge(f, h, EdgeKind::Calls, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    let (_d, file) = roundtrip(b);
    let gr = file.graph();
    let agg = |container: NodeId, to: &str| gr.aggregates(container).iter().find(|a| gr.name(a.b.to_native()) == to).unwrap();
    let root = gr.root();
    let m1 = find(gr, root, "m1");
    assert!(agg(root, "m2").guessed(), "every call from m1 to m2 is a guess");
    assert!(!agg(m1, "p2").guessed(), "p1 to p2 has a resolved call");
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
    let path = dir.path().join("g.codescape");
    let (b, _) = two_module_graph();
    write_graph(&b.finish(), &path).unwrap();
    let mut bytes = std::fs::read(&path).unwrap();

    let mut bumped = bytes.clone();
    bumped[8] = bumped[8].wrapping_add(1);
    let p2 = dir.path().join("v.codescape");
    std::fs::write(&p2, &bumped).unwrap();
    assert!(matches!(IndexFile::open(&p2), Err(FormatError::Version { .. })));

    bytes.truncate(bytes.len() / 2);
    let p3 = dir.path().join("t.codescape");
    std::fs::write(&p3, &bytes).unwrap();
    assert!(matches!(IndexFile::open(&p3), Err(FormatError::Corrupt(_))));

    let p4 = dir.path().join("e.codescape");
    std::fs::write(&p4, b"").unwrap();
    assert!(matches!(IndexFile::open(&p4), Err(FormatError::Corrupt(_))));
}

#[test]
fn records_are_compact() {
    assert!(std::mem::size_of::<ArchivedNode>() <= 32, "node is {} bytes", std::mem::size_of::<ArchivedNode>());
    assert!(std::mem::size_of::<ArchivedEdge>() <= 12, "edge is {} bytes", std::mem::size_of::<ArchivedEdge>());
    assert!(std::mem::size_of::<ArchivedAgg>() <= 20, "aggregate is {} bytes", std::mem::size_of::<ArchivedAgg>());
}

#[test]
fn leaf_and_childless_ranges_are_empty() {
    let (b, [_, _, _, f, ..]) = two_module_graph();
    let _ = f;
    let (_d, file) = roundtrip(b);
    let g = file.graph();
    let m1 = find(g, g.root(), "m1");
    let p1 = find(g, m1, "p1");
    let t = find(g, p1, "T");
    let f = find(g, t, "f");
    assert!(g.children(f).is_empty());
    assert!(g.aggregates(f).is_empty());
    assert_eq!(g.tags(f).count(), 0);
    assert!(g.out_edges(f).is_empty());
    let last = (g.node_count() - 1) as NodeId;
    assert!(g.children(last).is_empty());
}

#[test]
fn any_flipped_byte_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("g.codescape");
    let (mut b, [_, _, p1, f, g, _]) = two_module_graph();
    b.add_edge(f, g, EdgeKind::Calls, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    b.add_tag(p1, "t");
    write_graph(&b.finish(), &path).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let flipped = dir.path().join("f.codescape");
    for i in 8..bytes.len() {
        for bit in [0u8, 7] {
            let mut copy = bytes.clone();
            copy[i] ^= 1 << bit;
            std::fs::write(&flipped, &copy).unwrap();
            assert!(IndexFile::open(&flipped).is_err(), "flip at byte {i} bit {bit} was accepted");
        }
    }
}

#[test]
fn wiring_edges_count_apart() {
    let (mut b, [_, _, _, f, g, _]) = two_module_graph();
    b.add_edge(f, g, EdgeKind::Calls, Scope::Main, Confidence::Resolved, EdgeFlags::WIRING);
    let (_d, file) = roundtrip(b);
    let gr = file.graph();
    let a = &gr.aggregates(gr.root())[0];
    assert_eq!((a.calls.to_native(), a.deps.to_native(), a.wiring.to_native()), (0, 0, 1));
}

#[test]
fn implements_count_apart() {
    let (mut b, [_, _, _, f, g, _]) = two_module_graph();
    let (t, u) = (b.parent(f).unwrap(), b.parent(g).unwrap());
    b.add_edge(t, u, EdgeKind::Implements, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    b.add_edge(t, u, EdgeKind::Inherits, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    let (_d, file) = roundtrip(b);
    let gr = file.graph();
    let a = &gr.aggregates(gr.root())[0];
    assert_eq!((a.deps.to_native(), a.implements.to_native()), (0, 2));
}

#[test]
fn test_and_wiring_roll_up_to_containers() {
    let mut b = GraphBuilder::new("p");
    let m = b.add_node(b.root(), NodeKind::Module, SubKind::None, "m", Lang::Go);
    let tests = b.add_node(m, NodeKind::Package, SubKind::None, "testutil", Lang::Go);
    let t1 = b.add_node(tests, NodeKind::Member, SubKind::Function, "Helper", Lang::Go);
    b.add_flags(t1, NodeFlags::TEST);
    let wiring = b.add_node(m, NodeKind::Package, SubKind::None, "dagger", Lang::Go);
    let w1 = b.add_node(wiring, NodeKind::Type, SubKind::Interface, "Component", Lang::Go);
    let w2 = b.add_node(wiring, NodeKind::Type, SubKind::Class, "AppModule", Lang::Go);
    let wt = b.add_node(wiring, NodeKind::Type, SubKind::Class, "ModuleTest", Lang::Go);
    b.add_flags(w1, NodeFlags::WIRING);
    b.add_flags(w2, NodeFlags::WIRING);
    b.add_flags(wt, NodeFlags::TEST);
    let mixed = b.add_node(m, NodeKind::Package, SubKind::None, "app", Lang::Go);
    let real = b.add_node(mixed, NodeKind::Type, SubKind::Struct, "App", Lang::Go);
    let wired = b.add_node(mixed, NodeKind::Member, SubKind::Function, "provide", Lang::Go);
    b.add_flags(wired, NodeFlags::WIRING);
    let empty = b.add_node(m, NodeKind::Package, SubKind::None, "empty", Lang::Go);
    let _ = (real, empty);
    let (_d, file) = roundtrip(b);
    let g = file.graph();
    let m = find(g, g.root(), "m");
    assert!(g.flags(find(g, m, "testutil")).contains(NodeFlags::TEST));
    assert!(g.flags(find(g, m, "dagger")).contains(NodeFlags::WIRING));
    assert!(!g.flags(find(g, m, "app")).intersects(NodeFlags::WIRING | NodeFlags::TEST));
    assert!(!g.flags(find(g, m, "empty")).intersects(NodeFlags::WIRING | NodeFlags::TEST));
    assert!(!g.flags(m).intersects(NodeFlags::WIRING | NodeFlags::TEST));
}

#[test]
fn edges_touching_test_nodes_become_test_scope() {
    let (mut b, [_, _, _, f, g, h]) = two_module_graph();
    b.add_flags(h, NodeFlags::TEST);
    b.add_edge(f, h, EdgeKind::Calls, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    b.add_edge(f, g, EdgeKind::Calls, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    let (_d, file) = roundtrip(b);
    let gr = file.graph();
    let m1 = find(gr, gr.root(), "m1");
    assert_eq!(gr.aggregates(m1)[0].scope_mask, SCOPE_TEST);
    assert_eq!(gr.aggregates(gr.root())[0].scope_mask, SCOPE_MAIN);
}

#[test]
fn edges_touching_wiring_nodes_are_wiring() {
    let (mut b, [_, _, _, f, g, _]) = two_module_graph();
    b.add_flags(g, NodeFlags::WIRING);
    b.add_edge(f, g, EdgeKind::Calls, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    let (_d, file) = roundtrip(b);
    let gr = file.graph();
    let a = &gr.aggregates(gr.root())[0];
    assert_eq!((a.calls.to_native(), a.wiring.to_native()), (0, 1));
    let e = gr.out_edges(find(gr, find(gr, find(gr, find(gr, gr.root(), "m1"), "p1"), "T"), "f"));
    assert!(EdgeFlags(e[0].flags).contains(EdgeFlags::WIRING));
}

#[test]
fn constructor_sub_kind_roundtrips() {
    let mut b = GraphBuilder::new("p");
    let t = b.add_node(b.root(), NodeKind::Type, SubKind::Struct, "T", Lang::Go);
    b.add_node(t, NodeKind::Member, SubKind::Constructor, "NewT", Lang::Go);
    let (_d, file) = roundtrip(b);
    let g = file.graph();
    let t = find(g, g.root(), "T");
    assert_eq!(g.sub_kind(find(g, t, "NewT")), SubKind::Constructor);
}

#[test]
fn aggregate_counts_saturate() {
    let (mut b, [_, _, _, f, g, _]) = two_module_graph();
    let (t, u) = (b.parent(f).unwrap(), b.parent(g).unwrap());
    for _ in 0..70_000 {
        b.add_edge(t, u, EdgeKind::Calls, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    }
    let (_d, file) = roundtrip(b);
    let gr = file.graph();
    assert_eq!(gr.aggregates(gr.root())[0].calls.to_native(), u16::MAX);
}

#[test]
fn types_keep_their_own_visibility() {
    let mut b = GraphBuilder::new("p");
    let pkg = b.add_node(b.root(), NodeKind::Package, SubKind::None, "config", Lang::Go);
    let cfg = b.add_node(pkg, NodeKind::Type, SubKind::Struct, "Config", Lang::Go);
    let ctor = b.add_node(cfg, NodeKind::Member, SubKind::Constructor, "newConfig", Lang::Go);
    b.add_flags(ctor, NodeFlags::TEST);
    let other = b.add_node(pkg, NodeKind::Type, SubKind::Struct, "Options", Lang::Go);
    let wired = b.add_node(other, NodeKind::Member, SubKind::Constructor, "ProvideOptions", Lang::Go);
    b.add_flags(wired, NodeFlags::WIRING);
    let (_d, file) = roundtrip(b);
    let g = file.graph();
    let pkg = find(g, g.root(), "config");
    assert!(!g.flags(find(g, pkg, "Config")).intersects(NodeFlags::TEST | NodeFlags::WIRING));
    assert!(!g.flags(find(g, pkg, "Options")).intersects(NodeFlags::TEST | NodeFlags::WIRING));
}

#[test]
fn edges_touching_noise_are_set_aside() {
    let (mut b, [_, _, _, f, g, _]) = two_module_graph();
    let err = b.parent(g).unwrap();
    b.add_flags(err, NodeFlags::NOISE);
    b.add_edge(f, err, EdgeKind::Instantiates, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    b.add_edge(f, g, EdgeKind::Calls, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    let (_d, file) = roundtrip(b);
    let gr = file.graph();
    assert!(gr.aggregates(gr.root()).is_empty(), "nothing but noise links the two modules");
}

#[test]
fn packages_of_details_are_details() {
    let mut b = GraphBuilder::new("p");
    let m = b.add_node(b.root(), NodeKind::Module, SubKind::None, "common", Lang::Java);
    let bank = b.add_node(m, NodeKind::Package, SubKind::None, "messages.bank", Lang::Java);
    for (name, flags) in [("Deposit", NodeFlags::DETAIL), ("Withdraw", NodeFlags::DETAIL), ("BankTest", NodeFlags::TEST)] {
        let t = b.add_node(bank, NodeKind::Type, SubKind::Record, name, Lang::Java);
        b.add_flags(t, flags);
    }
    let mixed = b.add_node(m, NodeKind::Package, SubKind::None, "messages.chat", Lang::Java);
    let d = b.add_node(mixed, NodeKind::Type, SubKind::Record, "Line", Lang::Java);
    b.add_flags(d, NodeFlags::DETAIL);
    b.add_node(mixed, NodeKind::Type, SubKind::Class, "ChatRouter", Lang::Java);
    let only = b.add_node(b.root(), NodeKind::Module, SubKind::None, "dto-only", Lang::Java);
    let p = b.add_node(only, NodeKind::Package, SubKind::None, "dto", Lang::Java);
    let t = b.add_node(p, NodeKind::Type, SubKind::Record, "X", Lang::Java);
    b.add_flags(t, NodeFlags::DETAIL);
    let (_d, file) = roundtrip(b);
    let g = file.graph();
    let common = find(g, g.root(), "common");
    assert!(g.flags(find(g, common, "messages.bank")).contains(NodeFlags::DETAIL));
    assert!(!g.flags(find(g, common, "messages.chat")).contains(NodeFlags::DETAIL));
    let only = find(g, g.root(), "dto-only");
    assert!(g.flags(find(g, only, "dto")).contains(NodeFlags::DETAIL));
    assert!(!g.flags(only).contains(NodeFlags::DETAIL), "modules stay on the map");
}

#[test]
fn links_count_distinct_type_pairs() {
    let mut b = GraphBuilder::new("proj");
    let root = b.root();
    let m = b.add_node(root, NodeKind::Module, SubKind::None, "m", Lang::Java);
    let p1 = b.add_node(m, NodeKind::Package, SubKind::None, "p1", Lang::Java);
    let p2 = b.add_node(m, NodeKind::Package, SubKind::None, "p2", Lang::Java);
    let a = b.add_node(p1, NodeKind::Type, SubKind::Class, "A", Lang::Java);
    let a1 = b.add_node(a, NodeKind::Member, SubKind::Method, "a1", Lang::Java);
    let a2 = b.add_node(a, NodeKind::Member, SubKind::Method, "a2", Lang::Java);
    let t = b.add_node(p1, NodeKind::Type, SubKind::Class, "B", Lang::Java);
    let b1 = b.add_node(t, NodeKind::Member, SubKind::Method, "b1", Lang::Java);
    let c = b.add_node(p2, NodeKind::Type, SubKind::Class, "C", Lang::Java);
    let c1 = b.add_node(c, NodeKind::Member, SubKind::Method, "c1", Lang::Java);
    let c2 = b.add_node(c, NodeKind::Member, SubKind::Method, "c2", Lang::Java);
    let helper = b.add_node(p1, NodeKind::Member, SubKind::Function, "helper", Lang::Java);
    for (x, y) in [(a1, c1), (a2, c2), (a1, c2), (b1, c1), (helper, c1)] {
        b.add_edge(x, y, EdgeKind::Calls, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    }
    let w = b.add_node(p1, NodeKind::Type, SubKind::Class, "Module", Lang::Java);
    let w1 = b.add_node(w, NodeKind::Member, SubKind::Method, "provide", Lang::Java);
    b.add_edge(w1, c1, EdgeKind::Calls, Scope::Main, Confidence::Resolved, EdgeFlags::WIRING);
    let (_d, file) = roundtrip(b);
    let gr = file.graph();
    let m = find(gr, gr.root(), "m");
    let agg = &gr.aggregates(m)[0];
    assert_eq!(agg.calls.to_native(), 5);
    assert_eq!(agg.links.to_native(), 3, "A->C, B->C and helper->C; wiring is not a link");
}

#[test]
fn visibility_roundtrips_and_defaults_to_unknown() {
    let mut b = GraphBuilder::new("proj");
    let t = b.add_node(b.root(), NodeKind::Type, SubKind::Class, "T", Lang::Java);
    let m = b.add_node(t, NodeKind::Member, SubKind::Method, "run", Lang::Java);
    let h = b.add_node(t, NodeKind::Member, SubKind::Method, "helper", Lang::Java);
    b.set_visibility(m, Visibility::Public);
    b.set_visibility(h, Visibility::Private);
    assert_eq!(b.visibility(h), Visibility::Private);
    let (_d, file) = roundtrip(b);
    let g = file.graph();
    let t = find(g, g.root(), "T");
    assert_eq!(g.visibility(find(g, t, "run")), Visibility::Public);
    assert_eq!(g.visibility(find(g, t, "helper")), Visibility::Private);
    assert_eq!(g.visibility(t), Visibility::Unknown);
    assert_eq!(Visibility::Protected.uml(), "#");
}

#[test]
fn fields_are_stored_apart_from_the_graph() {
    let mut b = GraphBuilder::new("proj");
    let t = b.add_node(b.root(), NodeKind::Type, SubKind::Class, "T", Lang::Java);
    let m = b.add_node(t, NodeKind::Member, SubKind::Method, "run", Lang::Java);
    let f = b.add_node(t, NodeKind::Member, SubKind::Field, "count", Lang::Java);
    b.set_detail(f, "Map<String, Integer>");
    b.set_visibility(f, Visibility::Private);
    b.set_span(f, "T.java", 7, 7);
    b.add_edge(m, f, EdgeKind::Calls, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    let (_d, file) = roundtrip(b);
    let g = file.graph();
    let t = find(g, g.root(), "T");
    let kids: Vec<&str> = g.children(t).map(|c| g.name(c)).collect();
    assert_eq!(kids, ["run"], "fields are not nodes of the map");
    let fields: Vec<(&str, &str, Visibility, u32)> = g.fields(t).map(|f| (f.name, f.ty, f.visibility, f.line)).collect();
    assert_eq!(fields, [("count", "Map<String, Integer>", Visibility::Private, 7)]);
    assert_eq!(g.fields(find(g, t, "run")).count(), 0);
    assert!(g.out_edges(find(g, t, "run")).is_empty(), "edges to fields are dropped");
}

#[test]
fn complexity_is_kept_for_functions_and_defaults_to_one() {
    let mut b = GraphBuilder::new("proj");
    let t = b.add_node(b.root(), NodeKind::Type, SubKind::Class, "T", Lang::Java);
    let hard = b.add_node(t, NodeKind::Member, SubKind::Method, "hard", Lang::Java);
    b.add_node(t, NodeKind::Member, SubKind::Method, "easy", Lang::Java);
    b.set_complexity(hard, 17);
    let (_d, file) = roundtrip(b);
    let g = file.graph();
    let t = find(g, g.root(), "T");
    assert_eq!(g.complexity(find(g, t, "hard")), Some(17));
    assert_eq!(g.complexity(find(g, t, "easy")), Some(1));
    assert_eq!(g.complexity(t), None, "only functions have a complexity");
}

#[test]
fn a_call_edge_keeps_the_line_of_its_first_call() {
    let (mut b, [_, _, _, f, g, h]) = two_module_graph();
    b.set_span(f, "T.java", 5, 20);
    b.add_edge_at(f, g, EdgeKind::Calls, Scope::Main, Confidence::Resolved, EdgeFlags::NONE, 12);
    b.add_edge_at(f, g, EdgeKind::Calls, Scope::Main, Confidence::Resolved, EdgeFlags::NONE, 7);
    b.add_edge(f, h, EdgeKind::Calls, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    let (_d, file) = roundtrip(b);
    let gr = file.graph();
    let at = |path: &[&str]| path.iter().fold(gr.root(), |n, name| find(gr, n, name));
    let (f, g, h) = (at(&["m1", "p1", "T", "f"]), at(&["m2", "p3", "U", "g"]), at(&["m1", "p2", "V", "h"]));
    let edge = |to: NodeId| gr.out_edges(f).iter().find(|e| e.to.to_native() == to).unwrap();
    assert_eq!(gr.call_line(f, edge(g)), Some(7));
    assert_eq!(edge(g).count.to_native(), 2);
    assert_eq!(gr.call_line(f, edge(h)), None, "no line recorded");
}
