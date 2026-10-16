use srcrelief_format::*;
use srcrelief_layout::*;
use srcrelief_rules::FrameRule;
use std::collections::HashMap;

struct G {
    _dir: tempfile::TempDir,
    file: ReliefFile,
    ids: HashMap<String, NodeId>,
}

fn graph(modules: &[&str], deps: &[(&str, &str, Scope)]) -> G {
    let mut b = GraphBuilder::new("p");
    let mut ids = HashMap::new();
    for m in modules {
        let id = b.add_node(b.root(), NodeKind::Module, SubKind::None, m, Lang::Java);
        let pkg = b.add_node(id, NodeKind::Package, SubKind::None, "p", Lang::Java);
        let t = b.add_node(pkg, NodeKind::Type, SubKind::Class, "T", Lang::Java);
        b.set_span(t, "f", 1, 100);
        b.set_lines(t, 100);
        ids.insert(m.to_string(), id);
    }
    for (a, c, scope) in deps {
        b.add_edge(ids[*a], ids[*c], EdgeKind::DependsOn, *scope, Confidence::Resolved, EdgeFlags::NONE);
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("g.relief");
    write_graph(&b.finish(), &path).unwrap();
    let file = ReliefFile::open(&path).unwrap();
    let g = file.graph();
    let ids = g.children(g.root()).map(|c| (g.name(c).to_string(), c)).collect();
    G { _dir: dir, file, ids }
}

fn main_deps(pairs: &[(&'static str, &'static str)]) -> Vec<(&'static str, &'static str, Scope)> {
    pairs.iter().map(|(a, b)| (*a, *b, Scope::Main)).collect()
}

fn layer_of(l: &ContainerLayout, n: NodeId) -> u32 {
    l.children.iter().find(|c| c.node == n).unwrap().layer
}

#[test]
fn layers_follow_longest_path() {
    let g = graph(&["android", "bot", "client", "core", "desktop"], &main_deps(&[("bot", "desktop"), ("desktop", "client"), ("client", "core"), ("android", "client")]));
    let gr = g.file.graph();
    let l = layout_container(gr, gr.root(), &[], &LayoutOptions::default());
    let at = |n: &str| layer_of(&l, g.ids[n]);
    assert_eq!((at("bot"), at("android"), at("desktop"), at("client"), at("core")), (0, 0, 1, 2, 3));
    assert!(l.edges.iter().all(|e| e.dir == Direction::Down));
    let rect = |n: &str| l.children.iter().find(|c| c.node == g.ids[n]).unwrap().rect;
    assert!(rect("bot").y < rect("desktop").y && rect("client").y < rect("core").y);
}

#[test]
fn test_edges_excluded_by_default() {
    let g = graph(&["core", "navigation"], &[("navigation", "core", Scope::Main), ("core", "navigation", Scope::Test)]);
    let gr = g.file.graph();
    let l = layout_container(gr, gr.root(), &[], &LayoutOptions::default());
    assert_eq!(l.edges.len(), 1);
    assert!(l.edges.iter().all(|e| e.dir == Direction::Down));
    let with = layout_container(gr, gr.root(), &[], &LayoutOptions { include_test_edges: true, ..Default::default() });
    assert_eq!(with.edges.iter().filter(|e| e.dir == Direction::Up).count(), 1);
    assert!(with.edges.iter().any(|e| e.test_only));
}

#[test]
fn cycle_breaks_and_marks_up() {
    let g = graph(&["a", "b", "c"], &main_deps(&[("a", "b"), ("b", "c"), ("c", "a")]));
    let gr = g.file.graph();
    let l = layout_container(gr, gr.root(), &[], &LayoutOptions::default());
    assert_eq!(l.edges.iter().filter(|e| e.dir == Direction::Up).count(), 1);
    assert_eq!(l.children.len(), 3);
}

#[test]
fn hidden_children_are_left_out() {
    let mut b = GraphBuilder::new("p");
    let a = b.add_node(b.root(), NodeKind::Module, SubKind::None, "a", Lang::Java);
    let ext = b.add_node(b.root(), NodeKind::Module, SubKind::None, "(external)", Lang::None);
    b.add_flags(ext, NodeFlags::EXTERNAL);
    b.add_edge(a, ext, EdgeKind::DependsOn, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    let dir = tempfile::tempdir().unwrap();
    write_graph(&b.finish(), &dir.path().join("g")).unwrap();
    let f = ReliefFile::open(&dir.path().join("g")).unwrap();
    let g = f.graph();
    let l = layout_container(g, g.root(), &[], &LayoutOptions::default());
    assert_eq!(l.children.len(), 1);
    assert!(l.edges.is_empty());
}

#[test]
fn auto_prefix_frames() {
    let g = graph(&["client", "core", "server-core", "server-game", "server-world"], &main_deps(&[("server-game", "server-core"), ("server-world", "server-core"), ("server-core", "core"), ("client", "core")]));
    let gr = g.file.graph();
    let l = layout_container(gr, gr.root(), &[FrameRule::AutoPrefix { sep: "-".into() }], &LayoutOptions::default());
    assert!(!l.frames.is_empty() && l.frames.iter().all(|f| f.label == "server"));
    for m in ["server-core", "server-game", "server-world"] {
        let r = l.children.iter().find(|c| c.node == g.ids[m]).unwrap().rect;
        assert!(l.frames.iter().any(|f| f.rect.contains_rect(&r)), "{m} outside every frame");
    }
    let client = l.children.iter().find(|c| c.node == g.ids["client"]).unwrap().rect;
    assert!(l.frames.iter().all(|f| !f.rect.contains_rect(&client)));
    let pattern = layout_container(gr, gr.root(), &[FrameRule::Pattern { glob: "c*".into(), label: "c-things".into() }], &LayoutOptions::default());
    assert_eq!(pattern.frames[0].label, "c-things");
}

#[test]
fn boxes_do_not_overlap_and_fit_size() {
    let g = graph(&["a", "b", "c", "d", "e", "f"], &main_deps(&[("a", "b"), ("a", "c"), ("b", "d"), ("c", "d"), ("e", "f")]));
    let gr = g.file.graph();
    let l = layout_container(gr, gr.root(), &[], &LayoutOptions::default());
    for (i, x) in l.children.iter().enumerate() {
        assert!(x.rect.x >= 0.0 && x.rect.y >= l.header && x.rect.right() <= l.size[0] && x.rect.bottom() <= l.size[1]);
        for y in &l.children[i + 1..] {
            assert!(!x.rect.intersects(&y.rect), "{:?} overlaps {:?}", x, y);
        }
    }
}

#[test]
fn deterministic() {
    let g = graph(&["a", "b", "c", "d"], &main_deps(&[("a", "b"), ("c", "b"), ("b", "d")]));
    let gr = g.file.graph();
    let one = layout_container(gr, gr.root(), &[], &LayoutOptions::default());
    let two = layout_container(gr, gr.root(), &[], &LayoutOptions::default());
    assert_eq!(one, two);
}

#[test]
fn cache_reuses_layouts() {
    let g = graph(&["a", "b"], &main_deps(&[("a", "b")]));
    let gr = g.file.graph();
    let mut cache = LayoutCache::new(Vec::new(), LayoutOptions::default());
    let first = cache.get_or_compute(gr, gr.root()).clone();
    assert_eq!(cache.computed(), 1);
    assert_eq!(cache.get_or_compute(gr, gr.root()), &first);
    assert_eq!(cache.computed(), 1);
    cache.invalidate_all();
    cache.get_or_compute(gr, gr.root());
    assert_eq!(cache.computed(), 2);
}

#[test]
#[cfg_attr(debug_assertions, ignore)]
fn large_flat_container_under_budget() {
    let mut b = GraphBuilder::new("p");
    let m = b.add_node(b.root(), NodeKind::Module, SubKind::None, "m", Lang::Java);
    let pkg = b.add_node(m, NodeKind::Package, SubKind::None, "big", Lang::Java);
    let types: Vec<NodeId> = (0..2000)
        .map(|i| {
            let t = b.add_node(pkg, NodeKind::Type, SubKind::Class, &format!("T{i}"), Lang::Java);
            b.set_span(t, "f", 1, 10 + (i % 300));
            t
        })
        .collect();
    let mut x: u64 = 7;
    for _ in 0..6000 {
        x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let a = types[(x >> 33) as usize % 2000];
        let c = types[(x >> 13) as usize % 2000];
        b.add_edge(a, c, EdgeKind::Calls, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    }
    let dir = tempfile::tempdir().unwrap();
    write_graph(&b.finish(), &dir.path().join("g")).unwrap();
    let f = ReliefFile::open(&dir.path().join("g")).unwrap();
    let g = f.graph();
    let pkg = g.children(g.children(g.root()).start).start;
    let t = std::time::Instant::now();
    let l = layout_container(g, pkg, &[], &LayoutOptions::default());
    let elapsed = t.elapsed();
    assert_eq!(l.children.len(), 2000);
    assert!(elapsed.as_millis() < 100, "took {elapsed:?}");
    let aspect = l.size[0] / l.size[1];
    assert!((0.3..4.0).contains(&aspect), "aspect {aspect}");
}

#[test]
fn frames_never_cover_non_members() {
    let g = graph(
        &["android", "client", "core", "desktop", "server-chat", "server-core", "server-game", "server-world"],
        &main_deps(&[("server-game", "server-core"), ("server-world", "server-core"), ("server-chat", "server-core"), ("server-core", "core"), ("desktop", "client"), ("android", "client"), ("client", "core")]),
    );
    let gr = g.file.graph();
    let l = layout_container(gr, gr.root(), &[FrameRule::AutoPrefix { sep: "-".into() }], &LayoutOptions::default());
    assert!(!l.frames.is_empty());
    for f in &l.frames {
        assert_eq!(f.label, "server");
        for c in &l.children {
            let member = gr.name(c.node).starts_with("server-");
            if !member {
                assert!(!f.rect.intersects(&c.rect), "frame covers {}", gr.name(c.node));
            }
        }
    }
    for m in ["server-chat", "server-core", "server-game", "server-world"] {
        let r = l.children.iter().find(|c| c.node == g.ids[m]).unwrap().rect;
        assert!(l.frames.iter().any(|f| f.rect.contains_rect(&r)), "{m} not framed");
    }
}

#[test]
fn edge_endpoints_spread_along_box_sides() {
    let g = graph(&["a", "b", "c", "core"], &main_deps(&[("a", "core"), ("b", "core"), ("c", "core")]));
    let gr = g.file.graph();
    let l = layout_container(gr, gr.root(), &[], &LayoutOptions::default());
    let ends: Vec<f32> = l.edges.iter().map(|e| e.points.last().unwrap()[0]).collect();
    assert_eq!(ends.len(), 3);
    assert!(ends.iter().any(|x| (x - ends[0]).abs() > 5.0), "all arrows end at the same x: {ends:?}");
    let core = l.children.iter().find(|c| c.node == g.ids["core"]).unwrap().rect;
    assert!(ends.iter().all(|&x| x >= core.x && x <= core.right()));
}

fn with_entries(modules: &[&str], deps: &[(&str, &str)], apps: &[&str]) -> G {
    let mut b = GraphBuilder::new("p");
    let mut ids = HashMap::new();
    for m in modules {
        let id = b.add_node(b.root(), NodeKind::Module, SubKind::None, m, Lang::Java);
        let pkg = b.add_node(id, NodeKind::Package, SubKind::None, "p", Lang::Java);
        let t = b.add_node(pkg, NodeKind::Type, SubKind::Class, "T", Lang::Java);
        b.set_span(t, "f", 1, 100);
        let main = b.add_node(t, NodeKind::Member, SubKind::Method, "main", Lang::Java);
        if apps.contains(m) {
            b.add_entry(EntryPointSpec { node: main, kind: "main".into(), category: Category::Application, label: m.to_string(), parent: None, lifecycle: Lifecycle::Once, order: 0, rule: "r".into(), handoff: None });
        }
        ids.insert(m.to_string(), id);
    }
    for (a, c) in deps {
        b.add_edge(ids[*a], ids[*c], EdgeKind::DependsOn, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    }
    let dir = tempfile::tempdir().unwrap();
    write_graph(&b.finish(), &dir.path().join("g.relief")).unwrap();
    let file = ReliefFile::open(&dir.path().join("g.relief")).unwrap();
    let g = file.graph();
    let ids = g.children(g.root()).map(|c| (g.name(c).to_string(), c)).collect();
    G { _dir: dir, file, ids }
}

#[test]
fn pinned_entry_owners_take_the_top_row_alphabetically() {
    let g = with_entries(&["util", "zeta", "core", "bot", "desktop", "client"], &[("bot", "desktop"), ("desktop", "client"), ("zeta", "core"), ("client", "core"), ("util", "core")], &["zeta", "bot", "desktop"]);
    let gr = g.file.graph();
    let l = layout_container(gr, gr.root(), &[], &LayoutOptions::default());
    let row = |n: &str| l.children.iter().find(|c| c.node == g.ids[n]).unwrap();
    for app in ["bot", "desktop", "zeta"] {
        assert_eq!(row(app).layer, 0, "{app}");
        assert!(row(app).pinned);
    }
    assert!(!row("util").pinned);
    let mut top: Vec<&ChildBox> = l.children.iter().filter(|c| c.pinned).collect();
    top.sort_by(|a, b| a.rect.x.total_cmp(&b.rect.x));
    let names: Vec<&str> = top.iter().map(|c| gr.name(c.node)).collect();
    assert_eq!(names, ["bot", "desktop", "zeta"]);
    let bot_desktop = l.edges.iter().find(|e| e.a == g.ids["bot"] && e.b == g.ids["desktop"]).unwrap();
    assert_eq!(bot_desktop.dir, Direction::Sideways);
    assert!(row("client").layer >= 1 && row("core").layer > row("client").layer);
}

#[test]
fn pinned_edges_do_not_hang() {
    let g = with_entries(&["a", "b", "c"], &[("a", "b"), ("b", "a"), ("b", "c"), ("c", "a")], &["a", "b"]);
    let gr = g.file.graph();
    let l = layout_container(gr, gr.root(), &[], &LayoutOptions::default());
    assert_eq!(l.children.len(), 3);
    assert!(l.children.iter().filter(|c| c.pinned).all(|c| c.layer == 0));
}

fn wired_graph() -> G {
    let mut b = GraphBuilder::new("p");
    let mut ids = HashMap::new();
    for m in ["cmd", "billing", "auth", "core"] {
        let id = b.add_node(b.root(), NodeKind::Package, SubKind::None, m, Lang::Go);
        let f = b.add_node(id, NodeKind::Member, SubKind::Function, "f", Lang::Go);
        b.set_span(f, "f", 1, 40);
        ids.insert(m, (id, f));
    }
    let wire = b.add_node(ids["cmd"].0, NodeKind::Member, SubKind::Function, "initializeApp", Lang::Go);
    b.add_flags(wire, NodeFlags::WIRING);
    for m in ["billing", "auth", "core"] {
        b.add_edge(wire, ids[m].1, EdgeKind::Calls, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    }
    b.add_edge(ids["billing"].1, ids["core"].1, EdgeKind::Calls, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    b.add_edge(ids["auth"].1, ids["billing"].1, EdgeKind::Calls, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    let dir = tempfile::tempdir().unwrap();
    write_graph(&b.finish(), &dir.path().join("g.relief")).unwrap();
    let file = ReliefFile::open(&dir.path().join("g.relief")).unwrap();
    let g = file.graph();
    let ids = g.children(g.root()).map(|c| (g.name(c).to_string(), c)).collect();
    G { _dir: dir, file, ids }
}

#[test]
fn wiring_never_moves_boxes() {
    let g = wired_graph();
    let gr = g.file.graph();
    let hidden = layout_container(gr, gr.root(), &[], &LayoutOptions::default());
    assert!(hidden.edges.iter().all(|e| e.wiring == 0), "wiring arrows hidden by default");
    let shown = layout_container(gr, gr.root(), &[], &LayoutOptions { show_wiring: true, ..Default::default() });
    assert_eq!(shown.edges.iter().filter(|e| e.wiring > 0).count(), 3);
    let rects = |l: &ContainerLayout| -> Vec<(NodeId, u32)> {
        let mut v: Vec<(NodeId, u32)> = l.children.iter().map(|c| (c.node, c.layer)).collect();
        v.sort();
        v
    };
    assert_eq!(rects(&hidden), rects(&shown));
    let cmd = hidden.children.iter().find(|c| c.node == g.ids["cmd"]).unwrap();
    assert_eq!(cmd.layer, 0);
    let core = hidden.children.iter().find(|c| c.node == g.ids["core"]).unwrap();
    assert_eq!(core.layer, 2, "auth -> billing -> core, wiring does not stretch it");
}

#[test]
fn constructors_first_in_their_layer() {
    let mut b = GraphBuilder::new("p");
    let t = b.add_node(b.root(), NodeKind::Type, SubKind::Struct, "checkoutService", Lang::Go);
    for (name, sub) in [("Run", SubKind::Method), ("Alpha", SubKind::Method), ("NewCheckoutService", SubKind::Constructor)] {
        let m = b.add_node(t, NodeKind::Member, sub, name, Lang::Go);
        b.set_span(m, "f", 1, 5);
    }
    let dir = tempfile::tempdir().unwrap();
    write_graph(&b.finish(), &dir.path().join("g")).unwrap();
    let f = ReliefFile::open(&dir.path().join("g")).unwrap();
    let g = f.graph();
    let t = g.children(g.root()).start;
    let l = layout_container(g, t, &[], &LayoutOptions::default());
    let leftmost = l.children.iter().min_by(|a, b| (a.layer, a.rect.y as i64, a.rect.x as i64).cmp(&(b.layer, b.rect.y as i64, b.rect.x as i64))).unwrap();
    assert_eq!(g.name(leftmost.node), "NewCheckoutService");
}

#[test]
fn swap_removes_obvious_crossing() {
    let mut b = GraphBuilder::new("p");
    let t = b.add_node(b.root(), NodeKind::Type, SubKind::Class, "TickClock", Lang::Java);
    let mut ids = HashMap::new();
    for name in ["<init>", "tick", "getRate", "getElapsed", "elapsedMillis", "advance", "setRate", "awaitReady"] {
        let m = b.add_node(t, NodeKind::Member, SubKind::Method, name, Lang::Java);
        b.set_span(m, "f", 1, 6);
        ids.insert(name, m);
    }
    for (a, c) in [("<init>", "setRate"), ("tick", "advance"), ("tick", "elapsedMillis"), ("elapsedMillis", "awaitReady")] {
        b.add_edge(ids[a], ids[c], EdgeKind::Calls, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    }
    let dir = tempfile::tempdir().unwrap();
    write_graph(&b.finish(), &dir.path().join("g")).unwrap();
    let f = ReliefFile::open(&dir.path().join("g")).unwrap();
    let g = f.graph();
    let t = g.children(g.root()).start;
    let l = layout_container(g, t, &[], &LayoutOptions::default());
    assert_eq!(count_crossings(&l), 0);
}

#[test]
fn random_graphs_have_30_percent_fewer_crossings() {
    use srcrelief_layout::order::{Ordering, barycenter_poc, crossings, minimize};
    let mut seed: u64 = 42;
    let mut rnd = |n: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % n as u64) as usize
    };
    let (mut before, mut after) = (0usize, 0usize);
    for _ in 0..40 {
        let layers_n = 4 + rnd(5);
        let mut layers: Vec<Vec<usize>> = Vec::new();
        let mut layer_of = Vec::new();
        let mut next = 0;
        for l in 0..layers_n {
            let width = 3 + rnd(8);
            layers.push((next..next + width).collect());
            layer_of.extend(std::iter::repeat_n(l as u32, width));
            next += width;
        }
        let mut edges = Vec::new();
        for l in 0..layers_n - 1 {
            for &u in &layers[l] {
                for _ in 0..1 + rnd(3) {
                    let v = layers[l + 1][rnd(layers[l + 1].len())];
                    edges.push((u, v));
                }
            }
        }
        let poc = barycenter_poc(&layers, &layer_of, &edges);
        let best = minimize(&layers, &layer_of, &edges, &Ordering::default());
        before += crossings(&poc, &layer_of, &edges);
        after += crossings(&best, &layer_of, &edges);
    }
    assert!(after * 10 <= before * 7, "crossings {before} -> {after}");
}

#[test]
fn pinned_row_never_wraps() {
    let apps: Vec<String> = (0..14).map(|i| format!("app{i:02}")).collect();
    let mut names: Vec<&str> = apps.iter().map(|s| s.as_str()).collect();
    names.push("core");
    let deps: Vec<(&str, &str)> = apps.iter().map(|a| (a.as_str(), "core")).collect();
    let g = with_entries(&names, &deps, &names[..14]);
    let gr = g.file.graph();
    let l = layout_container(gr, gr.root(), &[], &LayoutOptions::default());
    let ys: Vec<f32> = l.children.iter().filter(|c| c.pinned).map(|c| c.rect.y).collect();
    assert_eq!(ys.len(), 14);
    assert!(ys.iter().all(|&y| (y - ys[0]).abs() < 0.5), "pinned boxes share one row: {ys:?}");
}

#[test]
fn squeezed_layers_keep_forward_edges_down() {
    let names: Vec<String> = (0..12).map(|i| format!("n{i:02}")).collect();
    let refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
    let deps: Vec<(&str, &str, Scope)> = refs.windows(2).map(|w| (w[0], w[1], Scope::Main)).collect();
    let g = graph(&refs, &deps);
    let gr = g.file.graph();
    let l = layout_container(gr, gr.root(), &[], &LayoutOptions::default());
    assert!(l.children.iter().map(|c| c.layer).max().unwrap() < 11, "a 12-long chain is squeezed");
    assert!(l.edges.iter().all(|e| e.dir == Direction::Down), "{:?}", l.edges.iter().map(|e| e.dir).collect::<Vec<_>>());
}

#[test]
fn frames_skip_non_members_between_pinned_and_unpinned_members() {
    let g = with_entries(&["android", "server-chat", "server-game", "server-world", "core"], &[("server-game", "core"), ("server-world", "core"), ("android", "core"), ("server-chat", "core")], &["server-game", "server-world"]);
    let gr = g.file.graph();
    let l = layout_container(gr, gr.root(), &[FrameRule::AutoPrefix { sep: "-".into() }], &LayoutOptions::default());
    let android = l.child(g.ids["android"]).unwrap().rect;
    assert!(l.frames.iter().all(|f| !f.rect.intersects(&android)), "android sits between server members but is not one");
    for m in ["server-chat", "server-game", "server-world"] {
        let r = l.child(g.ids[m]).unwrap().rect;
        assert!(l.frames.iter().any(|f| f.rect.contains_rect(&r)), "{m} framed");
    }
}
