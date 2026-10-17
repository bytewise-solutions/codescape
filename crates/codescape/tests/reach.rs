use codescape::reach::{ReachSet, compute_reach};
use codescape_format::*;

#[test]
fn reach_marks_ancestors_and_follows_dispatch() {
    let mut b = GraphBuilder::new("p");
    let m = |b: &mut GraphBuilder, name: &str| b.add_node(b.root(), NodeKind::Module, SubKind::None, name, Lang::Java);
    let desktop = m(&mut b, "desktop");
    let client = m(&mut b, "client");
    let core = m(&mut b, "core");
    let other = m(&mut b, "other");
    let ty = |b: &mut GraphBuilder, parent: NodeId, name: &str| {
        let p = b.add_node(parent, NodeKind::Package, SubKind::None, "p", Lang::Java);
        b.add_node(p, NodeKind::Type, SubKind::Class, name, Lang::Java)
    };
    let launcher = ty(&mut b, desktop, "Launcher");
    let main = b.add_node(launcher, NodeKind::Member, SubKind::Method, "main", Lang::Java);
    let app = ty(&mut b, client, "App");
    let render = b.add_node(app, NodeKind::Member, SubKind::Method, "render", Lang::Java);
    let screen = ty(&mut b, core, "Screen");
    let screen_render = b.add_node(screen, NodeKind::Member, SubKind::Method, "render", Lang::Java);
    let lonely = ty(&mut b, other, "Lonely");
    b.add_edge(main, app, EdgeKind::Instantiates, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    b.add_edge(render, screen_render, EdgeKind::FrameworkDispatch, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    let _ = lonely;
    let e = b.add_entry(EntryPointSpec {
        node: main,
        kind: "main".into(),
        category: Category::Application,
        label: "Launcher".into(),
        parent: None,
        lifecycle: Lifecycle::Once,
        order: 0,
        rule: "r".into(),
        handoff: None,
    });
    let dir = tempfile::tempdir().unwrap();
    write_graph(&b.finish(), &dir.path().join("g")).unwrap();
    let f = IndexFile::open(&dir.path().join("g")).unwrap();
    let g = f.graph();
    let reach: ReachSet = compute_reach(g, e as usize);
    let module = |n: &str| g.children(g.root()).find(|&c| g.name(c) == n).unwrap();
    for name in ["desktop", "client", "core"] {
        assert!(reach.contains(module(name)), "{name} reached");
    }
    assert!(!reach.contains(module("other")));
    assert!(reach.contains(g.root()));
    assert!(reach.len() >= 10);
}
