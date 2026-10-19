use codescape::links::calls_between;
use codescape_format::*;
use codescape_layout::LayoutOptions;

fn two_packages() -> (tempfile::TempDir, IndexFile) {
    let mut b = GraphBuilder::new("p");
    let mut pkg = |name: &str| b.add_node(b.root(), NodeKind::Package, SubKind::None, name, Lang::Java);
    let (a, bp) = (pkg("a"), pkg("b"));
    let x = b.add_node(a, NodeKind::Type, SubKind::Class, "X", Lang::Java);
    let y = b.add_node(bp, NodeKind::Type, SubKind::Class, "Y", Lang::Java);
    let run = b.add_node(x, NodeKind::Member, SubKind::Method, "run", Lang::Java);
    let save = b.add_node(y, NodeKind::Member, SubKind::Method, "save", Lang::Java);
    let load = b.add_node(y, NodeKind::Member, SubKind::Method, "load", Lang::Java);
    let log = b.add_node(y, NodeKind::Member, SubKind::Method, "log", Lang::Java);
    let exec = b.add_node(x, NodeKind::Member, SubKind::Method, "exec", Lang::Java);
    b.set_span(run, "X.java", 10, 30);
    b.set_span(exec, "X.java", 40, 50);
    let call = |b: &mut GraphBuilder, to, line| b.add_edge_at(run, to, EdgeKind::Calls, Scope::Main, Confidence::Resolved, EdgeFlags::NONE, line);
    call(&mut b, save, 14);
    call(&mut b, load, 16);
    b.add_edge_at(exec, save, EdgeKind::Calls, Scope::Main, Confidence::Resolved, EdgeFlags::NONE, 45);
    b.add_edge_at(run, log, EdgeKind::Calls, Scope::Main, Confidence::Resolved, EdgeFlags::NOISE, 22);
    b.add_edge_at(run, load, EdgeKind::Calls, Scope::Test, Confidence::Resolved, EdgeFlags::NONE, 24);
    b.add_edge_at(y, run, EdgeKind::Calls, Scope::Main, Confidence::Resolved, EdgeFlags::NONE, 0);
    let dir = tempfile::tempdir().unwrap();
    write_graph(&b.finish(), &dir.path().join("g")).unwrap();
    let f = IndexFile::open(&dir.path().join("g")).unwrap();
    (dir, f)
}

fn named(g: &ArchivedGraph, name: &str) -> NodeId {
    (0..g.node_count() as NodeId).find(|&n| g.name(n) == name).unwrap()
}

#[test]
fn calls_between_lists_each_main_call_in_name_then_line_order() {
    let (_d, f) = two_packages();
    let g = f.graph();
    let links = calls_between(g, named(g, "a"), named(g, "b"), &LayoutOptions::default());
    let shown: Vec<(&str, &str, Option<u32>)> = links.iter().map(|l| (g.name(l.from), g.name(l.to), l.line)).collect();
    assert_eq!(shown, [("exec", "save", Some(45)), ("run", "save", Some(14)), ("run", "load", Some(16))]);
}

#[test]
fn calls_between_runs_one_way() {
    let (_d, f) = two_packages();
    let g = f.graph();
    let back = calls_between(g, named(g, "b"), named(g, "a"), &LayoutOptions::default());
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].line, None);
}
