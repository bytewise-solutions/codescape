use srcrelief::camera::Camera;
use srcrelief::canvas::*;
use srcrelief::reach::ReachSet;
use srcrelief_format::*;
use srcrelief_layout::{LayoutCache, LayoutOptions, Rect};
use srcrelief_rules::VisualRules;

const VIEW: Rect = Rect { x: 0.0, y: 0.0, w: 1200.0, h: 800.0 };

struct Fx {
    _dir: tempfile::TempDir,
    file: ReliefFile,
}

fn fixture() -> Fx {
    let mut b = GraphBuilder::new("p");
    let mut modules = Vec::new();
    for m in ["left", "right"] {
        let id = b.add_node(b.root(), NodeKind::Module, SubKind::None, m, Lang::Java);
        let mut pkgs = Vec::new();
        for p in ["api", "impl"] {
            let pkg = b.add_node(id, NodeKind::Package, SubKind::None, &format!("{m}.{p}"), Lang::Java);
            let t = b.add_node(pkg, NodeKind::Type, SubKind::Class, "T", Lang::Java);
            b.set_span(t, "f", 1, 400);
            let f = b.add_node(t, NodeKind::Member, SubKind::Method, "f", Lang::Java);
            b.set_span(f, "f", 2, 20);
            pkgs.push(f);
        }
        b.add_edge(pkgs[0], pkgs[1], EdgeKind::Calls, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
        modules.push((id, pkgs));
    }
    let mock = b.add_node(modules[0].0, NodeKind::Package, SubKind::None, "left.mocks", Lang::Java);
    b.add_flags(mock, NodeFlags::MOCK);
    b.add_edge(modules[0].1[1], modules[1].1[0], EdgeKind::Calls, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    let dir = tempfile::tempdir().unwrap();
    write_graph(&b.finish(), &dir.path().join("g")).unwrap();
    let file = ReliefFile::open(&dir.path().join("g")).unwrap();
    Fx { _dir: dir, file }
}

fn node(g: &ArchivedGraph, path: &[&str]) -> NodeId {
    let mut n = g.root();
    for p in path {
        n = g.children(n).find(|&c| g.name(c) == *p).unwrap_or_else(|| panic!("no {p}"));
    }
    n
}

fn fit_all(g: &ArchivedGraph, cache: &mut LayoutCache) -> Camera {
    let size = cache.get_or_compute(g, g.root()).size;
    Camera::fit(Rect::new(0.0, 0.0, size[0], size[1]), VIEW)
}

fn scene(g: &ArchivedGraph, cache: &mut LayoutCache, cam: &Camera) -> Scene {
    build_scene(g, cache, cam, VIEW, &VisualRules::default(), None)
}

#[test]
fn closed_when_small_open_when_large() {
    let fx = fixture();
    let g = fx.file.graph();
    let mut cache = LayoutCache::new(Vec::new(), LayoutOptions::default());
    let cam = fit_all(g, &mut cache);
    let s = scene(g, &mut cache, &cam);
    let left = node(g, &["left"]);
    let b = s.boxes.iter().find(|b| b.node == left).unwrap();
    if b.rect.w <= OPEN_THRESHOLD_PX {
        assert!(!b.open);
        assert!(s.boxes.iter().all(|x| g.parent(x.node) != Some(left)));
    }
    let world = world_rect(g, &mut cache, left).unwrap();
    let mut close = Camera::fit(world, VIEW);
    close.zoom *= 0.9;
    let s = scene(g, &mut cache, &close);
    let b = s.boxes.iter().find(|b| b.node == left).unwrap();
    assert!(b.open, "a module filling the view is open: {:?}", b.rect);
    assert!(s.boxes.iter().any(|x| x.node == node(g, &["left", "left.api"])));
}

#[test]
fn offscreen_subtrees_not_visited() {
    let fx = fixture();
    let g = fx.file.graph();
    let mut cache = LayoutCache::new(Vec::new(), LayoutOptions::default());
    let right = node(g, &["right"]);
    let world = world_rect(g, &mut cache, right).unwrap();
    let mut cam = Camera::fit(world, VIEW);
    cam.zoom *= 4.0;
    let s = scene(g, &mut cache, &cam);
    let left = node(g, &["left"]);
    assert!(s.boxes.iter().all(|b| !g.is_ancestor(left, b.node)), "nothing from left is drawn");
    assert!(s.stats.visited < g.node_count());
}

#[test]
fn hidden_flags_skip_mock_and_test_nodes() {
    let fx = fixture();
    let g = fx.file.graph();
    let mut cache = LayoutCache::new(Vec::new(), LayoutOptions::default());
    let left = node(g, &["left"]);
    let cam = Camera::fit(world_rect(g, &mut cache, left).unwrap(), VIEW);
    let s = scene(g, &mut cache, &cam);
    let mock = node(g, &["left", "left.mocks"]);
    assert!(s.boxes.iter().all(|b| b.node != mock));
}

#[test]
fn edges_only_between_siblings_of_open_containers() {
    let fx = fixture();
    let g = fx.file.graph();
    let mut cache = LayoutCache::new(Vec::new(), LayoutOptions::default());
    let cam = fit_all(g, &mut cache);
    let s = scene(g, &mut cache, &cam);
    let (left, right) = (node(g, &["left"]), node(g, &["right"]));
    assert!(s.edges.iter().any(|e| e.a == left && e.b == right), "module edge at root");
    let left_cam = Camera::fit(world_rect(g, &mut cache, left).unwrap(), VIEW);
    let s = scene(g, &mut cache, &left_cam);
    let (api, imp) = (node(g, &["left", "left.api"]), node(g, &["left", "left.impl"]));
    assert!(s.edges.iter().any(|e| e.a == api && e.b == imp));
    for e in &s.edges {
        assert_eq!(g.parent(e.a), g.parent(e.b), "edges join siblings only");
    }
}

#[test]
fn labels_dropped_below_min_size() {
    let fx = fixture();
    let g = fx.file.graph();
    let mut cache = LayoutCache::new(Vec::new(), LayoutOptions::default());
    let mut cam = fit_all(g, &mut cache);
    cam.zoom *= 0.02;
    let s = scene(g, &mut cache, &cam);
    assert!(s.labels.is_empty());
}

#[test]
fn highlight_marks_reached_boxes() {
    let fx = fixture();
    let g = fx.file.graph();
    let mut cache = LayoutCache::new(Vec::new(), LayoutOptions::default());
    let cam = fit_all(g, &mut cache);
    let mut reach = ReachSet::new(g.node_count());
    reach.insert(node(g, &["right"]));
    let s = build_scene(g, &mut cache, &cam, VIEW, &VisualRules::default(), Some(&reach));
    assert!(s.boxes.iter().find(|b| b.node == node(g, &["right"])).unwrap().highlighted);
    assert!(!s.boxes.iter().find(|b| b.node == node(g, &["left"])).unwrap().highlighted);
}

#[test]
fn deepest_open_container_under_point() {
    let fx = fixture();
    let g = fx.file.graph();
    let mut cache = LayoutCache::new(Vec::new(), LayoutOptions::default());
    let left = node(g, &["left"]);
    let cam = Camera::fit(world_rect(g, &mut cache, left).unwrap(), VIEW);
    let s = scene(g, &mut cache, &cam);
    let deepest = s.deepest_open_at(VIEW.center()).unwrap();
    assert!(g.is_ancestor(left, deepest));
    assert!(s.boxes.iter().filter(|b| b.open && b.rect.contains(VIEW.center())).all(|b| g.depth(b.node) <= g.depth(deepest)));
    let api = node(g, &["left", "left.api"]);
    let api_box = s.boxes.iter().find(|b| b.node == api).unwrap();
    let hit = s.box_at(api_box.rect.center()).unwrap();
    assert!(g.is_ancestor(api, hit), "box_at returns the deepest box under the point");
}

#[test]
fn edges_hidden_until_children_are_legible() {
    let mut b = GraphBuilder::new("p");
    let m = b.add_node(b.root(), NodeKind::Module, SubKind::None, "huge", Lang::Java);
    for i in 0..400 {
        let p = b.add_node(m, NodeKind::Package, SubKind::None, &format!("p{i}"), Lang::Java);
        let t = b.add_node(p, NodeKind::Type, SubKind::Class, "T", Lang::Java);
        b.set_span(t, "f", 1, 50);
    }
    let dir = tempfile::tempdir().unwrap();
    write_graph(&b.finish(), &dir.path().join("g")).unwrap();
    let f = ReliefFile::open(&dir.path().join("g")).unwrap();
    let g = f.graph();
    let mut cache = LayoutCache::new(Vec::new(), LayoutOptions::default());
    let huge = g.children(g.root()).start;
    let world = world_rect(g, &mut cache, huge).unwrap();
    let cam = Camera::fit(world, Rect::new(0.0, 0.0, 300.0, 200.0));
    let s = build_scene(g, &mut cache, &cam, Rect::new(0.0, 0.0, 300.0, 200.0), &VisualRules::default(), None);
    let bx = s.boxes.iter().find(|b| b.node == huge).unwrap();
    assert!(bx.open, "a big box always shows its structure");
    assert!(s.boxes.iter().any(|b| g.parent(b.node) == Some(huge)), "children drawn as a preview");
    assert!(s.edges.iter().all(|e| g.parent(e.a) != Some(huge)), "but no arrows while 400 packages share 270 px");
    assert!(s.labels.iter().all(|l| g.parent(l.node) != Some(huge)));
}

#[test]
fn long_dotted_names_keep_their_tail_when_narrow() {
    assert_eq!(fit_label("com.example.game.desktop.tools", 400.0, 12.0), "com.example.game.desktop.tools");
    assert_eq!(fit_label("com.example.game.desktop.tools", 90.0, 12.0), "…desktop.tools");
    assert_eq!(fit_label("com.example.game.desktop.tools", 20.0, 12.0), "…tools");
    assert_eq!(fit_label("ViewManagerImpl", 20.0, 12.0), "ViewManagerImpl");
}

#[test]
fn edges_carry_the_depth_of_their_container() {
    let fx = fixture();
    let g = fx.file.graph();
    let mut cache = LayoutCache::new(Vec::new(), LayoutOptions::default());
    let left = node(g, &["left"]);
    let cam = Camera::fit(world_rect(g, &mut cache, left).unwrap(), VIEW);
    let s = scene(g, &mut cache, &cam);
    for e in &s.edges {
        let a = s.boxes.iter().find(|b| b.node == e.a);
        if let Some(a) = a {
            assert_eq!(e.depth, a.depth);
        }
    }
}

#[test]
fn scene_marks_implements_and_kinds() {
    let mut b = GraphBuilder::new("p");
    let m = b.add_node(b.root(), NodeKind::Package, SubKind::None, "screens", Lang::Java);
    let iface = b.add_node(m, NodeKind::Type, SubKind::Interface, "ViewManager", Lang::Java);
    let imp = b.add_node(m, NodeKind::Type, SubKind::Class, "ViewManagerImpl", Lang::Java);
    for t in [iface, imp] {
        b.set_span(t, "f", 1, 80);
    }
    b.add_edge(imp, iface, EdgeKind::Implements, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    let dir = tempfile::tempdir().unwrap();
    write_graph(&b.finish(), &dir.path().join("g")).unwrap();
    let f = ReliefFile::open(&dir.path().join("g")).unwrap();
    let g = f.graph();
    let mut cache = LayoutCache::new(Vec::new(), LayoutOptions::default());
    let pkg = g.children(g.root()).start;
    let cam = Camera::fit(world_rect(g, &mut cache, pkg).unwrap(), VIEW);
    let s = scene(g, &mut cache, &cam);
    let e = s.edges.iter().find(|e| g.name(e.a) == "ViewManagerImpl").expect("implements edge drawn");
    assert!(e.implements > 0 && e.weight == 0);
    let kinds: Vec<SubKind> = s.boxes.iter().filter(|b| b.kind == NodeKind::Type).map(|b| b.sub).collect();
    assert!(kinds.contains(&SubKind::Interface) && kinds.contains(&SubKind::Class));
}
