use srcrelief::camera::Camera;
use srcrelief::canvas::*;
use srcrelief::focus::{FocusMode, Mark, UsageIndex, focus};
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
    let idx = UsageIndex::new(g);
    let f = focus(g, &idx, node(g, &["left", "left.impl", "T", "f"]), None, FocusMode::Neighbours);
    let s = build_scene(g, &mut cache, &cam, VIEW, &VisualRules::default(), Some(&f.marks));
    let right = s.boxes.iter().find(|b| b.node == node(g, &["right"])).unwrap();
    assert_eq!(right.mark, Mark::Down, "left.impl calls into right");
    assert!(!right.dimmed);
    let left = s.boxes.iter().find(|b| b.node == node(g, &["left"])).unwrap();
    assert_eq!(left.mark, Mark::Focus);
    let unrelated = build_scene(g, &mut cache, &cam, VIEW, &VisualRules::default(), Some(&focus(g, &idx, node(g, &["right", "right.impl"]), None, FocusMode::Neighbours).marks));
    assert!(unrelated.boxes.iter().find(|b| b.node == node(g, &["left"])).unwrap().dimmed);
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
    let iface = s.boxes.iter().find(|b| g.name(b.node) == "ViewManager").expect("interface drawn");
    assert_eq!(iface.sub, SubKind::Interface);
    let imp = s.boxes.iter().find(|b| g.name(b.node) == "ViewManagerImpl").expect("implementation drawn inside its interface");
    assert!(imp.rect.x >= iface.rect.x && imp.rect.y >= iface.rect.y && imp.depth > iface.depth);
    assert!(s.edges.iter().all(|e| g.name(e.a) != "ViewManagerImpl"), "no separate implements arrow once folded");
}

#[test]
fn folded_implementations_are_reachable_and_details_counted() {
    let mut b = GraphBuilder::new("p");
    let pkg = b.add_node(b.root(), NodeKind::Package, SubKind::None, "billing", Lang::Go);
    let port = b.add_node(pkg, NodeKind::Type, SubKind::Interface, "Repository", Lang::Go);
    let adapter = b.add_node(pkg, NodeKind::Type, SubKind::Struct, "pgRepository", Lang::Go);
    let price = b.add_node(pkg, NodeKind::Type, SubKind::Struct, "Price", Lang::Go);
    for t in [port, adapter, price] {
        b.set_span(t, "f", 1, 60);
    }
    let get = b.add_node(adapter, NodeKind::Member, SubKind::Method, "Get", Lang::Go);
    b.set_span(get, "f", 2, 9);
    b.add_flags(price, NodeFlags::DETAIL);
    b.add_edge(adapter, port, EdgeKind::Implements, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    let dir = tempfile::tempdir().unwrap();
    write_graph(&b.finish(), &dir.path().join("g")).unwrap();
    let f = ReliefFile::open(&dir.path().join("g")).unwrap();
    let g = f.graph();
    let mut cache = LayoutCache::new(Vec::new(), LayoutOptions::default());
    let pkg = g.children(g.root()).start;
    let adapter = g.children(pkg).find(|&c| g.name(c) == "pgRepository").unwrap();
    let port = g.children(pkg).find(|&c| g.name(c) == "Repository").unwrap();
    let inner = world_rect(g, &mut cache, adapter).expect("folded implementation has a place on the map");
    let outer = world_rect(g, &mut cache, port).unwrap();
    assert!(inner.x >= outer.x && inner.y >= outer.y && inner.x + inner.w <= outer.x + outer.w + 0.01, "inside its interface");
    let cam = Camera::fit(world_rect(g, &mut cache, pkg).unwrap(), VIEW);
    let s = scene(g, &mut cache, &cam);
    let pkg_box = s.boxes.iter().find(|b| b.node == pkg).unwrap();
    assert_eq!(pkg_box.hidden_details, 1);
    assert!(s.boxes.iter().all(|b| b.node != adapter || g.parent(b.node) == Some(pkg)));
    let mut opened = LayoutCache::new(Vec::new(), LayoutOptions { details_in: vec![pkg], ..Default::default() });
    let s = scene(g, &mut opened, &cam);
    let pkg_box = s.boxes.iter().find(|b| b.node == pkg).unwrap();
    assert_eq!((pkg_box.hidden_details, pkg_box.shown_details), (0, 1), "the chip offers to hide them again");
    assert!(s.boxes.iter().any(|b| g.name(b.node) == "Price"));
}

#[test]
fn scene_edges_carry_their_link_count() {
    let fx = fixture();
    let g = fx.file.graph();
    let mut cache = LayoutCache::new(Vec::new(), LayoutOptions::default());
    let cam = fit_all(g, &mut cache);
    let s = scene(g, &mut cache, &cam);
    let (left, right) = (node(g, &["left"]), node(g, &["right"]));
    let e = s.edges.iter().find(|e| e.a == left && e.b == right).unwrap();
    assert_eq!(e.links, 1);
}

#[test]
fn a_type_zoomed_close_shows_its_file_instead_of_its_members() {
    let fx = fixture();
    let g = fx.file.graph();
    let mut cache = LayoutCache::new(Vec::new(), LayoutOptions::default());
    let t = node(g, &["left", "left.api", "T"]);
    let close = Camera::fit(world_rect(g, &mut cache, t).unwrap(), VIEW);
    let s = scene(g, &mut cache, &close);
    let b = s.boxes.iter().find(|b| b.node == t).unwrap();
    assert!(b.source && !b.open);
    assert!(s.boxes.iter().all(|x| g.parent(x.node) != Some(t)), "no member boxes over the file");
    let far = fit_all(g, &mut cache);
    let s = scene(g, &mut cache, &far);
    assert!(s.boxes.iter().all(|b| !b.source));
}

#[test]
fn only_the_type_being_zoomed_into_opens_as_a_file() {
    let fx = fixture();
    let g = fx.file.graph();
    let mut cache = LayoutCache::new(Vec::new(), LayoutOptions::default());
    let api = node(g, &["left", "left.api", "T"]);
    let imp = node(g, &["left", "left.impl", "T"]);
    let r = world_rect(g, &mut cache, api).unwrap();
    let wide = Camera::fit(r, Rect::new(0.0, 0.0, 1200.0 * 0.35, 800.0 * 0.35));
    let at = |p: [f32; 2]| build_scene_at(g, &mut LayoutCache::new(Vec::new(), LayoutOptions::default()), &wide, VIEW, &VisualRules::default(), None, p, &[]);
    let s = at([600.0, 400.0]);
    assert!(s.boxes.iter().all(|b| !b.source), "a type filling a third of the view stays boxes");
    let close = Camera::fit(r, VIEW);
    let elsewhere = build_scene_at(g, &mut cache, &close, VIEW, &VisualRules::default(), None, [-50.0, -50.0], &[]);
    assert!(elsewhere.boxes.iter().all(|b| !b.source), "the pointer is not on it");
    let on = build_scene_at(g, &mut cache, &close, VIEW, &VisualRules::default(), None, [600.0, 400.0], &[]);
    assert_eq!(on.boxes.iter().filter(|b| b.source).map(|b| b.node).collect::<Vec<_>>(), [api]);
    assert!(on.boxes.iter().all(|b| b.node != imp || !b.source));
}

#[test]
fn an_open_file_stays_open_until_zoomed_out() {
    let fx = fixture();
    let g = fx.file.graph();
    let mut cache = LayoutCache::new(Vec::new(), LayoutOptions::default());
    let api = node(g, &["left", "left.api", "T"]);
    let close = Camera::fit(world_rect(g, &mut cache, api).unwrap(), VIEW);
    let away = build_scene_at(g, &mut cache, &close, VIEW, &VisualRules::default(), None, [-50.0, -50.0], &[api]);
    assert_eq!(away.boxes.iter().filter(|b| b.source).map(|b| b.node).collect::<Vec<_>>(), [api], "moving the pointer off keeps it open");
    let half = Camera { zoom: close.zoom * 0.4, ..close };
    let out = build_scene_at(g, &mut cache, &half, VIEW, &VisualRules::default(), None, [600.0, 400.0], &[api]);
    assert!(out.boxes.iter().all(|b| !b.source), "zooming out closes it");
}

#[test]
fn a_class_with_a_nested_class_still_opens_as_a_file() {
    let mut b = GraphBuilder::new("p");
    let pkg = b.add_node(b.root(), NodeKind::Package, SubKind::None, "chat", Lang::Java);
    let outer = b.add_node(pkg, NodeKind::Type, SubKind::Class, "HistoryActor", Lang::Java);
    b.set_span(outer, "HistoryActor.java", 1, 180);
    let m = b.add_node(outer, NodeKind::Member, SubKind::Method, "addMessage", Lang::Java);
    b.set_span(m, "HistoryActor.java", 10, 40);
    let inner = b.add_node(outer, NodeKind::Type, SubKind::Class, "ChatChannelsModel", Lang::Java);
    b.set_span(inner, "HistoryActor.java", 150, 175);
    let dir = tempfile::tempdir().unwrap();
    write_graph(&b.finish(), &dir.path().join("g")).unwrap();
    let f = ReliefFile::open(&dir.path().join("g")).unwrap();
    let g = f.graph();
    let mut cache = LayoutCache::new(Vec::new(), LayoutOptions { show_details: true, ..Default::default() });
    let outer = g.children(g.children(g.root()).start).start;
    let cam = Camera::fit(world_rect(g, &mut cache, outer).unwrap(), VIEW);
    let s = scene(g, &mut cache, &cam);
    assert!(s.boxes.iter().any(|b| b.node == outer && b.source), "the nested class is part of the same file");
}

#[test]
fn methods_are_rows_of_their_card_and_free_functions_stay_boxes() {
    let mut b = GraphBuilder::new("p");
    let pkg = b.add_node(b.root(), NodeKind::Package, SubKind::None, "billing", Lang::Go);
    let t = b.add_node(pkg, NodeKind::Type, SubKind::Struct, "Service", Lang::Go);
    b.set_span(t, "s.go", 10, 90);
    for (name, sub, line) in [("Charge", SubKind::Method, 40), ("Refund", SubKind::Method, 60), ("NewService", SubKind::Constructor, 20)] {
        let m = b.add_node(t, NodeKind::Member, sub, name, Lang::Go);
        b.set_span(m, "s.go", line, line + 10);
    }
    let helper = b.add_node(pkg, NodeKind::Member, SubKind::Function, "round", Lang::Go);
    b.set_span(helper, "s.go", 95, 99);
    let dir = tempfile::tempdir().unwrap();
    write_graph(&b.finish(), &dir.path().join("g")).unwrap();
    let f = ReliefFile::open(&dir.path().join("g")).unwrap();
    let g = f.graph();
    let mut cache = LayoutCache::new(Vec::new(), LayoutOptions { show_details: true, ..Default::default() });
    let pkg = g.children(g.root()).start;
    let t = g.children(pkg).find(|&c| g.name(c) == "Service").unwrap();
    let cam = Camera::fit(world_rect(g, &mut cache, t).unwrap(), VIEW);
    let s = build_scene_at(g, &mut cache, &cam, VIEW, &VisualRules::default(), None, [-50.0, -50.0], &[]);
    let card = s.boxes.iter().find(|b| b.node == t).unwrap();
    let rows: Vec<&str> = card.rows.iter().map(|r| g.name(r.node)).collect();
    assert_eq!(rows, ["NewService", "Charge", "Refund"], "constructor first, then source order");
    assert!(s.boxes.iter().all(|b| g.parent(b.node) != Some(t)), "no method boxes");
    let r = card.rows[1].rect;
    assert_eq!(s.row_at([r.x + 2.0, r.y + 2.0]).map(|n| g.name(n)), Some("Charge"));
    let far = fit_all(g, &mut cache);
    let s = scene(g, &mut cache, &far);
    assert!(s.boxes.iter().any(|b| g.name(b.node) == "round"), "a function outside any type is still a box");
}

#[test]
fn a_card_showing_rows_has_a_header_not_a_corner_title() {
    let fx = fixture();
    let g = fx.file.graph();
    let mut cache = LayoutCache::new(Vec::new(), LayoutOptions::default());
    let t = node(g, &["left", "left.api", "T"]);
    let cam = Camera::fit(world_rect(g, &mut cache, t).unwrap(), VIEW);
    let s = build_scene_at(g, &mut cache, &cam, VIEW, &VisualRules::default(), None, [-50.0, -50.0], &[]);
    let card = s.boxes.iter().find(|b| b.node == t).unwrap();
    assert!(!card.rows.is_empty());
    assert!(s.labels.iter().all(|l| l.node != t), "the card draws its own UML header instead of a corner title");
}

#[test]
fn rows_start_at_the_card_left_edge_even_with_boxes_inside() {
    let mut b = GraphBuilder::new("p");
    let pkg = b.add_node(b.root(), NodeKind::Package, SubKind::None, "jobs", Lang::Go);
    let port = b.add_node(pkg, NodeKind::Type, SubKind::Interface, "JobService", Lang::Go);
    b.set_span(port, "a.go", 1, 400);
    for (i, m) in ["Find", "Purge"].iter().enumerate() {
        let n = b.add_node(port, NodeKind::Member, SubKind::Method, m, Lang::Go);
        b.set_span(n, "a.go", 2 + i as u32, 2 + i as u32);
    }
    let imp = b.add_node(pkg, NodeKind::Type, SubKind::Struct, "jobService", Lang::Go);
    b.set_span(imp, "b.go", 1, 10);
    b.add_edge(imp, port, EdgeKind::Implements, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    let dir = tempfile::tempdir().unwrap();
    write_graph(&b.finish(), &dir.path().join("g")).unwrap();
    let f = ReliefFile::open(&dir.path().join("g")).unwrap();
    let g = f.graph();
    let mut cache = LayoutCache::new(Vec::new(), LayoutOptions { show_details: true, ..Default::default() });
    let port = g.children(g.children(g.root()).start).find(|&c| g.name(c) == "JobService").unwrap();
    let cam = Camera::fit(world_rect(g, &mut cache, port).unwrap(), VIEW);
    let s = build_scene_at(g, &mut cache, &cam, VIEW, &VisualRules::default(), None, [-50.0, -50.0], &[]);
    let card = s.boxes.iter().find(|b| b.node == port).unwrap();
    assert!(card.open && !card.rows.is_empty());
    assert!(card.rows[0].rect.x - card.rect.x < card.rect.w * 0.1, "rows hug the left edge: card {:?} row {:?}", card.rect, card.rows[0].rect);
}

#[test]
fn cards_list_fields_above_methods() {
    let mut b = GraphBuilder::new("p");
    let pkg = b.add_node(b.root(), NodeKind::Package, SubKind::None, "billing", Lang::Java);
    let t = b.add_node(pkg, NodeKind::Type, SubKind::Class, "Invoice", Lang::Java);
    b.set_span(t, "I.java", 1, 90);
    let m = b.add_node(t, NodeKind::Member, SubKind::Method, "total", Lang::Java);
    b.set_span(m, "I.java", 20, 30);
    for (name, ty) in [("lines", "List<Line>"), ("currency", "String")] {
        let f = b.add_node(t, NodeKind::Member, SubKind::Field, name, Lang::Java);
        b.set_detail(f, ty);
        b.set_visibility(f, Visibility::Private);
        b.set_span(f, "I.java", 3, 3);
    }
    let dir = tempfile::tempdir().unwrap();
    write_graph(&b.finish(), &dir.path().join("g")).unwrap();
    let f = ReliefFile::open(&dir.path().join("g")).unwrap();
    let g = f.graph();
    let mut cache = LayoutCache::new(Vec::new(), LayoutOptions { show_details: true, ..Default::default() });
    let t = g.children(g.children(g.root()).start).start;
    let cam = Camera::fit(world_rect(g, &mut cache, t).unwrap(), VIEW);
    let s = build_scene_at(g, &mut cache, &cam, VIEW, &VisualRules::default(), None, [-50.0, -50.0], &[]);
    let card = s.boxes.iter().find(|b| b.node == t).unwrap();
    let names: Vec<&str> = card.field_rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["lines", "currency"]);
    assert_eq!(card.field_rows[0].ty, "List<Line>");
    assert!(card.rows[0].rect.y > card.field_rows[1].rect.bottom(), "methods come after the fields compartment");
}
